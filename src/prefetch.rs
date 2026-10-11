//! Next song's stream resolved ahead of time. mpv asks yt-dlp for each song as it starts (~2 s of
//! silence on every skip); here yt-dlp runs for the *next* song while the current one plays, and an
//! mpv `on_load` hook hands over the ready link when that song loads. Anything not ready, expired,
//! or failing to open goes back to mpv's usual yt-dlp path. Resolved links are also kept on disk
//! (owner-only) until shortly before they expire, so a relaunch starts the last song at once.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use libmpv2::Mpv;
use libmpv2_sys as sys;

use crate::settings::{Settings, make_private, read_json, state_dir, valid_proxy, write_json, ytdlp_cookie_file};

/// Stream links say when they expire (`expire=`, ~6 h out); one is used until this long before.
const MARGIN: u64 = 30 * 60;
/// For a link without an `expire`, how long it's trusted.
const FALLBACK_LIFE: u64 = 3600;
/// Links kept on disk at most (the queue ahead rarely needs more).
const KEEP: usize = 50;
/// Ahead of mpv's own yt-dlp hook: lower runs first, and it has to see the song page before
/// yt-dlp rewrites it.
const PRIORITY: i32 = -1000;
const ON_LOAD: u64 = 0;
const ON_FAIL: u64 = 1;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Ready {
    url: String,
    user_agent: String,
    /// Unix seconds after which the link isn't used.
    valid_until: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// When `url` stops being usable: its own `expire` minus a margin, or a short default.
fn valid_until(url: &str, now: u64) -> u64 {
    let expire = url.split(['?', '&']).find_map(|kv| kv.strip_prefix("expire=")?.parse::<u64>().ok());
    expire.map_or(now + FALLBACK_LIFE, |e| e.saturating_sub(MARGIN))
}

fn cache_file() -> std::path::PathBuf {
    state_dir().join("streams.json")
}

fn save(ready: &HashMap<String, Ready>) {
    let path = cache_file();
    write_json(&path, ready);
    make_private(&path);
}

#[derive(Default)]
struct Shared {
    /// Video id -> resolved stream.
    ready: HashMap<String, Ready>,
    /// Ids being resolved right now.
    pending: HashSet<String>,
    /// Stream link handed to mpv -> (the song page it replaced, video id), restored if it fails.
    swapped: HashMap<String, (String, String)>,
}

pub struct Prefetch {
    shared: Arc<Mutex<Shared>>,
}

struct Handle(*mut sys::mpv_handle);
// SAFETY: an mpv client handle may be used from any one thread; only the hook thread uses it.
unsafe impl Send for Handle {}

fn get_str(h: *mut sys::mpv_handle, name: &CStr) -> Option<String> {
    // SAFETY: valid handle; mpv returns a malloc'd string (or null) that we free.
    unsafe {
        let p = sys::mpv_get_property_string(h, name.as_ptr());
        if p.is_null() {
            return None;
        }
        let s = CStr::from_ptr(p).to_string_lossy().into_owned();
        sys::mpv_free(p.cast());
        Some(s)
    }
}

fn set_str(h: *mut sys::mpv_handle, name: &CStr, value: &str) {
    let Ok(v) = CString::new(value) else { return };
    // SAFETY: valid handle and NUL-terminated strings.
    unsafe {
        sys::mpv_set_property_string(h, name.as_ptr(), v.as_ptr());
    }
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

impl Prefetch {
    pub fn start(mpv: &Mpv) -> Option<Self> {
        // SAFETY: a second client on the same core; it lives in the hook thread until shutdown.
        let h = unsafe { sys::mpv_create_client(mpv.ctx.as_ptr(), c"ytmfast-prefetch".as_ptr()) };
        if h.is_null() {
            return None;
        }
        // SAFETY: valid handle, static hook names.
        unsafe {
            sys::mpv_hook_add(h, ON_LOAD, c"on_load".as_ptr(), PRIORITY);
            sys::mpv_hook_add(h, ON_FAIL, c"on_load_fail".as_ptr(), PRIORITY);
        }
        let mut ready: HashMap<String, Ready> = read_json(&cache_file()).unwrap_or_default();
        let t = now();
        ready.retain(|_, r| r.valid_until > t);
        let shared = Arc::new(Mutex::new(Shared { ready, ..Default::default() }));
        let (s, h) = (shared.clone(), Handle(h));
        std::thread::spawn(move || {
            let h = h;
            loop {
                // SAFETY: valid handle; the event stays valid until the next wait on this handle.
                let ev = unsafe { &*sys::mpv_wait_event(h.0, -1.0) };
                if ev.event_id == sys::mpv_event_id_MPV_EVENT_SHUTDOWN {
                    break;
                }
                if ev.event_id != sys::mpv_event_id_MPV_EVENT_HOOK {
                    continue;
                }
                // SAFETY: hook events carry an mpv_event_hook.
                let hook = unsafe { &*(ev.data as *const sys::mpv_event_hook) };
                let file = get_str(h.0, c"stream-open-filename").unwrap_or_default();
                if ev.reply_userdata == ON_LOAD {
                    if let Some(crate::ipc::Link::Track(id)) = crate::ipc::parse_link(&file) {
                        let mut s = lock(&s);
                        // Kept after use: the same song again (repeat, a relaunch) starts at once too.
                        if let Some(r) = s.ready.get(&id).filter(|r| r.valid_until > now()).cloned() {
                            if !r.user_agent.is_empty() {
                                set_str(h.0, c"file-local-options/user-agent", &r.user_agent);
                            }
                            set_str(h.0, c"stream-open-filename", &r.url);
                            // Only the file loading now can still fail: older swaps are done with.
                            s.swapped.clear();
                            s.swapped.insert(r.url, (file, id));
                        }
                    }
                } else {
                    let mut s = lock(&s);
                    if let Some((page, id)) = s.swapped.remove(&file) {
                        // The ready link didn't open (expired early, another network): forget it and
                        // give mpv the song page back, yt-dlp takes over.
                        s.ready.remove(&id);
                        save(&s.ready);
                        set_str(h.0, c"stream-open-filename", &page);
                    }
                }
                // SAFETY: valid handle and the id of the hook being handled.
                unsafe {
                    sys::mpv_hook_continue(h.0, hook.id);
                }
            }
            // SAFETY: the client handle is ours and no longer used.
            unsafe { sys::mpv_destroy(h.0) };
        });
        Some(Self { shared })
    }

    /// Resolves `id`'s stream in the background unless it's ready or under way.
    pub fn want(&self, s: &Settings, id: &str) {
        {
            let mut sh = lock(&self.shared);
            let t = now();
            sh.ready.retain(|_, r| r.valid_until > t);
            if sh.ready.contains_key(id) || !sh.pending.insert(id.to_string()) {
                return;
            }
        }
        let (shared, id, s) = (self.shared.clone(), id.to_string(), s.clone());
        std::thread::spawn(move || {
            let got = resolve(&s, &id);
            let mut sh = lock(&shared);
            sh.pending.remove(&id);
            if let Some((url, user_agent)) = got {
                let valid_until = valid_until(&url, now());
                sh.ready.insert(id, Ready { url, user_agent, valid_until });
                // ponytail: drops the soonest-expiring past KEEP; a queue never needs that many ahead
                while sh.ready.len() > KEEP {
                    let Some(k) = sh.ready.iter().min_by_key(|(_, r)| r.valid_until).map(|(k, _)| k.clone()) else { break };
                    sh.ready.remove(&k);
                }
                save(&sh.ready);
            }
        });
    }
}

/// yt-dlp for one song: the audio stream link and the User-Agent it must be fetched with.
fn resolve(s: &Settings, id: &str) -> Option<(String, String)> {
    let mut cmd = crate::settings::command(crate::settings::ytdlp());
    cmd.args(["--ignore-config", "--no-warnings", "--quiet", "--no-playlist", "-f", crate::audio::ytdl_format(s.quality)]);
    cmd.args(["-O", "%(url)s", "-O", "%(http_headers.User-Agent)s"]);
    if ytdlp_cookie_file().exists() {
        cmd.arg("--cookies").arg(ytdlp_cookie_file());
    }
    if valid_proxy(s.proxy.trim()) {
        cmd.args(["--proxy", s.proxy.trim()]);
    }
    cmd.arg(format!("https://music.youtube.com/watch?v={id}"));
    let out = cmd.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let url = lines.next()?.trim().to_string();
    let ua = lines.next().unwrap_or("").trim();
    let ua = if ua == "NA" { "" } else { ua };
    url.starts_with("https://").then(|| (url, ua.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn link_lifetime() {
        assert_eq!(super::valid_until("https://x/videoplayback?a=1&expire=1000000&b=2", 5), 1000000 - super::MARGIN);
        assert_eq!(super::valid_until("https://x/no-expiry", 100), 100 + super::FALLBACK_LIFE);
    }
}
