//! "Sign in with your browser": yt-dlp reads the browser's cookie store (decrypting it through the
//! system keyring when needed) and we keep only the YouTube ones.

use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;
use std::process::Stdio;

use crate::settings::{data_dir, make_private};
use crate::i18n::tr;

/// Our id and label. Ids in FIREFOX_FORKS are read as Firefox profiles; the rest are yt-dlp
/// browser names.
pub const BROWSERS: [(&str, &str); 12] = [
    ("zen", "Zen"),
    ("librewolf", "LibreWolf"),
    ("floorp", "Floorp"),
    ("waterfox", "Waterfox"),
    ("firefox", "Firefox"),
    ("chrome", "Chrome"),
    ("chromium", "Chromium"),
    ("brave", "Brave"),
    ("vivaldi", "Vivaldi"),
    ("edge", "Edge"),
    ("opera", "Opera"),
    ("safari", "Safari"),
];

const FIREFOX_FORKS: [&str; 4] = ["zen", "librewolf", "floorp", "waterfox"];

#[cfg(not(windows))]
fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from).unwrap_or_default()
}

/// Where a browser keeps its profiles on this system (Firefox family: the folder holding the
/// profile folders). Only browsers whose folder exists are offered.
fn profile_dir(id: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let (roaming, local) = (std::env::var_os("APPDATA").map(std::path::PathBuf::from)?, std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)?);
    #[cfg(windows)]
    let dir = match id {
        "zen" => roaming.join(r"zen\Profiles"),
        "librewolf" => roaming.join(r"librewolf\Profiles"),
        "floorp" => roaming.join(r"Floorp\Profiles"),
        "waterfox" => roaming.join(r"Waterfox\Profiles"),
        "firefox" => roaming.join(r"Mozilla\Firefox\Profiles"),
        "chrome" => local.join(r"Google\Chrome\User Data"),
        "chromium" => local.join(r"Chromium\User Data"),
        "brave" => local.join(r"BraveSoftware\Brave-Browser\User Data"),
        "vivaldi" => local.join(r"Vivaldi\User Data"),
        "edge" => local.join(r"Microsoft\Edge\User Data"),
        "opera" => roaming.join(r"Opera Software\Opera Stable"),
        _ => return None,
    };
    #[cfg(target_os = "macos")]
    let dir = home().join("Library").join(match id {
        "zen" => "Application Support/zen/Profiles",
        "librewolf" => "Application Support/librewolf/Profiles",
        "floorp" => "Application Support/Floorp/Profiles",
        "waterfox" => "Application Support/Waterfox/Profiles",
        "firefox" => "Application Support/Firefox/Profiles",
        "chrome" => "Application Support/Google/Chrome",
        "chromium" => "Application Support/Chromium",
        "brave" => "Application Support/BraveSoftware/Brave-Browser",
        "vivaldi" => "Application Support/Vivaldi",
        "edge" => "Application Support/Microsoft Edge",
        "opera" => "Application Support/com.operasoftware.Opera",
        "safari" => "Containers/com.apple.Safari",
        _ => return None,
    });
    #[cfg(not(any(windows, target_os = "macos")))]
    let dir = home().join(match id {
        "zen" => ".zen",
        "librewolf" => ".librewolf",
        "floorp" => ".floorp",
        "waterfox" => ".waterfox",
        "firefox" => ".mozilla/firefox",
        "chrome" => ".config/google-chrome",
        "chromium" => ".config/chromium",
        "brave" => ".config/BraveSoftware/Brave-Browser",
        "vivaldi" => ".config/vivaldi",
        "edge" => ".config/microsoft-edge",
        "opera" => ".config/opera",
        _ => return None,
    });
    Some(dir)
}

