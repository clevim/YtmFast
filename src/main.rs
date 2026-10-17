// A GUI program on Windows: no console window behind it (commands attach to the caller's, see main).
#![cfg_attr(windows, windows_subsystem = "windows")]
#[macro_use]
mod i18n;
mod audio;
mod crossfade;
mod deck;
mod heat;
mod home;
mod ipc;
mod loader;
#[cfg(target_os = "linux")]
mod mpris;
mod lyrics;
mod login;
mod notify;
#[cfg(target_os = "linux")]
mod pcm;
mod prefetch;
mod scrobble;
mod settings;
mod theme;
mod tray;
mod ui;
mod views;
mod vis;
mod winamp;

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{self, Color32, vec2};
use libmpv2::Mpv;
use rustypipe::client::{ClientType, RustyPipe, RustyPipeQuery};
use rustypipe::error::Error;
use rustypipe::model::{AlbumItem, ArtistItem, MusicArtist, MusicPlaylistItem, Thumbnail, TrackItem};
use serde_json::json;
#[cfg(target_os = "linux")]
use mpris::{MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, SeekDirection};
#[cfg(not(target_os = "linux"))]
use souvlaki::{MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, SeekDirection};
#[cfg(not(target_os = "linux"))]
use souvlaki::{MediaControls, PlatformConfig};

use settings::{LibraryCache, Played, Repeat, Session, Settings, write_json};
use crate::i18n::tr;

/// YouTube Music's id for the liked-songs auto playlist.
const LIKED_ID: &str = "LM";

/// Library playlists the app shows: not "Liked Music" (duplicates our pinned liked songs) nor
/// "New episodes" (`RDPN`, YouTube's podcast feed, which doesn't open as a playlist).
fn shown_in_library(p: &MusicPlaylistItem) -> bool {
    p.id != LIKED_ID && p.id != "RDPN"
}

pub enum Msg {
    Search(Found),
    List(List),
    Artist(MusicArtist),
    Lyrics(String, lyrics::Lyrics),
    Library(Vec<MusicPlaylistItem>, Vec<AlbumItem>),
    Liked(Vec<TrackItem>),
    NewAlbums(Vec<AlbumItem>),
    /// Home shelves, and whether YouTube answered as to a guest (a dropped session).
    Home(Vec<home::Shelf>, bool),
    /// "Most replayed" heat for a song (None: it has none, or the request failed).
    Heat(String, Option<heat::Heat>),
    /// The login was re-read from the browser at startup.
    Refreshed,
    /// More tracks for the running radio.
    Radio(Vec<TrackItem>),
    /// A radio (playlist / artist) to start playing now.
    RadioStart(Vec<TrackItem>),
    /// A library write finished; show the note and refresh the library.
    Edited(String),
    /// Login worked; Some(browser) when it came from a browser import.
    LoggedIn(Option<String>),
    /// Browsers with a YouTube session: (browser id, cookies).
    LoginScan(Vec<(String, String)>),
    /// Waiting for a browser sign-in: the session's cookies once it appears.
    LoginPoll(Option<String>),
    Account(Option<login::Account>),
    /// Background work finished with nothing to show.
    Quiet,
    /// (playlist id, (videoId, setVideoId) pairs in playlist order).
    SetIds(String, Vec<(String, String)>),
    /// `yt-dlp --version` output.
    YtDlp(String),
    Error(String),
    Media(MediaControlEvent),
    /// A command from `ytmfast <verb>` and where to send the answer.
    Ipc(String, Sender<String>),
    /// Start a radio from this track (opened from a link).
    PlayRadio(TrackItem),
    /// Play these tracks now (a playlist opened from a link).
    PlayNow(Vec<TrackItem>),
    /// The music video found for the current track (None: nothing found).
    VideoFound(Option<TrackItem>),
    /// (track id, direct video stream URL) for the video view.
    VideoUrl(String, Option<String>),
    /// Skin download finished (file name or error).
    SkinReady(Result<String, String>),
    Tray(tray::TrayCmd),
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum View {
    Home,
    Results,
    List,
    Artist,
    Queue,
    Lyrics,
    Video,
    Settings,
    Login,
}

#[derive(PartialEq, Clone, Copy, Default)]
enum ListKind {
    #[default]
    Playlist,
    Album,
    Liked,
}

#[derive(Clone, Default)]
pub struct List {
    id: String,
    kind: ListKind,
    title: String,
    cover: Vec<Thumbnail>,
    tracks: Vec<TrackItem>,
    /// Playlist id a radio can be seeded from.
    radio_id: Option<String>,
    /// YouTube's per-row ids (setVideoId), aligned with `tracks`; empty until fetched. Needed to
    /// move or remove one specific row of a playlist the user owns.
    set_ids: Vec<String>,
}

#[derive(Default)]
pub struct Found {
    tracks: Vec<TrackItem>,
    albums: Vec<AlbumItem>,
    playlists: Vec<MusicPlaylistItem>,
    artists: Vec<ArtistItem>,
}

/// One entry of the back/forward history. Rc keeps snapshots cheap.
struct Snap {
    view: View,
    list: Rc<List>,
    artist: Option<Rc<MusicArtist>>,
    found: Rc<Found>,
    query: String,
}

/// What the user asked for this frame; applied after drawing to keep borrows simple.
enum Act {
    Play(Vec<TrackItem>, usize),
    PlayNext(TrackItem),
    Radio(TrackItem),
    RadioOf(String),
    Enqueue(TrackItem),
    Jump(usize),
    Unqueue(usize),
    ClearQueue,
    Playlist(String, String),
    Album(String, String),
    Artist(String),
    Liked,
    Go(View),
    Back,
    Forward,
    Like(TrackItem),
    AddTo(String, TrackItem),
    /// Remove row `usize` (holding this track) from the open playlist.
    Remove(usize, TrackItem),
    /// Reorder the open playlist: (from, to) as final positions.
    Move(usize, usize),
    /// Reorder the queue: (from, to) as final positions.
    QueueMove(usize, usize),
    CreatePlaylist(String, Vec<String>),
    DeletePlaylist(String),
    Mini(bool),
    Shuffle,
    Repeat,
    Logout,
    /// Sign in with a session found by the scan (index into `login_scan`).
    LoginUse(usize),
    /// Open the Google sign-in page in a browser and wait for the session to appear there.
    LoginGoogle,
    LoginCancel,
    ClearCache,
    ClearHistory,
    /// Index into `winamp::GALLERY`.
    DownloadSkin(usize),
    /// Look up the music video of the playing track and switch to it.
    FindVideo,
    Quit,
}

#[derive(Default)]
struct Player {
    pos: i64,
    paused: bool,
    time: f64,
    dur: f64,
    vol: f64,
}

struct App {
    ctx: egui::Context,
    waker: Waker,
    /// The window closed into the tray (not quitting): keep running headless.
    to_tray: bool,
    /// Something asked for the window (tray, `ytmfast`, media "raise").
    want_show: bool,
    rt: tokio::runtime::Runtime,
    rp: RustyPipe,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    mpv: &'static Mpv,
    /// Created on first use and dropped when off: mpv's GL renderer costs ~175 MB.
    vis: Option<vis::Vis>,
    gpa: Option<vis::Gpa>,
    ytdlp_version: String,
    skin: Option<(String, winamp::Skin)>,
    /// Real PCM capture of our own stream (PipeWire) for the Winamp analyser.
    #[cfg(target_os = "linux")]
    pw: Option<pcm::PwTap>,
    #[cfg(target_os = "linux")]
    pw_ok: bool,
    pw_retry: f64,
    /// Latest audio-tap samples (empty when no new frame arrived).
    pcm: Vec<f32>,
    bars: [f32; 19],
    /// Video view active: mpv decodes the video stream into `vis`.
    video_on: bool,
    /// Track whose video stream was asked for (the view adds it as an extra track).
    video_for: Option<String>,
    /// Video shown full screen (F).
    vis_full: bool,
    #[cfg(target_os = "linux")]
    controls: Option<mpris::Controls>,
    #[cfg(not(target_os = "linux"))]
    controls: Option<MediaControls>,
    has_tray: bool,
    tray: Option<tray::TrayHandle>,
    s: Settings,
    applied: Settings,
    applied_theme: Option<(String, bool)>,
    logged_in: bool,
    pl: Player,
    view: View,
    /// When the current view's content appeared, drives the enter animation.
    shown_at: f64,
    back: Vec<Snap>,
    fwd: Vec<Snap>,
    status: String,
    query: String,
    focus_search: bool,
    found: Rc<Found>,
    list: Rc<List>,
    artist: Option<Rc<MusicArtist>>,
    /// (track id, lyrics) — None body = still loading or none available.
    lyrics: Option<(String, Option<lyrics::Lyrics>)>,
    /// Lyrics timing nudge for one track: (track id, seconds; positive shows lines earlier).
    lyrics_offset: (String, f64),
    /// Lyrics side panel (the full-window view is View::Lyrics).
    lyrics_panel: bool,
    /// Theme change reveal (screenshot of the old colours).
    transition: theme::Transition,
    reveal_theme: bool,
    /// Second player for crossfades, only while the setting is on.
    fader: Option<crossfade::Fader>,
    /// Resolves the next song's stream while the current one plays (skips start at once).
    prefetch: Option<prefetch::Prefetch>,
    /// Last song handed to `prefetch`.
    prefetched: String,
    /// What the loudness-following equalizer last got (see audio::update_eq).
    eq_sent: String,
    /// Crossfade in progress for the interface: (0..1, outgoing queue position).
    xfade: Option<(f32, i64)>,
    /// Last time decoded artwork was checked against its budget (egui time).
    images_trimmed: f64,
    /// When the home feed was last asked for (egui time): refreshed every 30 min with a window.
    home_loaded: f64,
    /// Last automatic renewal of a dropped session.
    renewed_at: Option<std::time::Instant>,
    /// The playing song's "most replayed" heat: (video id, data once fetched).
    heat: Option<(String, Option<heat::Heat>)>,
    /// The heap was trimmed after this window's start-up.
    window_trimmed: bool,
    lib: (Vec<MusicPlaylistItem>, Vec<AlbumItem>),
    liked: HashSet<String>,
    liked_tracks: Vec<TrackItem>,
    new_albums: Vec<AlbumItem>,
    /// YouTube Music's home feed (personalized when logged in).
    home: Vec<home::Shelf>,
    queue: Vec<TrackItem>,
    /// Upcoming tracks in their original order while shuffle is on.
    unshuffled: Option<Vec<TrackItem>>,
    /// End of the "play next" block right after the current track.
    next_end: i64,
    radio_on: bool,
    radio_busy: bool,
    history: Vec<Played>,
    recorded: i64,
    /// Track last announced (notification, ListenBrainz "playing now"), so a reopened window or a
    /// pause doesn't announce it again.
    announced: String,
    queue_tab: usize,
    cookie_path: String,
    /// Name being typed for a new playlist, plus the tracks to seed it with.
    new_playlist: Option<(String, Vec<String>)>,
    confirm_delete: bool,
    mini: bool,
    full_size: egui::Vec2,
    sidebar_open: bool,
    show_shortcuts: bool,
    /// Sidebar library: 0 = everything, 1 = playlists, 2 = albums; plus a name filter.
    lib_filter: u8,
    lib_query: String,
    lib_search: bool,
    /// Row being dragged in a reorderable list.
    drag: Option<usize>,
    settings_tab: usize,
    /// The window was maximized when the mini player took over (restored on the way back).
    was_maximized: bool,
    /// The window closes to come back in the other mode (mini <-> full), centred by the compositor.
    reopen: bool,
    /// When the current status message appeared (it shows as a toast for a few seconds).
    status_at: f64,
    status_seen: String,
    /// Sessions found in local browsers; None until a scan finishes.
    login_scan: Option<Vec<(String, String)>>,
    scanning: bool,
    /// Browser we're waiting on for a sign-in, and since when.
    login_wait: Option<(String, std::time::Instant)>,
    login_polling: bool,
    login_poll_at: std::time::Instant,
    account: Option<login::Account>,
    art_colors: HashMap<String, Option<Color32>>,
    /// What the OS was last told: queue position, paused, track id (a new queue can keep the position).
    last: (i64, bool, String),
    acts: Vec<Act>,
    quitting: bool,
    /// Settings changed but not written yet: dragging a slider changes them every frame, the file
    /// gets them once things settle (and always when the window closes).
    unsaved_since: Option<std::time::Instant>,
}

/// Repaint handle that outlives windows: closing to the tray destroys the window and reopening
/// creates a new egui context, so background threads wake whatever window exists (if any).
#[derive(Clone, Default)]
pub struct Waker(Arc<std::sync::RwLock<Option<egui::Context>>>);

impl Waker {
    pub fn wake(&self) {
        if let Some(c) = &*self.0.read().unwrap() {
            c.request_repaint();
        }
    }
    fn set(&self, ctx: Option<egui::Context>) {
        *self.0.write().unwrap() = ctx;
    }
}

/// Saves the login cookies (already validated) owner-only, where rustypipe and yt-dlp read them.
fn store_cookies(cookies: &str) {
    let path = settings::cookie_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    settings::write_private(&path, cookies.as_bytes()).ok();
}

/// Logged in, YouTube Music wraps music videos in a song/video switch rustypipe 0.11 can't read
/// ("no video item"): asked anonymously first (same track and lyrics), the account only for what
/// needs it (a private upload).
pub async fn music_details(q: &RustyPipeQuery, id: &str) -> Result<rustypipe::model::TrackDetails, Error> {
    match q.clone().unauthenticated().music_details(id).await {
        Err(_) => q.music_details(id).await,
        ok => ok,
    }
}

/// Library writes go straight to InnerTube; rustypipe only covers reads.
async fn yt_write(q: &RustyPipeQuery, endpoint: &str, body: serde_json::Value) -> Result<serde_json::Value, Error> {
    let txt = q.raw(ClientType::DesktopMusic, endpoint, &body).await?;
    Ok(serde_json::from_str(&txt).unwrap_or_default())
}

fn shuffled(mut v: Vec<TrackItem>) -> Vec<TrackItem> {
    let s = std::collections::hash_map::RandomState::new();
    v.sort_by_cached_key(|t| s.hash_one(&t.id));
    v
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Days since a yt-dlp release ("2026.08.19" or "2026.08.19.123456" nightly), given "now" in Unix seconds.
fn ytdlp_days_old(version: &str, now: u64) -> Option<i64> {
    let mut it = version.split('.').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if !(2000..3000).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil date (Howard Hinnant's algorithm).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(now as i64 / 86_400 - days)
}

/// Walks a raw YouTube Music page for playlist rows (videoId, setVideoId), in order, plus the
/// first continuation token for the next page.
fn collect_set_ids(v: &serde_json::Value, out: &mut Vec<(String, String)>, token: &mut Option<String>) {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            if let Some(d) = m.get("playlistItemData")
                && let (Some(video), Some(set)) = (d["videoId"].as_str(), d["playlistSetVideoId"].as_str())
            {
                out.push((video.into(), set.into()));
            }
            for (k, x) in m {
                let t = match k.as_str() {
                    "continuationCommand" => x["token"].as_str(),
                    "nextContinuationData" => x["continuation"].as_str(),
                    _ => None,
                };
                if let Some(t) = t {
                    token.get_or_insert_with(|| t.to_string());
                }
                collect_set_ids(x, out, token);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_set_ids(x, out, token)),
        _ => {}
    }
}

/// Gives each track its row id, matching in order (duplicates included). None unless every
/// track is matched: a wrong id would move or delete the wrong row.
fn align_set_ids(tracks: &[&str], pairs: &[(String, String)]) -> Option<Vec<String>> {
    let mut next = 0;
    let mut out = Vec::with_capacity(tracks.len());
    for id in tracks {
        let k = pairs[next..].iter().position(|(v, _)| v == id)?;
        out.push(pairs[next + k].1.clone());
        next += k + 1;
    }
    Some(out)
}

fn yt_url(t: &TrackItem) -> String {
    format!("https://music.youtube.com/watch?v={}", t.id)
}

impl App {
    /// `cc`: the first window, or None to start headless in the tray (`--tray`).
    fn new(cc: Option<&eframe::CreationContext>) -> Self {
        let s = Settings::load();
        if settings::valid_proxy(s.proxy.trim()) {
            // SAFETY: still single-threaded here; reqwest (rustypipe) reads these when it builds its client.
            unsafe {
                std::env::set_var("HTTPS_PROXY", s.proxy.trim());
                std::env::set_var("HTTP_PROXY", s.proxy.trim());
            }
        }
        theme::install_bundled();
        let waker = Waker::default();

        // Two workers are plenty for a few HTTP requests at a time; one per core (and glibc's heap
        // arena per thread) only held memory.
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio");
        let dir = settings::data_dir();
        std::fs::create_dir_all(&dir).ok();
        // Holds the login cookie (cookies.txt and rustypipe's cache): owner-only.
        settings::make_private(&dir);
        settings::make_private(&settings::cookie_file());
        let rp = {
            let _g = rt.enter();
            let mut b = RustyPipe::builder().storage_dir(&dir);
            // YouTube's own texts (home shelf titles, view counts) in the system language and region.
            let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
            let tag = locale.split(['.', '@']).next().unwrap_or("").replace('_', "-");
            if let Ok(lang) = tag.parse::<rustypipe::param::Language>() {
                b = b.lang(lang);
            }
            if let Some(country) = tag.split('-').nth(1).and_then(|c| serde_json::from_value::<rustypipe::param::Country>(json!(c)).ok()) {
                b = b.country(country);
            }
            b.build().expect("rustypipe")
        };

        let volume = s.volume;
        let mpv = Mpv::with_initializer(|i| {
            i.set_property("vid", "no")?;
            i.set_property("vo", "libmpv")?;
            i.set_property("ytdl", "yes")?;
            i.set_property("prefetch-playlist", "yes")?;
            i.set_property("idle", "yes")?;
            i.set_property("audio-client-name", "ytmfast")?;
            // mpv's own yt-dlp hook uses the copy the package ships, when there is one (Windows,
            // macOS). The path goes into a comma / equals separated list: only a plain one.
            let ytdlp = settings::ytdlp();
            if ytdlp.components().count() > 1 && !ytdlp.to_string_lossy().contains([',', '=']) {
                i.set_property("script-opts", format!("ytdl_hook-ytdl_path={}", ytdlp.display()))?;
            }
            // Built-in Lua scripts each run their own interpreter thread; only ytdl_hook is used.
            for (k, v) in [("osc", "no"), ("load-stats-overlay", "no"), ("load-console", "no"), ("load-auto-profiles", "no"), ("load-select", "no"), ("load-positioning", "no"), ("load-commands", "no"), ("input-default-bindings", "no"), ("input-vo-keyboard", "no")] {
                i.set_property(k, v).ok();
            }
            i.set_property("volume", volume)?;
            // Video view: hardware decoding when available; letterbox bars stay black.
            i.set_property("hwdec", "auto-safe")?;
            Ok(())
        })
        .expect("libmpv");
        // Lives for the whole process; the render context borrows it for 'static.
        let mpv: &'static Mpv = Box::leak(Box::new(mpv));
        audio::apply(mpv, &s);

        let (tx, rx) = channel();
        // Windows media controls hang off the window handle; elsewhere it's unused.
        #[cfg(target_os = "windows")]
        let hwnd = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            cc.and_then(|cc| cc.window_handle().ok()).and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(w) => Some(w.hwnd.get() as *mut std::ffi::c_void),
                _ => None,
            })
        };
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let hwnd = None;
        #[cfg(target_os = "linux")]
        let controls = {
            let (tx, waker) = (tx.clone(), waker.clone());
            mpris::Controls::spawn(&rt, move |e| {
                // Straight to mpv: a window out of focus may not be running frames to read messages.
                if !playback(mpv, &e) {
                    tx.send(Msg::Media(e)).ok();
                }
                waker.wake();
            })
        };
        tray::DIRECT
            .set(Box::new(move |c| match c {
                tray::TrayCmd::Toggle => playback(mpv, &MediaControlEvent::Toggle),
                tray::TrayCmd::Next => playback(mpv, &MediaControlEvent::Next),
                tray::TrayCmd::Prev => playback(mpv, &MediaControlEvent::Previous),
                tray::TrayCmd::Volume(n) => mpv.command("add", &["volume", &(n * 5).to_string()]).is_ok(),
                tray::TrayCmd::Show | tray::TrayCmd::Quit => false,
            }))
            .ok();
        #[cfg(not(target_os = "linux"))]
        let controls = MediaControls::new(PlatformConfig { display_name: "YtmFast", dbus_name: "ytmfast", hwnd })
            .ok()
            .and_then(|mut c| {
                let (tx, waker) = (tx.clone(), waker.clone());
                c.attach(move |e| {
                    tx.send(Msg::Media(e)).ok();
                    waker.wake();
                })
                .ok()?;
                Some(c)
            });
        let tray = tray::spawn(&rt, tx.clone(), waker.clone());
        let has_tray = tray.is_some();
        ipc::serve(tx.clone(), waker.clone());