/// The profile actually in use: the one whose cookie store changed last.
fn newest_profile(root: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let db = e.path().join("cookies.sqlite");
            Some((std::fs::metadata(&db).ok()?.modified().ok()?, e.path()))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// What to pass to `--cookies-from-browser`.
fn ytdlp_browser(id: &str) -> Option<String> {
    if FIREFOX_FORKS.contains(&id) {
        return Some(format!("firefox:{}", newest_profile(&profile_dir(id)?)?.display()));
    }
    Some(id.to_string())
}

pub fn detect() -> Vec<(&'static str, &'static str)> {
    BROWSERS.iter().filter(|(id, _)| profile_dir(id).is_some_and(|d| d.is_dir())).copied().collect()
}

pub fn label(id: &str) -> &'static str {
    BROWSERS.iter().find(|(b, _)| *b == id).map_or("navegador", |(_, l)| l)
}

/// Keeps the Netscape header and the youtube.com cookies; drops every other site.
pub fn filter_youtube(netscape: &str) -> String {
    netscape
        .lines()
        .filter(|l| {
            if l.starts_with("# ") || l.starts_with("# Netscape") || l.is_empty() {
                return true;
            }
            // "#HttpOnly_" prefixes a real cookie line; other '#' lines are comments.
            let domain = l.strip_prefix("#HttpOnly_").unwrap_or(l).split('\t').next().unwrap_or("");
            domain == "youtube.com" || domain.ends_with(".youtube.com")
        })
        .map(|l| format!("{l}\n"))
        .collect()
}

fn signed_in(cookies: &str) -> bool {
    cookies.contains("\tSAPISID\t") || cookies.contains("\t__Secure-3PAPISID\t")
}

/// YouTube's cookies straight from a Firefox-family profile, as the same Netscape jar yt-dlp
/// would write, without starting yt-dlp (a Python process) on every launch. The database is
/// copied first (with its journal, so the newest changes are in): the browser keeps it locked.
/// Cookies inside containers count too, as with yt-dlp; outside ones win a tie.
#[cfg(target_os = "linux")]
fn firefox_cookies(profile: &Path) -> Result<String, String> {
    let tmp = data_dir().join(".firefox-cookies");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    make_private(&tmp);
    let read = || -> Result<String, String> {
        for f in ["cookies.sqlite", "cookies.sqlite-wal"] {
            let src = profile.join(f);
            if src.exists() {
                std::fs::copy(&src, tmp.join(f)).map_err(|e| e.to_string())?;
                make_private(&tmp.join(f));
            }
        }
        let db = rusqlite::Connection::open(tmp.join("cookies.sqlite")).map_err(|e| e.to_string())?;
        // Firefox 142 (schema 16) stores the expiry in milliseconds.
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        let mut q = db
            .prepare("SELECT host, name, value, path, expiry, isSecure, isHttpOnly, originAttributes FROM moz_cookies WHERE host = 'youtube.com' OR host LIKE '%.youtube.com'")
            .map_err(|e| e.to_string())?;
        type Row = (String, String, String, String, i64, bool, bool, String);
        let rows = q
            .query_map([], |r| -> rusqlite::Result<Row> { Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)) })
            .map_err(|e| e.to_string())?;
        let mut best: std::collections::HashMap<(String, String, String), Row> = Default::default();
        for row in rows.flatten() {
            let key = (row.0.clone(), row.3.clone(), row.1.clone());
            let in_container = row.7.contains("userContextId=");
            if !in_container || !best.contains_key(&key) {
                best.insert(key, row);
            }
        }
        let mut jar = String::from("# Netscape HTTP Cookie File\n");
        for (host, name, value, path, expiry, secure, http_only, _) in best.into_values() {
            let expiry = if version >= 16 { expiry / 1000 } else { expiry };
            let (prefix, sub) = (if http_only { "#HttpOnly_" } else { "" }, if host.starts_with('.') { "TRUE" } else { "FALSE" });
            let secure = if secure { "TRUE" } else { "FALSE" };
            jar += &format!("{prefix}{host}\t{sub}\t{path}\t{secure}\t{expiry}\t{name}\t{value}\n");
        }
        Ok(jar)
    };
    let jar = read();
    std::fs::remove_dir_all(&tmp).ok();
    jar
}