        let cache: LibraryCache = settings::read_json(&LibraryCache::path()).unwrap_or_default();
        let history: Vec<Played> = settings::read_json(&settings::history_path()).unwrap_or_default();
        let logged_in = settings::cookie_file().exists();
        let mut app = Self {
            ctx: cc.map_or_else(egui::Context::default, |cc| cc.egui_ctx.clone()),
            waker,
            to_tray: false,
            want_show: false,
            rt,
            rp,
            tx,
            rx,
            mpv,
            vis: None,
            gpa: cc.and_then(|cc| cc.get_proc_address.clone()),
            ytdlp_version: String::new(),
            skin: None,
            #[cfg(target_os = "linux")]
            pw: None,
            #[cfg(target_os = "linux")]
            pw_ok: pcm::available(),
            pw_retry: f64::NEG_INFINITY,
            pcm: vec![],
            bars: [0.0; 19],
            video_on: false,
            video_for: None,
            vis_full: false,
            controls,
            has_tray,
            tray,
            applied: s.clone(),
            s,
            applied_theme: None,
            logged_in,
            pl: Player { pos: -1, paused: true, ..Default::default() },
            view: View::Home,
            shown_at: 0.0,
            back: vec![],
            fwd: vec![],
            status: String::new(),
            query: String::new(),
            focus_search: false,
            found: Default::default(),
            list: Default::default(),
            artist: None,
            lyrics: None,
            lyrics_offset: Default::default(),
            lyrics_panel: false,
            transition: Default::default(),
            reveal_theme: false,
            fader: None,
            prefetch: prefetch::Prefetch::start(mpv),
            prefetched: String::new(),
            eq_sent: String::new(),
            xfade: None,
            images_trimmed: 0.0,
            home_loaded: 0.0,
            renewed_at: None,
            heat: None,
            window_trimmed: false,
            liked: cache.liked.iter().map(|t| t.id.clone()).collect(),
            liked_tracks: if logged_in { cache.liked } else { vec![] },
            lib: if logged_in { (cache.playlists.into_iter().filter(shown_in_library).collect(), cache.albums) } else { Default::default() },
            new_albums: cache.new_albums,
            home: vec![],
            queue: vec![],
            unshuffled: None,
            next_end: 0,
            radio_on: false,
            radio_busy: false,
            history,
            recorded: -1,
            announced: String::new(),
            queue_tab: 0,
            cookie_path: String::new(),
            new_playlist: None,
            confirm_delete: false,
            mini: false,
            full_size: vec2(1200.0, 760.0),
            sidebar_open: true,
            show_shortcuts: false,
            lib_filter: 0,
            lib_query: String::new(),
            lib_search: false,
            drag: None,
            settings_tab: 0,
            was_maximized: false,
            reopen: false,
            status_at: 0.0,
            status_seen: String::new(),
            login_scan: None,
            scanning: false,
            login_wait: None,
            login_polling: false,
            login_poll_at: std::time::Instant::now(),
            account: None,
            art_colors: HashMap::new(),
            last: (-2, true, String::new()),
            acts: vec![],
            quitting: false,
            unsaved_since: None,
        };
        let q = app.rp.query();
        app.spawn(async move { Ok(Msg::NewAlbums(q.music_new_albums().await?)) });
        app.load_home();
        if logged_in {
            app.load_library();
        }
        app.restore_session();
        // An old yt-dlp is the usual reason YouTube playback breaks: check it once per start.
        {
            let (tx, waker) = (app.tx.clone(), app.waker.clone());
            std::thread::spawn(move || {
                let v = settings::refresh_ytdlp(|v| ytdlp_days_old(v, now_secs()));
                tx.send(Msg::YtDlp(v)).ok();
                waker.wake();
            });
        }
        if app.logged_in {
            app.fetch_account();
        }
        // Browsers rotate their session cookies; re-reading keeps our copy from going stale.
        if app.logged_in && !app.s.login_browser.is_empty() {
            // Counts as a renewal: what loads meanwhile with the old session needn't start another.
            app.renewed_at = Some(std::time::Instant::now());
            app.login_from_browser(app.s.login_browser.clone(), true);
        }
        if let Some(link) = STARTUP_LINK.get() {
            app.play_link(link).ok();
        }
        if let Some(cc) = cc {
            app.attach(cc);
        }
        app
    }

    /// Per-window setup: runs for the first window and every one reopened from the tray.
    fn attach(&mut self, cc: &eframe::CreationContext) {
        let ctx = cc.egui_ctx.clone();
        egui_extras::install_image_loaders(&ctx);
        ctx.add_bytes_loader(Arc::new(loader::DiskLoader::new(settings::cache_dir().join("art"))));
        ui::setup_fonts(&ctx);
        ctx.set_zoom_factor(self.s.zoom);
        self.waker.set(Some(ctx.clone()));
        self.ctx = ctx;
        self.gpa = cc.get_proc_address.clone();
        self.applied_theme = None;
        self.last = (-2, true, String::new());
        // Timestamps from the old context would run backwards against the new clock (at 0 again).
        self.shown_at = 0.0;
        self.images_trimmed = 0.0;
        self.window_trimmed = false;
        self.home_loaded = 0.0;
        self.pw_retry = f64::NEG_INFINITY;
    }

    /// Headless mode while closed to the tray: playback, media keys, MPRIS and `ytmfast` commands
    /// keep working. Returns true when the window should come back, false to quit.
    fn background(&mut self) -> bool {
        self.waker.set(None);
        self.want_show = false;
        self.reopen = false;
        // The window's memory is freed but glibc keeps it mapped; hand it back to the system.
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            // SAFETY: plain libc call, no pointers involved.
            unsafe {
                libc::malloc_trim(0);
            }
            page_out_code();
        }
        loop {
            match self.rx.recv_timeout(Duration::from_millis(250)) {
                Ok(msg) => self.handle(msg),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return false,
            }
            self.tick();
            if self.quitting {
                return false;
            }
            if self.want_show {
                return true;
            }
        }
    }

    /// Work shared by the window's `logic` and the headless tray loop.
    fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg);
        }
        let m = self.mpv;
        self.pl = Player {
            pos: m.get_property("playlist-pos").unwrap_or(-1),
            paused: m.get_property("pause").unwrap_or(true),
            time: m.get_property("time-pos").unwrap_or(0.0),
            dur: m.get_property("duration").unwrap_or(0.0),
            vol: m.get_property("volume").unwrap_or(100.0),
        };
        self.crossfade();
        self.sync_os();
        // The song after this one, resolved in the background (also when a radio fills the queue later).
        if let (Some(p), Some(next)) = (&self.prefetch, usize::try_from(self.pl.pos).ok().and_then(|i| self.queue.get(i + 1)))
            && self.prefetched != next.id
        {
            self.prefetched = next.id.clone();
            p.want(&self.s, &next.id);
        }
        if self.s.eq_on && self.s.eq_loudness && !audio::update_eq(self.mpv, &self.s, self.pl.vol, &mut self.eq_sent) {
            audio::apply(self.mpv, &self.s);
        }
        #[cfg(target_os = "linux")]
        if let Some(c) = self.controls.as_mut() {
            c.set_position(self.pl.time);
            c.set_volume(self.pl.vol / 100.0);
        }
        self.record_history();

        // Autoplay / endless radio: top the queue up before it runs dry.
        let p = &self.pl;
        if (self.radio_on || self.s.autoplay)
            && self.s.repeat == Repeat::Off
            && !self.radio_busy
            && p.pos >= 0
            && p.pos + 3 >= self.queue.len() as i64
            && let Some(seed) = self.queue.last().map(|t| t.id.clone())
        {
            self.radio_on = true;
            self.fetch_radio(seed);
        }
        self.poll_login();
        for act in std::mem::take(&mut self.acts) {
            self.apply(act);
        }
        self.settings_changed();
    }

    /// While waiting on a browser sign-in, re-read that browser's session every 3 s (5 min max).
    fn poll_login(&mut self) {
        let Some((browser, since)) = self.login_wait.clone() else { return };
        if since.elapsed() > Duration::from_secs(300) {
            self.login_wait = None;
            self.status = tr("O tempo para entrar acabou. Tente de novo.").into();
            return;
        }
        if self.login_polling || self.login_poll_at.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.login_polling = true;
        self.login_poll_at = std::time::Instant::now();
        self.spawn(async move {
            let found = tokio::task::spawn_blocking(move || login::import(&browser).ok()).await.ok().flatten();
            Ok(Msg::LoginPoll(found))
        });
    }

    /// Looks for signed-in YouTube sessions in the local browsers (Login page).
    fn scan_logins(&mut self) {
        if self.scanning || self.login_scan.is_some() {
            return;
        }
        self.scanning = true;
        self.spawn(async move {
            let found = tokio::task::spawn_blocking(login::scan).await.unwrap_or_default();
            Ok(Msg::LoginScan(found.into_iter().map(|(id, c)| (id.to_string(), c)).collect()))
        });
    }

    /// YouTube can stop accepting the copied session without an error (answers turn into a
    /// guest's). The login is read again from the browser it came from, at most every 10 minutes
    /// so a browser that's signed out too doesn't loop; what loaded as a guest then reloads.
    fn renew_session(&mut self) {
        if !self.logged_in || self.s.login_browser.is_empty() || self.renewed_at.is_some_and(|t| t.elapsed() < Duration::from_secs(600)) {
            return;
        }
        self.renewed_at = Some(std::time::Instant::now());
        self.login_from_browser(self.s.login_browser.clone(), true);
    }

    /// Name, handle and photo of the signed-in account, for the avatar menu.
    fn fetch_account(&self) {
        let q = self.rp.query().authenticated();
        self.spawn(async move {
            let txt = q.raw(ClientType::DesktopMusic, "account/account_menu", &json!({})).await?;
            Ok(Msg::Account(login::parse_account(&serde_json::from_str(&txt).unwrap_or_default())))
        });
    }

    /// Validates cookies with YouTube, then stores them (owner-only) for rustypipe and yt-dlp.
    /// Nothing is written unless YouTube accepts them.
    fn login_with(&self, cookies: String, browser: Option<String>, quiet: bool) {
        let rp = self.rp.clone();
        self.spawn(async move {
            rp.user_auth_set_cookie_txt(&cookies).await?;
            store_cookies(&cookies);
            Ok(if quiet { Msg::Refreshed } else { Msg::LoggedIn(browser) })
        });
    }

    /// Imports the login from a browser. `quiet` = startup refresh: failures stay silent.
    fn login_from_browser(&self, browser: String, quiet: bool) {
        let rp = self.rp.clone();
        self.spawn(async move {
            let id = browser.clone();
            let imported = tokio::task::spawn_blocking(move || login::import(&id)).await.unwrap_or_else(|e| Err(e.to_string()));
            let cookies = match imported {
                Ok(c) => c,
                Err(_) if quiet => return Ok(Msg::Quiet),
                Err(e) => return Ok(Msg::Error(e)),
            };
            match rp.user_auth_set_cookie_txt(&cookies).await {
                Ok(()) => {}
                Err(_) if quiet => return Ok(Msg::Quiet),
                Err(e) => return Err(e),
            }
            store_cookies(&cookies);
            Ok(if quiet { Msg::Refreshed } else { Msg::LoggedIn(Some(browser)) })
        });
    }

    /// Plays a YouTube / YouTube Music link: a track starts a radio, a playlist plays in order.
    fn play_link(&mut self, url: &str) -> Result<(), String> {
        let q = self.q();
        match ipc::parse_link(url).ok_or(tr("link não reconhecido"))? {
            ipc::Link::Track(id) => self.spawn(async move { Ok(Msg::PlayRadio(music_details(&q, &id).await?.track)) }),
            ipc::Link::Playlist(id) => self.spawn(async move {
                let mut pl = q.music_playlist(&id).await?;
                pl.tracks.extend_limit(&q, 2000).await?;
                Ok(Msg::PlayNow(pl.tracks.items))
            }),
        }
        self.status = tr("Abrindo link…").into();
        Ok(())
    }

    /// Answers one command-line request. Replies "ok", a value, or "erro: …".
    fn ipc(&mut self, line: &str) -> String {
        let mut words = line.split_whitespace();
        let verb = words.next().unwrap_or("");
        let arg = words.next();
        let num = arg.and_then(|v| v.parse::<f64>().ok()).filter(|n| n.is_finite());
        let m = self.mpv;
        let err = |e: &str| format!("erro: {e}");
        match verb {
            "play-pause" => self.media(MediaControlEvent::Toggle),
            "play" => self.media(MediaControlEvent::Play),
            "pause" => self.media(MediaControlEvent::Pause),
            "next" => self.media(MediaControlEvent::Next),
            "previous" => self.media(MediaControlEvent::Previous),
            "stop" => {
                m.set_property("pause", true).ok();
                m.command("seek", &["0", "absolute"]).ok();
            }
            "seek" | "seek-to" => {
                let Some(n) = num else { return err(tr("informe os segundos")) };
                let mode = if verb == "seek" { "relative" } else { "absolute" };
                m.command("seek", &[&n.to_string(), mode]).ok();
            }
            "volume" => {
                let Some(n) = num else { return err(tr("informe o volume (0-100)")) };
                self.s.volume = n.clamp(0.0, 100.0);
                m.set_property("volume", self.s.volume).ok();
            }
            "volume-up" | "volume-down" => {
                m.command("add", &["volume", if verb == "volume-up" { "5" } else { "-5" }]).ok();
            }
            "mute" => drop(m.command("cycle", &["mute"])),
            "shuffle" => self.apply(Act::Shuffle),
            "repeat" => self.apply(Act::Repeat),
            "like" => match self.current().cloned() {
                Some(t) if self.logged_in => self.toggle_like(t),
                Some(_) => return err(tr("entre na conta para curtir")),
                None => return err(tr("nada tocando")),
            },
            "show" => self.show_window(),
            "quit" => self.apply(Act::Quit),
            "now-playing" => {
                let Some(t) = self.current() else { return err(tr("nada tocando")) };
                let p = &self.pl;
                if arg == Some("--raw") {
                    return json!({
                        "id": t.id, "title": t.name, "artists": ui::artists(t),
                        "album": t.album.as_ref().map(|a| a.name.clone()),
                        "position": p.time, "duration": p.dur, "paused": p.paused, "volume": p.vol,
                    })
                    .to_string();
                }
                let state = if p.paused { tr("pausado") } else { tr("tocando") };
                return format!("{} - {} ({} / {}) [{state}]", ui::artists(t), t.name, ui::fmt_time(p.time), ui::fmt_time(p.dur));
            }
            "play-uri" => {
                let Some(url) = arg else { return err(tr("informe o link")) };
                if let Err(e) = self.play_link(url) {
                    return err(&e);
                }
                self.show_window();
            }
            other => return err(&tf!("comando desconhecido: {}", other)),
        }
        "ok".into()
    }

    fn now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    fn q(&self) -> RustyPipeQuery {
        // YouTube's own texts (home shelves) follow a language picked in settings; "auto" keeps
        // the system's, set when the client was built.
        let q = match self.s.language.as_str() {
            "en" => self.rp.query().lang(rustypipe::param::Language::En),
            "pt" => self.rp.query().lang(rustypipe::param::Language::Pt),
            _ => self.rp.query(),
        };
        if self.logged_in { q.authenticated() } else { q }
    }

    fn spawn(&self, fut: impl Future<Output = Result<Msg, Error>> + Send + 'static) {
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        self.rt.spawn(async move {
            tx.send(fut.await.unwrap_or_else(|e| Msg::Error(e.to_string()))).ok();
            waker.wake();
        });
    }

    // ---- navigation ----

    fn snap(&self) -> Snap {
        Snap { view: self.view, list: self.list.clone(), artist: self.artist.clone(), found: self.found.clone(), query: self.query.clone() }
    }

    fn restore(&mut self, s: Snap) {
        self.view = s.view;
        self.list = s.list;
        self.artist = s.artist;
        self.found = s.found;
        self.query = s.query;
        self.shown_at = self.now();
        self.confirm_delete = false;
    }

    fn go(&mut self, v: View) {
        if v != self.view {
            self.back.push(self.snap());
            self.back.truncate(50);
            self.fwd.clear();
        }
        self.view = v;
        self.shown_at = self.now();
        self.confirm_delete = false;
        self.vis_full = false;
    }

    /// Like `go`, but always records history (page content changed under the same view).
    fn go_new(&mut self, v: View) {
        self.back.push(self.snap());
        self.fwd.clear();
        self.view = v;
        self.shown_at = self.now();
        self.confirm_delete = false;
    }

    // ---- loading ----

    fn load_home(&mut self) {
        let q = self.q();
        // Home falls back to the local shelves when this fails: no error toast.
        self.home_loaded = self.ctx.input(|i| i.time);
        self.spawn(async move { Ok(home::fetch(&q).await.map_or(Msg::Home(vec![], false), |(h, guest)| Msg::Home(h, guest))) });
    }

    fn load_library(&self) {
        let q = self.q();
        self.spawn(async move {
            let (p, a) = tokio::join!(q.music_saved_playlists(), q.music_saved_albums());
            Ok(Msg::Library(p?.items, a?.items))
        });
        let q = self.q();
        self.spawn(async move {
            let mut pl = q.music_liked_tracks().await?;
            pl.tracks.extend_limit(&q, 5000).await?;
            Ok(Msg::Liked(pl.tracks.items))
        });
    }

    fn save_library_cache(&self) {
        let c = LibraryCache {
            playlists: self.lib.0.clone(),
            albums: self.lib.1.clone(),
            liked: self.liked_tracks.clone(),
            new_albums: self.new_albums.clone(),
        };
        write_json(&LibraryCache::path(), &c);
    }

    fn search(&mut self) {
        let (q, text) = (self.rp.query(), self.query.trim().to_string());
        self.status = tr("Buscando…").into();
        self.found = Default::default();
        self.go_new(View::Results);
        self.spawn(async move {
            let (t, a, p, ar) = tokio::join!(
                q.music_search_tracks(&text),
                q.music_search_albums(&text),
                q.music_search_playlists(&text, false),
                q.music_search_artists(&text),
            );
            // The songs are what matters: only their failure is an error; the rest shows what came.
            Ok(Msg::Search(Found {
                tracks: t?.items.items,
                albums: a.map(|r| r.items.items).unwrap_or_default(),
                playlists: p.map(|r| r.items.items).unwrap_or_default(),
                artists: ar.map(|r| r.items.items).unwrap_or_default(),
            }))
        });
    }

    fn open_list(&mut self, id: String, name: String, album: bool) {
        // YouTube Music's own "Liked Music" is the same list as our liked songs.
        if id == LIKED_ID {
            return self.show_liked();
        }
        let q = self.q();
        self.status = tf!("Carregando {}…", name);
        // Auto playlists (mixes, "New Episodes": ids starting with RD) only open as radios.
        let mix_cover = id.starts_with("RD").then(|| self.lib.0.iter().find(|p| p.id == id).map(|p| p.thumbnail.clone()).unwrap_or_default());
        self.spawn(async move {
            let result: Result<List, Error> = async {
                if album {
                    let mut a = q.music_album(&id).await?;
                    // Album tracks come without covers.
                    for t in a.tracks.iter_mut().filter(|t| t.cover.is_empty()) {
                        t.cover = a.cover.clone();
                    }
                    return Ok(List { id, kind: ListKind::Album, title: a.name, cover: a.cover, tracks: a.tracks, radio_id: a.playlist_id, set_ids: vec![] });
                }
                if let Some(cover) = mix_cover {
                    let tracks = q.music_radio(&id).await?.items;
                    return Ok(List { id, kind: ListKind::Playlist, title: name.clone(), cover, tracks, radio_id: None, set_ids: vec![] });
                }
                let mut pl = q.music_playlist(&id).await?;
                // ponytail: caps huge playlists at 2000 tracks, page lazily on scroll if that bites
                pl.tracks.extend_limit(&q, 2000).await?;
                Ok(List { radio_id: Some(id.clone()), id, kind: ListKind::Playlist, title: pl.name, cover: pl.thumbnail, tracks: pl.tracks.items, set_ids: vec![] })
            }
            .await;
            // Say what failed; the raw YouTube error alone means nothing to the user.
            Ok(result.map(Msg::List).unwrap_or_else(|e| Msg::Error(tf!("Não consegui abrir \"{}\" ({})", name, e))))
        });
    }

    fn open_artist(&mut self, id: String) {
        let q = self.q();
        self.status = tr("Carregando artista…").into();
        self.spawn(async move { Ok(Msg::Artist(q.music_artist(&id, false).await?)) });
    }

    fn show_liked(&mut self) {
        let cover = self.liked_tracks.first().map(|t| t.cover.clone()).unwrap_or_default();
        self.list = Rc::new(List {
            id: LIKED_ID.into(),
            kind: ListKind::Liked,
            title: tr("Músicas curtidas").into(),
            cover,
            tracks: self.liked_tracks.clone(),
            radio_id: None,
            set_ids: vec![],
        });
        self.go_new(View::List);
    }

    fn load_lyrics(&mut self) {
        let Some(t) = self.current().cloned() else { return };
        if self.lyrics.as_ref().is_some_and(|(id, _)| *id == t.id) {
            return;
        }
        self.lyrics = Some((t.id.clone(), None));
        // Lyrics are public: no login needed (an expired one would only make this fail).
        let q = self.rp.query();
        let words = self.s.lyrics_words;
        self.spawn(async move { Ok(Msg::Lyrics(t.id.clone(), lyrics::fetch(&q, &t, words).await)) });
    }

    /// The lyrics nudge for the playing track (0 for any other).
    fn lyrics_nudge(&self) -> f64 {
        self.current().filter(|t| t.id == self.lyrics_offset.0).map_or(0.0, |_| self.lyrics_offset.1)
    }

    /// The synced line being sung now, if the playing track's lyrics are loaded and timed.
    fn lyric_now(&self) -> Option<&str> {
        let (id, Some(ly)) = self.lyrics.as_ref()? else { return None };
        self.current().filter(|t| t.id == *id)?;
        let i = ly.line_at(self.pl.time + 0.15 + self.lyrics_nudge())?;
        Some(ly.synced[i].1.as_str()).filter(|l| !l.trim().is_empty())
    }

    // ---- queue (mirrors mpv's playlist index for index) ----

    fn current(&self) -> Option<&TrackItem> {
        usize::try_from(self.pl.pos).ok().and_then(|i| self.queue.get(i))
    }

    fn play(&mut self, mut tracks: Vec<TrackItem>, mut start: usize) {
        if tracks.is_empty() {
            return;
        }
        self.unshuffled = None;
        if self.s.shuffle && tracks.len() > 1 {
            let first = tracks.remove(start.min(tracks.len() - 1));
            self.unshuffled = Some(tracks[start.min(tracks.len())..].to_vec());
            tracks = std::iter::once(first).chain(shuffled(tracks)).collect();
            start = 0;
        }
        self.mpv.command("stop", &[]).ok();
        self.queue.clear();
        self.radio_on = false;
        self.enqueue(tracks);
        self.next_end = start as i64 + 1;
        self.mpv.command("playlist-play-index", &[&start.to_string()]).ok();
        self.mpv.set_property("pause", false).ok();
    }

    fn enqueue(&mut self, tracks: Vec<TrackItem>) {
        for t in &tracks {
            self.mpv.command("loadfile", &[&yt_url(t), "append"]).ok();
        }
        self.queue.extend(tracks);
    }

    /// Inserts after the current track and after earlier "play next" picks.
    fn play_next(&mut self, t: TrackItem) {
        if self.pl.pos < 0 {
            return self.play(vec![t], 0);
        }
        let at = self.next_end.max(self.pl.pos + 1).min(self.queue.len() as i64);
        self.mpv.command("loadfile", &[&yt_url(&t), "insert-at", &at.to_string()]).ok();
        self.queue.insert(at as usize, t);
        self.next_end = at + 1;
    }

    fn unqueue(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        self.mpv.command("playlist-remove", &[&i.to_string()]).ok();
        self.queue.remove(i);
        if (i as i64) < self.next_end {
            self.next_end -= 1;
        }
    }

    /// Replaces everything after the current track.
    fn set_upcoming(&mut self, tracks: Vec<TrackItem>) {
        let keep = (self.pl.pos + 1).max(0) as usize;
        for _ in keep..self.queue.len() {
            self.mpv.command("playlist-remove", &[&keep.to_string()]).ok();
        }
        self.queue.truncate(keep);
        self.enqueue(tracks);
        self.next_end = keep as i64;
    }

    fn toggle_shuffle(&mut self) {
        self.s.shuffle = !self.s.shuffle;
        let keep = (self.pl.pos + 1).max(0) as usize;
        if self.pl.pos < 0 || keep >= self.queue.len() {
            return;
        }
        let upcoming = self.queue[keep..].to_vec();
        if self.s.shuffle {
            self.unshuffled = Some(upcoming.clone());
            self.set_upcoming(shuffled(upcoming));
        } else if let Some(orig) = self.unshuffled.take() {
            let still: HashSet<&str> = upcoming.iter().map(|t| t.id.as_str()).collect();
            let restored = orig.into_iter().filter(|t| still.contains(t.id.as_str())).collect();
            self.set_upcoming(restored);
        }
    }

    fn radio(&mut self, t: TrackItem) {
        let id = t.id.clone();
        self.play(vec![t], 0);
        self.radio_on = true;
        self.fetch_radio(id);
    }

    fn fetch_radio(&mut self, seed: String) {
        self.radio_busy = true;
        let q = self.q();
        self.spawn(async move { Ok(Msg::Radio(q.music_radio_track(&seed).await?.items)) });
    }

    fn restore_session(&mut self) {
        let Some(sess) = settings::read_json::<Session>(&Session::path()) else { return };
        if sess.queue.is_empty() {
            return;
        }
        let pos = sess.pos.clamp(0, sess.queue.len() as i64 - 1) as usize;
        self.mpv.set_property("pause", true).ok();
        for (i, t) in sess.queue.iter().enumerate() {
            if i == pos && sess.time > 1.0 {
                self.mpv.command("loadfile", &[&yt_url(t), "append", "-1", &format!("start={:.0}", sess.time)]).ok();
            } else {
                self.mpv.command("loadfile", &[&yt_url(t), "append"]).ok();
            }
        }
        self.queue = sess.queue;
        self.next_end = pos as i64 + 1;
        self.mpv.command("playlist-play-index", &[&pos.to_string()]).ok();
        self.pl.pos = pos as i64;
    }

    fn save_session(&self) {
        let sess = Session { queue: self.queue.clone(), pos: self.pl.pos, time: self.pl.time };
        write_json(&Session::path(), &sess);
    }

    // ---- library writes ----

    fn toggle_like(&mut self, t: TrackItem) {
        let like = !self.liked.contains(&t.id);
        // Optimistic: flip locally now, the server call follows.
        if like {
            self.liked.insert(t.id.clone());
            self.liked_tracks.insert(0, t.clone());
        } else {
            self.liked.remove(&t.id);
            self.liked_tracks.retain(|x| x.id != t.id);
        }
        if self.list.kind == ListKind::Liked {
            Rc::make_mut(&mut self.list).tracks = self.liked_tracks.clone();
        }
        let q = self.q();
        self.spawn(async move {
            let endpoint = if like { "like/like" } else { "like/removelike" };
            yt_write(&q, endpoint, json!({ "target": { "videoId": t.id } })).await?;
            Ok(Msg::Edited(if like { tr("Adicionada às curtidas").into() } else { tr("Removida das curtidas").into() }))
        });
    }

    fn add_to(&mut self, playlist: String, t: TrackItem) {
        if self.list.id == playlist {
            let l = Rc::make_mut(&mut self.list);
            l.tracks.push(t.clone());
            l.set_ids.clear();
        }
        let q = self.q();
        self.spawn(async move {
            let actions = json!([{ "action": "ACTION_ADD_VIDEO", "addedVideoId": t.id }]);
            yt_write(&q, "browse/edit_playlist", json!({ "playlistId": playlist, "actions": actions })).await?;
            Ok(Msg::Edited(tf!("\"{}\" adicionada à playlist", t.name)))
        });
    }

    fn remove_from_list(&mut self, i: usize, t: TrackItem) {
        let playlist = self.list.id.clone();
        let l = Rc::make_mut(&mut self.list);
        // With the row id only that row goes; without it YouTube removes every copy of the track.
        let action = if l.set_ids.len() == l.tracks.len() && i < l.tracks.len() {
            l.tracks.remove(i);
            json!({ "action": "ACTION_REMOVE_VIDEO", "setVideoId": l.set_ids.remove(i), "removedVideoId": t.id })
        } else {
            l.tracks.retain(|x| x.id != t.id);
            l.set_ids.clear();
            json!({ "action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID", "removedVideoId": t.id })
        };
        let q = self.q();
        self.spawn(async move {
            yt_write(&q, "browse/edit_playlist", json!({ "playlistId": playlist, "actions": [action] })).await?;
            Ok(Msg::Edited(tf!("\"{}\" removida da playlist", t.name)))
        });
    }

    /// Whether the open playlist can be reordered right now.
    fn can_reorder(&self) -> bool {
        let l = &self.list;
        l.kind == ListKind::Playlist && !l.tracks.is_empty() && l.set_ids.len() == l.tracks.len() && self.owns(&l.id)
    }

    fn move_in_list(&mut self, from: usize, to: usize) {
        if !self.can_reorder() || from == to || to >= self.list.tracks.len() {
            return;
        }
        let playlist = self.list.id.clone();
        let l = Rc::make_mut(&mut self.list);
        let t = l.tracks.remove(from);
        l.tracks.insert(to, t);
        let sid = l.set_ids.remove(from);
        l.set_ids.insert(to, sid.clone());
        // YouTube's move is "put this row before that one"; no successor = to the end.
        let mut action = json!({ "action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": sid });
        if let Some(next) = l.set_ids.get(to + 1) {
            action["movedSetVideoIdSuccessor"] = json!(next);
        }
        let q = self.q();
        self.spawn(async move {
            yt_write(&q, "browse/edit_playlist", json!({ "playlistId": playlist, "actions": [action] })).await?;
            Ok(Msg::Quiet)
        });
    }

    fn move_in_queue(&mut self, from: usize, to: usize) {
        if from == to || from >= self.queue.len() || to >= self.queue.len() {
            return;
        }
        // mpv's playlist-move puts the entry where `target` is now, so moving down targets one further.
        let target = if to > from { to + 1 } else { to };
        self.mpv.command("playlist-move", &[&from.to_string(), &target.to_string()]).ok();
        let t = self.queue.remove(from);
        self.queue.insert(to, t);
    }

    /// Reads the open playlist's row ids (they're in the raw page, rustypipe drops them).
    fn fetch_set_ids(&self, id: String) {
        let q = self.q();
        self.spawn(async move {
            let mut pairs = vec![];
            let mut token = None;
            let first = q.raw(ClientType::DesktopMusic, "browse", &json!({ "browseId": format!("VL{id}") })).await?;
            collect_set_ids(&serde_json::from_str(&first).unwrap_or_default(), &mut pairs, &mut token);
            // ponytail: 40 pages (~4000 rows) cap, same order of magnitude as the track list itself
            for _ in 0..40 {
                let Some(t) = token.take() else { break };
                let before = pairs.len();
                let page = q.raw(ClientType::DesktopMusic, "browse", &json!({ "continuation": t })).await?;
                collect_set_ids(&serde_json::from_str(&page).unwrap_or_default(), &mut pairs, &mut token);
                if pairs.len() == before {
                    break;
                }
            }
            Ok(Msg::SetIds(id, pairs))
        });
    }

    fn owns(&self, playlist_id: &str) -> bool {
        self.lib.0.iter().any(|p| p.id == playlist_id)
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::Play(t, i) => self.play(t, i),
            Act::PlayNext(t) => self.play_next(t),
            Act::Radio(t) => self.radio(t),
            Act::RadioOf(id) => {
                let q = self.q();
                self.status = tr("Montando rádio…").into();
                self.spawn(async move { Ok(Msg::RadioStart(q.music_radio(&id).await?.items)) });
            }
            Act::Enqueue(t) => self.enqueue(vec![t]),
            Act::Jump(i) => {
                self.mpv.command("playlist-play-index", &[&i.to_string()]).ok();
                self.mpv.set_property("pause", false).ok();
            }
            Act::Unqueue(i) => self.unqueue(i),
            Act::ClearQueue => self.set_upcoming(vec![]),
            Act::Playlist(id, name) => self.open_list(id, name, false),
            Act::Album(id, name) => self.open_list(id, name, true),
            Act::Artist(id) => self.open_artist(id),
            Act::Liked => self.show_liked(),
            Act::Go(v) => self.go(v),
            Act::Back => {
                if let Some(s) = self.back.pop() {
                    self.fwd.push(self.snap());
                    self.restore(s);
                }
            }
            Act::Forward => {
                if let Some(s) = self.fwd.pop() {
                    self.back.push(self.snap());
                    self.restore(s);
                }
            }
            Act::Like(t) => self.toggle_like(t),
            Act::AddTo(p, t) => self.add_to(p, t),
            Act::Remove(i, t) => self.remove_from_list(i, t),
            Act::Move(from, to) => self.move_in_list(from, to),
            Act::QueueMove(from, to) => self.move_in_queue(from, to),
            Act::CreatePlaylist(title, ids) => {
                let q = self.q();
                self.spawn(async move {
                    let mut body = json!({ "title": title, "description": "", "privacyStatus": "PRIVATE" });
                    if !ids.is_empty() {
                        body["videoIds"] = json!(ids);
                    }
                    yt_write(&q, "playlist/create", body).await?;
                    Ok(Msg::Edited(tf!("Playlist \"{}\" criada", title)))
                });
            }
            Act::DeletePlaylist(id) => {
                let q = self.q();
                self.lib.0.retain(|p| p.id != id);
                self.go(View::Home);
                self.spawn(async move {
                    yt_write(&q, "playlist/delete", json!({ "playlistId": id })).await?;
                    Ok(Msg::Edited(tr("Playlist excluída").into()))
                });
            }
            Act::Mini(on) => self.set_mini(on),
            Act::Shuffle => self.toggle_shuffle(),
            Act::Repeat => {
                self.s.repeat = match self.s.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
            }
            Act::LoginUse(i) => {
                if let Some((id, cookies)) = self.login_scan.as_ref().and_then(|v| v.get(i)).cloned() {
                    self.status = tf!("Entrando com o {}…", login::label(&id));
                    self.login_with(cookies, Some(id), false);
                }
            }
            Act::LoginGoogle => {
                let browser = login::default_browser().or_else(|| login::detect().first().map(|(id, _)| *id));
                login::open_in(browser, login::SIGN_IN_URL);
                match browser {
                    Some(b) => self.login_wait = Some((b.to_string(), std::time::Instant::now())),
                    None => self.status = tr("Nenhum navegador compatível encontrado.").into(),
                }
            }
            Act::LoginCancel => self.login_wait = None,
            Act::Logout => {
                self.s.login_browser.clear();
                self.account = None;
                self.login_scan = None;
                std::fs::remove_file(settings::cookie_file()).ok();
                std::fs::remove_file(settings::ytdlp_cookie_file()).ok();
                let rp = self.rp.clone();
                self.rt.spawn(async move { rp.user_auth_remove_cookie().await.ok() });
                self.logged_in = false;
                self.lib = Default::default();
                self.liked.clear();
                self.liked_tracks.clear();
                std::fs::remove_file(LibraryCache::path()).ok();
                self.home.clear();
                self.load_home();
                audio::apply(self.mpv, &self.s);
                self.status = tr("Você saiu da conta").into();
            }
            Act::ClearCache => {
                std::fs::remove_dir_all(settings::cache_dir()).ok();
                // The image loader keeps writing into this folder.
                std::fs::create_dir_all(settings::cache_dir().join("art")).ok();
                self.ctx.forget_all_images();
                self.art_colors.clear();
                self.status = tr("Cache limpo").into();
            }
            Act::ClearHistory => {
                self.history.clear();
                std::fs::remove_file(settings::history_path()).ok();
            }
            Act::FindVideo => {
                if let Some(t) = self.current().cloned() {
                    self.status = tr("Procurando o videoclipe…").into();
                    let q = self.rp.query();
                    let text = format!("{} {}", ui::artists(&t), t.name);
                    self.spawn(async move {
                        let found = q.music_search_videos(&text).await?.items.items.into_iter().next();
                        Ok(Msg::VideoFound(found))
                    });
                }
            }
            Act::DownloadSkin(i) => {
                self.status = tr("Baixando a skin…").into();
                let (tx, waker) = (self.tx.clone(), self.waker.clone());
                std::thread::spawn(move || {
                    tx.send(Msg::SkinReady(winamp::download(i))).ok();
                    waker.wake();
                });
            }
            Act::Quit => {
                self.quitting = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn mini_size(&self) -> egui::Vec2 {
        mini_size(&self.s)
    }

    /// Pins the window to the mini size: no resizing, no maximize button.
    fn lock_mini_size(&self, size: egui::Vec2) {
        use egui::ViewportCommand as V;
        self.ctx.send_viewport_cmd(V::Resizable(false));
        self.ctx.send_viewport_cmd(V::EnableButtons { close: true, minimized: true, maximize: false });
        self.ctx.send_viewport_cmd(V::MinInnerSize(size));
        self.ctx.send_viewport_cmd(V::MaxInnerSize(size));
        self.ctx.send_viewport_cmd(V::InnerSize(size));
    }

    /// Video view on/off. The song never restarts: its video stream is fetched on the side and added
    /// to the playing file as an extra track, which mpv keeps in sync. Off: the picture is dropped.
    fn set_video(&mut self, on: bool) {
        if on == self.video_on {
            if on {
                self.ensure_video();
            }
            return;
        }
        self.video_on = on;
        if on {
            self.ensure_video();
        } else {
            // Removed, not just hidden: a deselected track could keep its stream downloading.
            if let Ok(id) = self.mpv.get_property::<i64>("current-tracks/video/id") {
                self.mpv.command("video-remove", &[&id.to_string()]).ok();
            }
            self.mpv.set_property("vid", "no").ok();
            self.video_for = None;
            self.vis = None;
            trim_heap();
        }
    }

    /// Asks for the current track's video once (again after a track change).
    fn ensure_video(&mut self) {
        let Some(t) = self.current().cloned() else { return };
        if self.video_for.as_deref() == Some(t.id.as_str()) {
            return;
        }
        self.video_for = Some(t.id.clone());
        let (s, url, id) = (self.s.clone(), yt_url(&t), t.id.clone());
        self.spawn(async move {
            let found = tokio::task::spawn_blocking(move || audio::video_url(&s, &url)).await.ok().flatten();
            Ok(Msg::VideoUrl(id, found))
        });
    }

    /// Puts `t` where the current queue entry is and continues from the same position
    /// (a per-file `start`, so nothing else is affected).
    fn swap_current(&mut self, t: TrackItem) {
        let Ok(pos) = usize::try_from(self.pl.pos) else { return };
        let at = (pos + 1).to_string();
        let start = format!("start={:.1}", self.pl.time.max(0.0));
        self.mpv.command("loadfile", &[&yt_url(&t), "insert-at", &at, &start]).ok();
        self.queue.insert(pos + 1, t);
        self.mpv.command("playlist-play-index", &[&at]).ok();
        self.mpv.command("playlist-remove", &[&pos.to_string()]).ok();
        self.queue.remove(pos);
    }

    fn set_mini(&mut self, on: bool) {
        use egui::{ViewportCommand as V, WindowLevel};
        if on && !self.mini && self.s.random_skin {
            let others: Vec<String> = winamp::list_skins().into_iter().filter(|k| *k != self.s.winamp_skin).collect();
            if !others.is_empty() {
                let i = std::collections::hash_map::RandomState::new().hash_one(now_secs()) as usize % others.len();
                self.s.winamp_skin = others[i].clone();
                self.skin = None;
            }
        }
        if on == self.mini {
            // Already mini: a new skin, scale or section only changes its size.
            if on {
                self.lock_mini_size(self.mini_size());
                let level = if self.s.mini_on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal };
                self.ctx.send_viewport_cmd(V::WindowLevel(level));
            }
            return;
        }
        if on {
            self.was_maximized = self.ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if !self.was_maximized
                && let Some(r) = self.ctx.input(|i| i.viewport().inner_rect)
            {
                self.full_size = r.size();
            }
        }
        self.mini = on;
        // Wayland gives apps no say in where a window goes, and a resized one stays where it was
        // (often half off-screen). A new window is placed by the compositor, centred: main() reopens
        // it right away in the other mode. Playback isn't affected.
        self.reopen = true;
        self.ctx.send_viewport_cmd(V::Close);
    }

    fn show_window(&mut self) {
        use egui::ViewportCommand as V;
        // Headless (closed to the tray): the tray loop reopens the window.
        self.want_show = true;
        self.ctx.send_viewport_cmd(V::Visible(true));
        self.ctx.send_viewport_cmd(V::Minimized(false));
        self.ctx.send_viewport_cmd(V::Focus);
    }

    fn handle(&mut self, msg: Msg) {
        if !matches!(msg, Msg::Media(_) | Msg::Radio(_) | Msg::Quiet) {
            self.status.clear();
        }
        match msg {
            Msg::Search(f) => {
                self.found = Rc::new(f);
                self.shown_at = self.now();
            }
            Msg::List(l) => {
                // Same page refreshed: replace in place, no history entry.
                let same = self.view == View::List && self.list.id == l.id;
                if l.kind == ListKind::Playlist && self.logged_in && self.owns(&l.id) {
                    self.fetch_set_ids(l.id.clone());
                }
                self.list = Rc::new(l);
                if same { self.shown_at = self.now() } else { self.go_new(View::List) }
            }
            Msg::Artist(a) => {
                self.artist = Some(Rc::new(a));
                self.go_new(View::Artist);
            }
            Msg::Lyrics(id, body) => {
                if self.lyrics.as_ref().is_some_and(|(cur, _)| *cur == id) {
                    self.lyrics = Some((id, Some(body)));
                }
            }
            Msg::Library(mut p, a) => {
                p.retain(shown_in_library);
                self.lib = (p, a);
                self.save_library_cache();
            }
            Msg::Liked(t) => {
                self.liked = t.iter().map(|t| t.id.clone()).collect();
                self.liked_tracks = t;
                self.save_library_cache();
            }
            Msg::Home(h, guest) => {
                self.home = h;
                if guest {
                    self.renew_session();
                }
            }
            Msg::NewAlbums(a) => {
                self.new_albums = a;
                self.save_library_cache();
            }
            Msg::Radio(t) => {
                self.radio_busy = false;
                let seen: HashSet<&str> = self.queue.iter().map(|t| t.id.as_str()).collect();
                let fresh: Vec<_> = t.into_iter().filter(|t| !seen.contains(t.id.as_str())).collect();
                self.enqueue(fresh);
            }
            Msg::RadioStart(t) => {
                self.play(t, 0);
                self.radio_on = true;
            }
            Msg::SetIds(id, pairs) => {
                if self.list.id == id {
                    let ids: Vec<&str> = self.list.tracks.iter().map(|t| t.id.as_str()).collect();
                    let aligned = align_set_ids(&ids, &pairs).unwrap_or_default();
                    Rc::make_mut(&mut self.list).set_ids = aligned;
                }
            }
            Msg::Edited(note) => {
                self.status = note;
                // Rows changed server side: re-read their ids so the next move targets the right row.
                if self.list.kind == ListKind::Playlist && self.owns(&self.list.id) {
                    self.fetch_set_ids(self.list.id.clone());
                }
                let q = self.q();
                self.spawn(async move {
                    let (p, a) = tokio::join!(q.music_saved_playlists(), q.music_saved_albums());
                    Ok(Msg::Library(p?.items, a?.items))
                });
            }
            Msg::Quiet => {}
            Msg::Heat(id, h) => {
                if self.heat.as_ref().is_some_and(|(cur, _)| *cur == id) {
                    self.heat = Some((id, h));
                }
            }
            // Startup refresh done: what loaded with the old cookies loads again with the new ones.
            Msg::Refreshed => {
                audio::apply(self.mpv, &self.s);
                self.fetch_account();
                self.load_library();
                self.load_home();
            }
            Msg::LoginScan(found) => {
                self.scanning = false;
                self.login_scan = Some(found);
            }
            Msg::LoginPoll(found) => {
                self.login_polling = false;
                // No session yet: keep waiting (poll_login tries again in a few seconds).
                if let Some(cookies) = found
                    && let Some((browser, _)) = self.login_wait.take()
                {
                    self.status = tf!("Login detectado no {}. Entrando…", login::label(&browser));
                    self.login_with(cookies, Some(browser), false);
                    self.show_window();
                }
            }
            Msg::Account(a) => {
                // Signed in, yet YouTube knows no account: the session was dropped.
                if a.is_none() {
                    self.renew_session();
                }
                self.account = a;
            }
            Msg::YtDlp(v) => {
                if ytdlp_days_old(&v, now_secs()).is_some_and(|d| d > 60) {
                    self.status = tf!("O yt-dlp {} está desatualizado; atualize se a reprodução falhar (YouTube muda com frequência).", v);
                }
                self.ytdlp_version = v;
            }
            Msg::LoggedIn(browser) => {
                self.s.login_browser = browser.unwrap_or_default();
                self.logged_in = true;
                self.login_wait = None;
                self.fetch_account();
                audio::apply(self.mpv, &self.s);
                self.go(View::Home);
                self.load_library();
                self.load_home();
            }
            Msg::Error(e) => {
                self.radio_busy = false;
                self.status = tf!("Erro: {}", e);
            }
            Msg::Media(e) => self.media(e),
            Msg::Ipc(line, reply) => {
                let answer = self.ipc(&line);
                reply.send(answer).ok();
            }
            Msg::PlayRadio(t) => self.radio(t),
            Msg::PlayNow(t) => self.play(t, 0),
            Msg::VideoUrl(id, url) => {
                if self.video_on && self.current().is_some_and(|t| t.id == id) {
                    match url {
                        Some(u) => {
                            self.mpv.command("video-add", &[&u, "select"]).ok();
                            // A track added mid-song only gets its video output after a refresh: an
                            // in-place seek (the audio is already cached, so no audible gap).
                            self.mpv.command("seek", &["0", "relative+exact"]).ok();
                        }
                        None => self.status = tr("Não consegui carregar o vídeo desta faixa.").into(),
                    }
                }
            }
            Msg::VideoFound(found) => match found {
                Some(v) => {
                    self.status = tf!("Videoclipe: {}", v.name);
                    // A different recording: it replaces the track, then its picture is added.
                    self.swap_current(v);
                    self.video_on = true;
                    self.video_for = None;
                }
                None => self.status = tr("Não encontrei um videoclipe para esta faixa.").into(),
            },
            Msg::SkinReady(r) => match r {
                Ok(name) => {
                    self.s.winamp_skin = name;
                    self.status = tr("Skin instalada. Abra o mini player (Ctrl+M) para usar.").into();
                }
                Err(e) => self.status = tf!("Erro baixando a skin: {}", e),
            },
            Msg::Tray(c) => match c {
                tray::TrayCmd::Show => self.show_window(),
                tray::TrayCmd::Toggle => self.media(MediaControlEvent::Toggle),
                tray::TrayCmd::Next => self.media(MediaControlEvent::Next),
                tray::TrayCmd::Prev => self.media(MediaControlEvent::Previous),
                tray::TrayCmd::Quit => self.apply(Act::Quit),
                tray::TrayCmd::Volume(n) => drop(self.mpv.command("add", &["volume", &(n * 5).to_string()])),
            },
        }
    }

    fn media(&mut self, e: MediaControlEvent) {
        if playback(self.mpv, &e) {
            return;
        }
        match e {
            MediaControlEvent::Raise => self.show_window(),
            MediaControlEvent::Quit => self.apply(Act::Quit),
            _ => {}
        }
    }

    fn crossfade(&mut self) {
        let on = self.s.crossfade_secs > 0 && !self.video_on && self.s.repeat != Repeat::One;
        self.xfade = None;
        if !on {
            if let Some(mut f) = self.fader.take() {
                f.cancel(self.mpv);
            }
            return;
        }
        if self.fader.is_none() {
            self.fader = crossfade::Fader::new();
        }
        let Some(f) = self.fader.as_mut() else { return };
        let pos = usize::try_from(self.pl.pos).ok();
        let next = pos.and_then(|p| self.queue.get(p + 1)).map(|t| (t.id.as_str(), yt_url(t)));
        let now = crossfade::Now {
            pos: self.pl.pos,
            paused: self.pl.paused,
            time: self.pl.time,
            dur: self.pl.dur,
            current: pos.and_then(|p| self.queue.get(p)).map(|t| t.id.as_str()),
            next,
        };
        f.tick(self.mpv, &self.s, now);
        // Past the middle of a fade the incoming track is what's heard: the interface, MPRIS and the
        // tray follow it (its time from the helper), so nothing jumps when the main player catches up.
        self.xfade = f.display().map(|d| (d.k, d.from));
        if let Some(d) = f.display().filter(|d| d.k >= 0.5 && d.dur > 0.0) {
            self.pl.pos = d.from + 1;
            self.pl.time = d.time;
            self.pl.dur = d.dur;
        }
    }

    fn sync_os(&mut self) {
        let (pos, paused, time) = (self.pl.pos, self.pl.paused, self.pl.time);
        let id = self.current().map(|t| t.id.clone()).unwrap_or_default();
        if self.last.0 == pos && self.last.1 == paused && self.last.2 == id {
            return;
        }
        let track_changed = self.last.0 != pos || self.last.2 != id;
        self.last = (pos, paused, id);
        // Tray hover: title and artist.
        if let Some(tray) = &self.tray {
            let (title, mut desc) = match self.current() {
                Some(t) => {
                    (t.name.clone(), tray::escape(&ui::artists(t)))
                }
                None => ("YtmFast".into(), tr("Nada tocando").into()),
            };
            if paused {
                desc.push_str(tr(" · pausado"));
            }
            tray.now_playing(&self.rt, title, desc);
        }
        if track_changed {
            // Each song frees a lot (mpv's demuxer and decoder for the last one): give it back.
            trim_heap();
            let title = self.current().map_or("YtmFast".into(), |t| format!("{} • {}", t.name, ui::artists(t)));
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            if pos >= self.next_end {
                self.next_end = pos + 1;
            }
            self.save_session();
            if self.view == View::Lyrics || self.lyrics_panel {
                self.load_lyrics();
            }
        }
        if let Some(t) = self.current().filter(|t| !paused && t.id != self.announced).cloned() {
            self.announced = t.id.clone();
            // Only when the window isn't in front: there the song is on screen already.
            if self.s.notify_track && !self.ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
                notify::track(&self.rt, t.name.clone(), ui::artists(&t));
            }
            if !self.s.listenbrainz_token.trim().is_empty() {
                scrobble::now_playing(&self.s.listenbrainz_token, &t);
            }
        }
        let track = usize::try_from(pos).ok().and_then(|i| self.queue.get(i));
        let Some(c) = self.controls.as_mut() else { return };
        if let Some(t) = track.filter(|_| track_changed) {
            let a = ui::artists(t);
            c.set_metadata(MediaMetadata {
                title: Some(&t.name),
                artist: Some(&a),
                album: t.album.as_ref().map(|a| a.name.as_str()),
                cover_url: t.cover.last().map(|c| c.url.as_str()),
                duration: t.duration.map(|d| Duration::from_secs(d.into())),
            })
            .ok();
        }
        let progress = Some(MediaPosition(Duration::from_secs_f64(time.max(0.0))));
        c.set_playback(match (track, paused) {
            (None, _) => MediaPlayback::Stopped,
            (_, true) => MediaPlayback::Paused { progress },
            _ => MediaPlayback::Playing { progress },
        })
        .ok();
    }

    /// Counts a play after 30 s (or half of a shorter track), like Spotifast: into the history and,
    /// with a token, to ListenBrainz.
    fn record_history(&mut self) {
        let p = &self.pl;
        let scrobble = !self.s.listenbrainz_token.trim().is_empty();
        if !(self.s.history || scrobble) || p.pos < 0 || self.recorded == p.pos || p.dur <= 0.0 || p.time < (p.dur / 2.0).min(30.0) {
            return;
        }
        self.recorded = p.pos;
        let started = now_secs().saturating_sub(p.time as u64);
        if let Some(t) = self.current().cloned() {
            if scrobble {
                scrobble::listen(&self.s.listenbrainz_token, &t, started);
            }
            if !self.s.history {
                return;
            }
            self.history.insert(0, Played { track: t, at: now_secs() });
            self.history.truncate(settings::HISTORY_MAX);
            write_json(&settings::history_path(), &self.history);
        }
    }

    /// Applies whatever changed in settings since last frame, then persists them.
    fn settings_changed(&mut self) {
        if self.unsaved_since.is_some_and(|t| t.elapsed() > Duration::from_secs(1)) {
            self.unsaved_since = None;
            self.s.save();
        }
        if self.s == self.applied {
            return;
        }
        if self.s.lyrics_words != self.applied.lyrics_words {
            // Fetched again in the new style next time they're shown.
            self.lyrics = None;
            if self.view == View::Lyrics || self.lyrics_panel {
                self.load_lyrics();
            }
        }
        if self.s.language != self.applied.language {
            i18n::apply(&self.s.language);
            self.load_home();
        }
        if self.s.autostart_tray != self.applied.autostart_tray {
            settings::set_autostart(self.s.autostart_tray);
        }

        let (a, b) = (&self.applied, &self.s);
        let eq_moved = a.eq_preamp_db != b.eq_preamp_db || a.eq_bands_db != b.eq_bands_db;
        let audio_changed = a.quality != b.quality
            || a.normalize != b.normalize
            || a.gapless != b.gapless
            || a.audio_device != b.audio_device
            || a.eq_on != b.eq_on
            || a.eq_loudness != b.eq_loudness
            || a.balance != b.balance
            || a.mono != b.mono
            || a.buffer_mb != b.buffer_mb
            || a.repeat != b.repeat
            || a.speed != b.speed
            || a.proxy != b.proxy;
        if audio_changed {
            audio::apply(self.mpv, &self.s);
        } else if eq_moved && self.s.eq_on {
            // A band being dragged: retuned in place, no rebuilt filter chain (no dropout). A flat
            // curve, or no filter in place yet, takes the full path.
            let vol = self.pl.vol;
            if audio::eq_curve(&self.s, vol).is_none() || !audio::update_eq(self.mpv, &self.s, vol, &mut self.eq_sent) {
                audio::apply(self.mpv, &self.s);
            }
        }
        // A new skin or scale applies at once if the mini player is open.
        let skin_changed = a.winamp_skin != b.winamp_skin || a.skin_scale != b.skin_scale || a.deck_playlist != b.deck_playlist || a.deck_eq != b.deck_eq;
        if a.zoom != b.zoom {
            self.ctx.set_zoom_factor(b.zoom);
        }
        if a.theme != b.theme || a.tilt != b.tilt || a.effects_3d != b.effects_3d || a.reduce_motion != b.reduce_motion {
            self.applied_theme = None;
            self.reveal_theme |= a.theme != b.theme;
        }
        self.unsaved_since.get_or_insert_with(std::time::Instant::now);
        self.applied = self.s.clone();
        if skin_changed && self.mini {
            self.skin = None;
            self.set_mini(true);
        }
    }

    fn sync_theme(&mut self) {
        let system_dark = self.ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark);
        let key = (self.s.theme.clone(), system_dark);
        if self.applied_theme.as_ref() == Some(&key) {
            return;
        }
        // A picked theme, or the system flipping light/dark; not in the mini player (the skin draws it).
        let flipped = self.applied_theme.as_ref().is_some_and(|k| k.0 == key.0);
        if (std::mem::take(&mut self.reveal_theme) || flipped) && !self.mini && !self.s.reduce_motion {
            self.transition.begin(&self.ctx);
            if self.transition.holding(&self.ctx) {
                self.reveal_theme = true;
                return;
            }
        }
        let look = theme::Look {
            pal: theme::resolve(&self.s.theme, system_dark),
            tilt: if self.s.effects_3d { self.s.tilt } else { 0.0 },
            motion: !self.s.reduce_motion,
        };
        theme::apply(&self.ctx, look);
        self.applied_theme = Some(key);
    }

    /// Dominant colour of a cover, computed once its bytes are loaded.
    fn accent_for(&mut self, cover: &[Thumbnail], size: f32) -> Option<Color32> {
        if !self.s.accent_from_art {
            return None;
        }
        let url = ui::cover_url(&self.ctx, cover, size)?;
        if let Some(c) = self.art_colors.get(&url) {
            return *c;
        }
        match self.ctx.try_load_bytes(&url) {
            Ok(egui::load::BytesPoll::Ready { bytes, .. }) => {
                let c = ui::art_color(&bytes);
                self.art_colors.insert(url, c);
                c
            }
            Ok(_) => None,
            Err(_) => {
                self.art_colors.insert(url, None);
                None
            }
        }
    }
}