/// Exports the browser's cookies (Firefox family read directly, others through yt-dlp) and
/// returns only YouTube's. Blocking.
pub fn import(browser: &str) -> Result<String, String> {
    if !BROWSERS.iter().any(|(id, _)| *id == browser) {
        return Err(tr("navegador desconhecido").into());
    }
    #[cfg(target_os = "linux")]
    if (FIREFOX_FORKS.contains(&browser) || browser == "firefox")
        && let Some(yt) = profile_dir(browser).and_then(|d| newest_profile(&d)).and_then(|p| firefox_cookies(&p).ok()).map(|j| filter_youtube(&j)).filter(|j| signed_in(j))
    {
        return Ok(yt);
    }
    let source = ytdlp_browser(browser).ok_or_else(|| tf!("Perfil do {} não encontrado.", label(browser)))?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    make_private(&dir);
    // The export holds every site's cookies: it lives in our private folder for a moment, then goes.
    let tmp = dir.join(".browser-cookies.tmp");
    let run = || -> Result<String, String> {
        // Created owner-only before yt-dlp fills it. It must already be a valid (empty) cookie jar:
        // yt-dlp loads the file first and gives up on an empty one, writing nothing.
        crate::settings::write_private(&tmp, b"# Netscape HTTP Cookie File\n").map_err(|e| e.to_string())?;
        crate::settings::command(crate::settings::ytdlp())
            .args(["--ignore-config", "--quiet", "--no-warnings", "--cookies-from-browser", &source, "--cookies"])
            .arg(&tmp)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Without a URL yt-dlp exits with an error, but only after writing the cookie jar.
            .status()
            .map_err(|_| tr("yt-dlp não encontrado").to_string())?;
        std::fs::read_to_string(&tmp).map_err(|e| e.to_string())
    };
    let all = run();
    remove(&tmp);
    let yt = filter_youtube(&all?);
    if !signed_in(&yt) {
        return Err(tf!("Não achei um login do YouTube no {}. Entre em music.youtube.com nele e tente de novo.", label(browser)));
    }
    Ok(yt)
}

/// Our id of the system's default browser (from `xdg-settings`), if we support it. Asked once
/// per run: the login page draws every frame, and this starts a process.
pub fn default_browser() -> Option<&'static str> {
    static ONCE: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *ONCE.get_or_init(query_default_browser)
}

/// The default browser's own name for itself (a desktop file, a registry ProgId or a bundle id),
/// lowercase. Blocking: one small process.
fn default_browser_name() -> Option<String> {
    #[cfg(windows)]
    let out = crate::settings::command("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice", "/v", "ProgId"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    #[cfg(target_os = "macos")]
    let out = Command::new("defaults").args(["read", "com.apple.LaunchServices/com.apple.launchservices.secure", "LSHandlers"]).stderr(Stdio::null()).output().ok()?;
    #[cfg(not(any(windows, target_os = "macos")))]
    let out = Command::new("xdg-settings").args(["get", "default-web-browser"]).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    // macOS lists every handler: the https one's bundle id.
    #[cfg(target_os = "macos")]
    let text = text.split('}').find(|h| h.contains("lshandlerurlscheme = https"))?.split("lshandlerroleall = ").nth(1)?.split(';').next()?.trim_matches(['"', ' ']).to_string();
    Some(text)
}

fn query_default_browser() -> Option<&'static str> {
    let name = default_browser_name()?;
    // Desktop files (google-chrome.desktop), ProgIds (ChromeHTML, MSEdgeHTM, FirefoxURL-…) and
    // bundle ids (com.google.chrome, com.microsoft.edgemac, com.apple.safari) all name it.
    let id = [("google-chrome", "chrome"), ("chromehtml", "chrome"), ("com.google.chrome", "chrome"), ("microsoft-edge", "edge"), ("msedge", "edge"), ("edgemac", "edge"), ("brave", "brave"), ("operastable", "opera"), ("com.apple.safari", "safari")]
        .iter()
        .find(|(needle, _)| name.contains(needle))
        .map(|(_, id)| *id)
        .or_else(|| BROWSERS.iter().map(|(id, _)| *id).find(|id| name.contains(id)))?;
    BROWSERS.iter().map(|(b, _)| *b).find(|b| *b == id)
}