impl App {
    /// Runs before each frame: playback state, OS controls, messages.
    fn logic(&mut self, ctx: &egui::Context) {
        self.tick();
        self.sync_theme();
        // Every 10 s: drop artwork unseen for 2 min, and keep decoded artwork under 24 MiB.
        let now = ctx.input(|i| i.time);
        // A few seconds into a new window the GL driver is done compiling shaders and setting up;
        // what it freed is still held by the heap until handed back.
        if !self.window_trimmed && now > 4.0 {
            self.window_trimmed = true;
            trim_heap();
        }
        // Fresh recommendations, and a dropped session noticed, every 30 minutes with a window.
        if now - self.home_loaded > 1800.0 {
            self.load_home();
        }
        if now - self.images_trimmed > 10.0 {
            self.images_trimmed = now;
            ui::forget_stale_images(ctx, 24 << 20);
        }
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if self.mini && maximized {
            // Maximized anyway (a WM shortcut): the mini window has one size.
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        if !self.mini
            && !ctx.input(|i| i.viewport().maximized.unwrap_or(false))
            && let Some(r) = ctx.input(|i| i.viewport().inner_rect)
        {
            self.full_size = r.size();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && !self.reopen {
            if self.s.close_to_tray && self.has_tray {
                // Linux: the window really closes (Wayland can't hide one) and main() keeps the
                // player running headless. Windows and macOS: the tray icon lives on the window's
                // event loop, so the window stays, hidden, with its memory handed back.
                #[cfg(target_os = "linux")]
                {
                    self.to_tray = true;
                }
                #[cfg(not(target_os = "linux"))]
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.set_video(false);
                    self.vis = None;
                    ctx.forget_all_images();
                    ui::clear_seen();
                    release_memory();
                }
            } else {
                self.quitting = true;
            }
        }
        // Idle = zero CPU; only tick while something is playing.
        if self.pl.pos >= 0 && !self.pl.paused {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// The window is going away (quit or into the tray): free what belongs to it, save state.
    fn window_closed(&mut self) {
        // GL-backed renderers go while the context is still current; textures die with the window.
        self.vis = None;
        // The renderer belongs to this window; mpv goes back to audio only.
        self.set_video(false);
        #[cfg(target_os = "linux")]
        {
            self.pw = None;
        }
        self.skin = None;
        self.transition = Default::default();
        // Decoded artwork, fonts and the image byte cache live in the window's context: let them go
        // with it instead of carrying them into the tray. The next window gets a fresh context.
        self.ctx.forget_all_images();
        self.ctx = egui::Context::default();
        ui::clear_seen();
        trim_heap();
        self.s.volume = self.pl.vol;
        self.s.save();
        self.unsaved_since = None;
        self.save_session();
    }
}

/// eframe owns its app; this hands ours back through `slot` when the window closes,
/// so the same player survives into the tray and into the next window.
struct Host {
    app: Option<App>,
    slot: Rc<std::cell::RefCell<Option<App>>>,
    /// Without vsync (see main), back-to-back frames are paced here: at most ~60 per second.
    last_frame: std::time::Instant,
}

impl eframe::App for Host {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        const FRAME: Duration = Duration::from_micros(16_600);
        if let Some(wait) = FRAME.checked_sub(self.last_frame.elapsed()) {
            std::thread::sleep(wait);
        }
        self.last_frame = std::time::Instant::now();
        if let Some(app) = self.app.as_mut() {
            app.logic(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(app) = self.app.as_mut() {
            app.draw(ui, frame);
            app.transition.paint(ui.ctx());
            for act in std::mem::take(&mut app.acts) {
                app.apply(act);
            }
            app.settings_changed();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(mut app) = self.app.take() {
            app.window_closed();
            *self.slot.borrow_mut() = Some(app);
        }
    }
}

/// Gives freed heap back to the system (the allocator keeps it otherwise): after a window, the
/// video or a song goes.
fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: plain libc call, no pointers involved.
    unsafe {
        libc::malloc_trim(0);
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means every zone; goal 0 means as much as possible.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}

/// The window just hid into the tray (Windows, macOS): free heap, and on Windows let the
/// system page out what the hidden window held (its working set), as it would for any idle app.
#[cfg(not(target_os = "linux"))]
fn release_memory() {
    trim_heap();
    #[cfg(windows)]
    // SAFETY: the current-process pseudo handle needs no closing.
    unsafe {
        windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(windows_sys::Win32::System::Threading::GetCurrentProcess());
    }
}

/// In the tray most loaded code (the UI) goes unused: its pages are clean copies of files on disk,
/// so the kernel may drop them from RAM. Whatever is still needed faults straight back in. Writable
/// memory and device mappings are left alone.
// ponytail: the kernel only pages out files we own or may write (our own binary in ~/.cargo/bin),
// never system libraries like the GL driver; freeing those would take restarting without a window.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn page_out_code() {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else { return };
    for line in maps.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(range), Some(perms), Some(path)) = (f.first(), f.get(1), f.get(5)) else { continue };
        if !path.starts_with('/') || path.starts_with("/dev/") || perms.as_bytes().get(1) == Some(&b'w') {
            continue;
        }
        let Some((a, b)) = range.split_once('-') else { continue };
        let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) else { continue };
        // SAFETY: an advisory call on our own read-only file mappings; contents never change.
        unsafe {
            libc::madvise(a as *mut libc::c_void, b - a, libc::MADV_PAGEOUT);
        }
    }
}

/// Media commands that only need mpv (thread-safe), so the tray and MPRIS can run them from their
/// own threads. False = the app has to handle it (raise, quit, ...).
fn playback(m: &Mpv, e: &MediaControlEvent) -> bool {
    let sign = |d: &SeekDirection| if *d == SeekDirection::Forward { 1.0 } else { -1.0 };
    // mpv errors here only mean "nothing loaded"; nothing to report.
    let _ = match e {
        MediaControlEvent::Play => m.set_property("pause", false),
        MediaControlEvent::Pause => m.set_property("pause", true),
        MediaControlEvent::Toggle => m.command("cycle", &["pause"]),
        MediaControlEvent::Next => m.command("playlist-next", &[]),
        MediaControlEvent::Previous => m.command("playlist-prev", &[]),
        MediaControlEvent::Stop => m.command("stop", &[]),
        MediaControlEvent::Seek(d) => m.command("seek", &[&(10.0 * sign(d)).to_string()]),
        MediaControlEvent::SeekBy(d, t) => m.command("seek", &[&(t.as_secs_f64() * sign(d)).to_string()]),
        MediaControlEvent::SetPosition(MediaPosition(t)) => m.command("seek", &[&t.as_secs_f64().to_string(), "absolute"]),
        // The setting follows mpv's volume when the app next ticks (see window_closed / settings).
        MediaControlEvent::SetVolume(v) => m.set_property("volume", v * 100.0),
        _ => return false,
    };
    true
}

/// Exact size of the mini window: fixed for a Winamp skin, the compact player otherwise.
fn mini_size(s: &Settings) -> egui::Vec2 {
    if s.winamp_skin.is_empty() { vec2(deck::W, deck::height(s)) } else { vec2(winamp::W, winamp::H) * s.skin_scale.clamp(1, 4) as f32 }
}

/// Link given on the command line when no instance was running.
static STARTUP_LINK: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// glvnd loads every installed EGL vendor; on an NVIDIA-only machine Mesa (and LLVM with it) is
/// ~20 MB of memory for nothing. Hybrid machines (an Intel/AMD card too) are left alone.
#[cfg(target_os = "linux")]
fn nvidia_only_egl() -> bool {
    const VAR: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";
    const NVIDIA: &str = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";
    if std::env::var_os(VAR).is_some() || !std::path::Path::new(NVIDIA).exists() {
        return false;
    }
    let drivers: Vec<String> = std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("card") && !n.contains('-')))
        .filter_map(|e| std::fs::read_link(e.path().join("device/driver")).ok()?.file_name()?.to_str().map(String::from))
        .collect();
    if drivers.is_empty() || drivers.iter().any(|d| d != "nvidia") {
        return false;
    }
    // SAFETY: first thing in main, before any other thread exists.
    unsafe { std::env::set_var(VAR, NVIDIA) };
    true
}

fn main() -> eframe::Result {
    // Few heap arenas: one per thread (glibc's default, up to 8 per core) keeps freed memory around.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: plain libc call before any thread exists.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
        // Fixed thresholds: big blocks (images, audio buffers) get their own mappings and go back to
        // the system when freed, instead of glibc raising the threshold and fragmenting the heap.
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 10);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 20);
    }
    #[cfg(target_os = "linux")]
    let mut forced_egl = nvidia_only_egl();
    settings::migrate_old_name();
    i18n::apply(&Settings::load().language);
    let args: Vec<String> = std::env::args().skip(1).collect();
    // A GUI program has no console: `ytmfast now-playing` from a terminal writes to the caller's.
    #[cfg(windows)]
    if !args.is_empty() {
        // SAFETY: plain Win32 call; failing (no parent console) just leaves output unshown.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
    if let Some(code) = ipc::client(&args) {
        std::process::exit(code);
    }
    if let Some(link) = args.first().filter(|a| ipc::is_link(a)) {
        STARTUP_LINK.set(link.clone()).ok();
    }
    let slot: Rc<std::cell::RefCell<Option<App>>> = Rc::default();
    // Windows / macOS: `--tray` opens the window hidden (the tray lives on its event loop).
    let mut start_hidden = cfg!(not(target_os = "linux")) && args.first().is_some_and(|a| a == "--tray");
    // `--tray` (login autostart): no window, so no GL driver, until the tray icon is clicked.
    if cfg!(target_os = "linux") && args.first().is_some_and(|a| a == "--tray") {
        let mut app = App::new(None);
        if app.has_tray && !app.background() {
            app.window_closed();
            return Ok(());
        }
        *slot.borrow_mut() = Some(app);
    }
    loop {
        // The window opens straight in its mode: mini (fixed size, own title bar) or full.
        let (mini, viewport) = {
            let app = slot.borrow();
            let s = app.as_ref().map_or_else(Settings::load, |a| a.s.clone());
            let mini = app.as_ref().map_or(s.start_mini, |a| a.mini);
            let base = egui::ViewportBuilder::default()
                .with_visible(!std::mem::take(&mut start_hidden))
                .with_title("YtmFast")
                .with_app_id("ytmfast")
                .with_icon(Arc::new(egui::IconData { rgba: tray::icon_rgba(256), width: 256, height: 256 }));
            let vp = if mini {
                let size = mini_size(&s);
                let level = if s.mini_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                base.with_inner_size(size).with_min_inner_size(size).with_max_inner_size(size).with_resizable(false).with_decorations(false).with_maximize_button(false).with_window_level(level)
            } else {
                let (size, max) = app.as_ref().map_or((vec2(1200.0, 760.0), false), |a| (a.full_size, a.was_maximized));
                base.with_inner_size(size).with_min_inner_size([820.0, 520.0]).with_maximized(max)
            };
            (mini, vp)
        };
        // No vsync: on Wayland a swap waits for the compositor's frame callback, which never comes
        // while the window is hidden or minimised, and the UI thread would sit there. Frames are
        // drawn on demand anyway, so this costs nothing.
        let mut opts = eframe::NativeOptions { viewport, ..Default::default() };
        opts.glow_options.vsync = false;
        let s2 = slot.clone();
        let run = eframe::run_native(
            "ytmfast",
            opts,
            Box::new(move |cc| {
                let previous = s2.borrow_mut().take();
                let app = match previous {
                    Some(mut app) => {
                        app.attach(cc);
                        app
                    }
                    None => {
                        let mut app = App::new(Some(cc));
                        app.mini = mini;
                        app
                    }
                };
                Ok(Box::new(Host { app: Some(app), slot: s2, last_frame: std::time::Instant::now() }))
            }),
        );
        // The NVIDIA-only EGL didn't work here after all: once more with every vendor.
        #[cfg(target_os = "linux")]
        if run.is_err() && std::mem::take(&mut forced_egl) && slot.borrow().is_none() {
            // SAFETY: the failed window left no thread of ours touching the environment.
            unsafe { std::env::remove_var("__EGL_VENDOR_LIBRARY_FILENAMES") };
            continue;
        }
        run?;
        let Some(mut app) = slot.borrow_mut().take() else { break };
        if std::mem::take(&mut app.reopen) && !app.quitting {
            // Mini <-> full: same app, new window in the other mode.
            app.window_closed();
            *slot.borrow_mut() = Some(app);
            continue;
        }
        if app.quitting || !app.to_tray {
            break;
        }
        app.to_tray = false;
        if !app.background() {
            app.window_closed();
            break;
        }
        *slot.borrow_mut() = Some(app);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn set_ids_parse_and_align() {
        let page = serde_json::json!({ "contents": [
            { "musicResponsiveListItemRenderer": { "playlistItemData": { "videoId": "a", "playlistSetVideoId": "S1" } } },
            { "musicResponsiveListItemRenderer": { "playlistItemData": { "videoId": "b", "playlistSetVideoId": "S2" } } },
            { "musicResponsiveListItemRenderer": { "playlistItemData": { "videoId": "a", "playlistSetVideoId": "S3" } } },
            { "continuationItemRenderer": { "continuationEndpoint": { "continuationCommand": { "token": "T" } } } }
        ]});
        let (mut pairs, mut token) = (vec![], None);
        super::collect_set_ids(&page, &mut pairs, &mut token);
        assert_eq!(pairs.len(), 3);
        assert_eq!(token.as_deref(), Some("T"));
        // Duplicates keep their own row ids, in order.
        assert_eq!(super::align_set_ids(&["a", "b", "a"], &pairs), Some(vec!["S1".into(), "S2".into(), "S3".into()]));
        // A track the page doesn't have: refuse rather than guess.
        assert_eq!(super::align_set_ids(&["a", "z"], &pairs), None);
    }

    #[test]
    fn ytdlp_age() {
        // 2026-10-10 00:00 UTC
        let now = 1_791_590_400;
        assert_eq!(super::ytdlp_days_old("2026.08.19", now), Some(52));
        assert_eq!(super::ytdlp_days_old("2026.10.10.000123", now), Some(0));
        assert_eq!(super::ytdlp_days_old("nightly", now), None);
        assert_eq!(super::ytdlp_days_old("", now), None);
    }
}