/// The default browser's signed-in YouTube session, with its cookies (kept in memory only).
/// Only the default browser is read: that's where the user expects their account to be.
/// Blocking: a second or two.
pub fn scan() -> Vec<(&'static str, String)> {
    default_browser().and_then(|id| import(id).ok().map(|c| (id, c))).into_iter().collect()
}

/// The Google sign-in page, landing back on YouTube Music.
pub const SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";

/// Opens `url` in a specific browser when we know how to launch it, else the default one.
pub fn open_in(browser: Option<&str>, url: &str) {
    #[cfg(windows)]
    {
        let _ = browser;
        crate::settings::command("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn().ok();
    }
    #[cfg(target_os = "macos")]
    {
        let app = browser.and_then(|b| match b {
            "chrome" => Some("Google Chrome"),
            "edge" => Some("Microsoft Edge"),
            "firefox" => Some("Firefox"),
            "brave" => Some("Brave Browser"),
            "safari" => Some("Safari"),
            _ => None,
        });
        let mut cmd = Command::new("open");
        if let Some(a) = app {
            cmd.args(["-a", a]);
        }
        cmd.arg(url).spawn().ok();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let launched = browser.and_then(|b| {
            let desktop = match b {
                "chrome" => "google-chrome",
                "edge" => "microsoft-edge",
                other => other,
            };
            Command::new("gtk-launch").args([desktop, url]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()
        });
        if launched.is_none() {
            Command::new("xdg-open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
        }
    }
}

/// The signed-in account, as shown in YouTube Music's account menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub name: String,
    pub handle: String,
    pub photo: Vec<rustypipe::model::Thumbnail>,
}

/// Reads the account header out of an `account/account_menu` response.
pub fn parse_account(v: &serde_json::Value) -> Option<Account> {
    fn find<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        match v {
            serde_json::Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find(x, key))),
            serde_json::Value::Array(a) => a.iter().find_map(|x| find(x, key)),
            _ => None,
        }
    }
    let h = find(v, "activeAccountHeaderRenderer")?;
    let text = |k: &str| h[k]["runs"][0]["text"].as_str().or_else(|| h[k]["simpleText"].as_str()).unwrap_or("").to_string();
    let photo: Vec<rustypipe::model::Thumbnail> = h["accountPhoto"]["thumbnails"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| serde_json::from_value(t.clone()).ok())
        .collect();
    let name = text("accountName");
    (!name.is_empty()).then(|| Account { name, handle: text("channelHandle"), photo })
}

fn remove(p: &Path) {
    std::fs::remove_file(p).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_youtube() {
        let jar = "# Netscape HTTP Cookie File\n\
                   .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n\
                   #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdef\n\
                   music.youtube.com\tFALSE\t/\tTRUE\t0\tPREF\tx\n\
                   .bank.example\tTRUE\t/\tTRUE\t0\tsession\tsecret\n\
                   .notyoutube.com\tTRUE\t/\tTRUE\t0\tevil\t1\n";
        let out = filter_youtube(jar);
        assert!(out.contains("SAPISID") && out.contains("SID\tdef") && out.contains("PREF"));
        assert!(!out.contains("bank") && !out.contains("evil"));
        assert!(signed_in(&out));
        assert!(import("lynx").is_err());
    }

    #[test]
    fn account_header() {
        let v = serde_json::json!({ "actions": [{ "openPopupAction": { "popup": { "multiPageMenuRenderer": { "header": {
            "activeAccountHeaderRenderer": {
                "accountName": { "runs": [{ "text": "Fulano" }] },
                "channelHandle": { "runs": [{ "text": "@fulano" }] },
                "accountPhoto": { "thumbnails": [{ "url": "https://x/y=s88", "width": 88, "height": 88 }] }
            }
        }}}}}]});
        let a = parse_account(&v).unwrap();
        assert_eq!((a.name.as_str(), a.handle.as_str(), a.photo.len()), ("Fulano", "@fulano", 1));
        assert!(parse_account(&serde_json::json!({})).is_none());
    }
}
