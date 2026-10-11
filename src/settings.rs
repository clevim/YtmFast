//! Settings, on-disk locations and the small JSON files the app keeps between runs.

use std::path::{Path, PathBuf};

use rustypipe::model::{AlbumItem, MusicPlaylistItem, TrackItem};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

fn dir(xdg: &str, fallback: &str, windows: &str) -> PathBuf {
    // macOS keeps app files under ~/Library: caches in Caches, everything else in Application Support.
    #[cfg(target_os = "macos")]
    let fallback = if fallback == ".cache" { "Library/Caches" } else { "Library/Application Support" };
    std::env::var_os(xdg)
        .or_else(|| std::env::var_os(windows))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback))
        .join("ytmfast")
}

/// A helper process (yt-dlp, the registry tool): on Windows without the console window a GUI
/// program's children would otherwise flash up.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

const YTDLP: &str = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };

/// The yt-dlp the Windows and macOS packages ship beside the executable, if this is one.
pub fn bundled_ytdlp() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|e| Some(e.parent()?.join(YTDLP))).filter(|p| p.is_file())
}

/// Our own updatable copy of the bundled yt-dlp (the app folder may be read-only or signed).
pub fn own_ytdlp() -> PathBuf {
    data_dir().join("bin").join(YTDLP)
}

/// Which yt-dlp to run: our updated copy, else the bundled one, else the system's on the PATH.
pub fn ytdlp() -> PathBuf {
    Some(own_ytdlp()).filter(|p| p.is_file()).or_else(bundled_ytdlp).unwrap_or_else(|| PathBuf::from(YTDLP))
}

/// Packages that bring their own yt-dlp keep it current: YouTube changes often and an old copy
/// stops playing. The first run copies it to the data folder, then `yt-dlp -U` replaces that copy
/// when it's over a month old. Blocking (a download at most); run off the UI thread. Returns the
/// version in use.
pub fn refresh_ytdlp(days_old: impl Fn(&str) -> Option<i64>) -> String {
    let version = |p: &Path| command(p).arg("--version").stdin(std::process::Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let Some(bundled) = bundled_ytdlp() else { return version(&ytdlp()) };
    let own = own_ytdlp();
    if !own.is_file() {
        std::fs::create_dir_all(own.parent().unwrap_or(&own)).ok();
        if std::fs::copy(&bundled, &own).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o755)).ok();
            }
        }
    }
    let v = version(&own);
    if days_old(&v).is_some_and(|d| d > 30) {
        command(&own).args(["-U", "--quiet", "--no-warnings"]).stdin(std::process::Stdio::null()).output().ok();
        return version(&own);
    }
    v
}

/// The app was called ytfast: its folders move to the new name once, on the first start.
pub fn migrate_old_name() {
    for (xdg, fallback, windows) in [("XDG_CONFIG_HOME", ".config", "APPDATA"), ("XDG_DATA_HOME", ".local/share", "APPDATA"), ("XDG_STATE_HOME", ".local/state", "LOCALAPPDATA"), ("XDG_CACHE_HOME", ".cache", "LOCALAPPDATA")] {
        let new = dir(xdg, fallback, windows);
        let old = new.with_file_name("ytfast");
        if !new.exists() && old.is_dir() {
            std::fs::rename(&old, &new).ok();
        }
    }
}

/// Start with the session, hidden in the tray: a Run entry in the user's registry on Windows, a
/// LaunchAgent on macOS, an XDG autostart entry elsewhere.
#[cfg(windows)]
pub fn set_autostart(on: bool) {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = command("reg");
    if on {
        let Ok(exe) = std::env::current_exe() else { return };
        cmd.args(["add", KEY, "/v", "YtmFast", "/t", "REG_SZ", "/f", "/d", &format!("\"{}\" --tray", exe.display())]);
    } else {
        cmd.args(["delete", KEY, "/v", "YtmFast", "/f"]);
    }
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().ok();
}

#[cfg(target_os = "macos")]
pub fn set_autostart(on: bool) {
    let dir = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/LaunchAgents");
    let file = dir.join("com.ytmfast.app.plist");
    if !on {
        std::fs::remove_file(file).ok();
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let xml = exe.display().to_string().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>com.ytmfast.app</string>\n<key>ProgramArguments</key><array><string>{xml}</string><string>--tray</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n"
    );
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(file, plist).ok();
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_autostart(on: bool) {
    let dir = dir("XDG_CONFIG_HOME", ".config", "APPDATA").with_file_name("autostart");
    let file = dir.join("ytmfast.desktop");
    if !on {
        std::fs::remove_file(file).ok();
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    // Desktop Entry quoting: inside quotes \ " ` $ are escaped, and % (a field code) is doubled.
    let path = exe.display().to_string();
    let quoted: String = path.chars().flat_map(|c| match c {
        '\\' | '"' | '`' | '$' => vec!['\\', c],
        '%' => vec!['%', '%'],
        c => vec![c],
    }).collect();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=YtmFast\nComment=Player do YouTube Music, na bandeja\nExec=\"{quoted}\" --tray\nIcon=ytmfast\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
    );
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(file, entry).ok();
}

/// settings.json and themes/
pub fn config_dir() -> PathBuf {
    dir("XDG_CONFIG_HOME", ".config", "APPDATA")
}
/// rustypipe's cache and the login cookies
pub fn data_dir() -> PathBuf {
    dir("XDG_DATA_HOME", ".local/share", "APPDATA")
}
/// session.json, history.json
pub fn state_dir() -> PathBuf {
    dir("XDG_STATE_HOME", ".local/state", "LOCALAPPDATA")
}
/// artwork and library caches; always safe to delete
pub fn cache_dir() -> PathBuf {
    dir("XDG_CACHE_HOME", ".cache", "LOCALAPPDATA")
}
pub fn cookie_file() -> PathBuf {
    data_dir().join("cookies.txt")
}

/// yt-dlp writes its cookie jar back after every run, including cookies YouTube cleared or
/// rotated away; it gets this copy so our login (cookies.txt) is never overwritten.
pub fn ytdlp_cookie_file() -> PathBuf {
    data_dir().join("cookies-ytdlp.txt")
}

/// A bare file name inside one of our folders: no separators, no "..", no hidden files.
/// Settings that name files (theme, skin) must pass this so they can't point elsewhere.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

/// A proxy URL that's safe on mpv / yt-dlp's comma-separated option lists
/// (a comma would let it smuggle in extra yt-dlp options).
pub fn valid_proxy(p: &str) -> bool {
    p.len() < 256 && p.contains("://") && !p.contains([',', ' ', '\t', '\n', '\r', '"'])
}

/// Writes `bytes` to a file that is owner-only from the moment it exists (cookies, tokens): setting
/// the permissions afterwards would leave it readable for an instant.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    std::io::Write::write_all(&mut o.open(path)?, bytes)?;
    // An older file keeps its mode through `open`: tighten it too.
    make_private(path);
    Ok(())
}

/// A ListenBrainz token as it goes into a request header: letters, digits and dashes only (a
/// pasted newline must not add headers of its own).
pub fn valid_token(t: &str) -> bool {
    !t.is_empty() && t.len() <= 128 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Owner-only permissions (0700 dirs, 0600 files) for things holding the login cookie.
pub fn make_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).ok();
    }
    #[cfg(not(unix))]
    let _ = path;
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Atomic write: a crash mid-save never leaves a truncated file behind.
pub fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Some(parent) = path.parent() else { return };
    std::fs::create_dir_all(parent).ok();
    let tmp = path.with_extension("tmp");
    // Owner-only from the start: settings can hold a ListenBrainz token, and history and the queue
    // are nobody else's business either.
    if let Ok(txt) = serde_json::to_string(value)
        && write_private(&tmp, txt.as_bytes()).is_ok()
    {
        std::fs::rename(&tmp, path).ok();
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    High,
    Normal,
    Low,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    Off,
    All,
    One,
}

/// How the compact player is drawn, always in the theme's colours.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeckStyle {
    #[default]
    Modern,
    /// Raised chrome and a sunken display with segment digits, like Winamp 2.
    Classic,
    /// A pale screen inked in one colour, like an old handheld.
    Lcd,
}

pub const EQ_FREQS: [u32; 10] = [60, 170, 310, 600, 1000, 3000, 6000, 12000, 14000, 16000];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub volume: f64,
    pub quality: Quality,
    pub normalize: bool,
    pub gapless: bool,
    /// Seconds of crossfade between tracks; 0 = off.
    pub crossfade_secs: u32,
    /// Keep playing similar music when the queue runs out.
    pub autoplay: bool,
    pub audio_device: String,
    pub eq_on: bool,
    pub eq_preamp_db: f32,
    pub eq_bands_db: [f32; 10],
    /// Adaptive loudness: more bass and treble as the volume goes down.
    pub eq_loudness: bool,
    pub balance: f32,
    pub mono: bool,
    pub buffer_mb: u32,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// "system", "dark", "light" or a file name inside themes/
    pub theme: String,
    /// Interface language: "auto" (follows the system), "pt" or "en".
    pub language: String,
    pub accent_from_art: bool,
    pub effects_3d: bool,
    pub tilt: f32,
    pub reduce_motion: bool,
    pub tracklist_compact: bool,
    /// Lyrics word by word (KuGou's timing, karaoke style) instead of line by line.
    pub lyrics_words: bool,
    pub sidebar_compact: bool,
    pub zoom: f32,
    pub close_to_tray: bool,
    pub mini_on_top: bool,
    pub start_mini: bool,
    /// Start with the computer, hidden in the tray (no window until clicked).
    pub autostart_tray: bool,
    /// Winamp skin file in skins/; empty = the modern mini player.
    pub winamp_skin: String,
    /// Pick a different skin each time the mini player opens.
    pub random_skin: bool,
    /// Compact player: playlist / equalizer sections open.
    pub deck_playlist: bool,
    pub deck_eq: bool,
    pub deck_style: DeckStyle,
    pub skin_scale: u8,
    pub history: bool,
    /// Playback speed (1.0 = normal); mpv keeps the pitch.
    pub speed: f64,
    /// Desktop notification when the song changes and the window isn't in front.
    pub notify_track: bool,
    /// ListenBrainz user token; empty = no scrobbling.
    pub listenbrainz_token: String,
    /// Empty = system proxy settings.
    pub proxy: String,
    /// Browser the login was imported from; cookies are re-read from it on every start.
    pub login_browser: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 100.0,
            quality: Quality::High,
            normalize: false,
            gapless: true,
            crossfade_secs: 0,
            autoplay: true,
            audio_device: "auto".into(),
            eq_on: false,
            eq_preamp_db: 0.0,
            eq_bands_db: [0.0; 10],
            eq_loudness: false,
            balance: 0.0,
            mono: false,
            buffer_mb: 64,
            shuffle: false,
            repeat: Repeat::Off,
            theme: "system".into(),
            language: "auto".into(),
            accent_from_art: true,
            effects_3d: true,
            tilt: 1.0,
            reduce_motion: false,
            tracklist_compact: false,
            lyrics_words: false,
            sidebar_compact: false,
            zoom: 1.0,
            close_to_tray: true,
            mini_on_top: true,
            start_mini: false,
            autostart_tray: false,
            winamp_skin: String::new(),
            random_skin: false,
            deck_playlist: true,
            deck_eq: false,
            deck_style: DeckStyle::Modern,
            skin_scale: 2,
            history: true,
            speed: 1.0,
            notify_track: false,
            listenbrainz_token: String::new(),
            proxy: String::new(),
            login_browser: String::new(),
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("settings.json")
    }
    pub fn load() -> Self {
        let mut s: Self = read_json(&Self::path()).unwrap_or_default();
        s.sanitize();
        s
    }

    /// Clamps a hand-edited (or corrupted) settings file back into sane ranges.
    pub fn sanitize(&mut self) {
        let fin = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        self.volume = if self.volume.is_finite() { self.volume.clamp(0.0, 100.0) } else { 100.0 };
        self.eq_preamp_db = fin(self.eq_preamp_db, -12.0, 12.0, 0.0);
        for b in &mut self.eq_bands_db {
            *b = fin(*b, -12.0, 12.0, 0.0);
        }
        self.balance = fin(self.balance, -1.0, 1.0, 0.0);
        self.tilt = fin(self.tilt, 0.3, 2.0, 1.0);
        self.zoom = fin(self.zoom, 0.5, 2.0, 1.0);
        self.skin_scale = self.skin_scale.clamp(1, 4);
        self.buffer_mb = self.buffer_mb.clamp(8, 1024);
        self.crossfade_secs = self.crossfade_secs.min(12);
        self.speed = if self.speed.is_finite() { self.speed.clamp(0.5, 2.0) } else { 1.0 };
        if !matches!(self.language.as_str(), "auto" | "pt" | "en") {
            self.language = "auto".into();
        }
        if !self.proxy.is_empty() && !valid_proxy(self.proxy.trim()) {
            self.proxy.clear();
        }
        if !matches!(self.theme.as_str(), "system" | "dark" | "light") && !safe_name(&self.theme) {
            self.theme = "system".into();
        }
        if !crate::login::BROWSERS.iter().any(|(id, _)| *id == self.login_browser) {
            self.login_browser.clear();
        }
        if !self.winamp_skin.is_empty() && !safe_name(&self.winamp_skin) {
            self.winamp_skin.clear();
        }
    }
    pub fn save(&self) {
        write_json(&Self::path(), self);
    }
}

/// The queue as it was when the app closed.
#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    pub queue: Vec<TrackItem>,
    pub pos: i64,
    pub time: f64,
}

impl Session {
    pub fn path() -> PathBuf {
        state_dir().join("session.json")
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Played {
    pub track: TrackItem,
    /// Unix seconds
    pub at: u64,
}

pub const HISTORY_MAX: usize = 500;

pub fn history_path() -> PathBuf {
    state_dir().join("history.json")
}

/// Last known library, shown instantly on startup while the fresh copy loads.
#[derive(Serialize, Deserialize, Default)]
pub struct LibraryCache {
    pub playlists: Vec<MusicPlaylistItem>,
    pub albums: Vec<AlbumItem>,
    pub liked: Vec<TrackItem>,
    pub new_albums: Vec<AlbumItem>,
}

impl LibraryCache {
    pub fn path() -> PathBuf {
        cache_dir().join("library.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_settings_are_neutralised() {
        let mut s: Settings = serde_json::from_str(
            r#"{"theme": "../../etc/passwd", "winamp_skin": "/tmp/x.wsz", "proxy": "http://h:1,exec=rm -rf ~", "zoom": 40, "skin_scale": 9, "balance": -7}"#,
        )
        .unwrap();
        s.sanitize();
        assert_eq!((s.theme.as_str(), s.winamp_skin.as_str(), s.proxy.as_str()), ("system", "", ""));
        assert_eq!((s.zoom, s.skin_scale, s.balance), (2.0, 4, -1.0));
        assert!(safe_name("nord.json") && !safe_name("..") && !safe_name(".installed-palettes") && !safe_name("a/b"));
        assert!(valid_proxy("socks5://127.0.0.1:9050") && !valid_proxy("127.0.0.1:8080"));
        assert!(valid_token("0a1b2c3d-aaaa-bbbb-cccc-1234567890ab") && !valid_token("abc\r\nX-Evil: 1") && !valid_token(""));
        // Private writes: owner-only, also over an existing world-readable file.
        let p = std::env::temp_dir().join(format!("ytmfast-private-{}", std::process::id()));
        std::fs::write(&p, "old").unwrap();
        write_private(&p, b"secret").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "secret");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn settings_roundtrip_and_defaults() {
        // Unknown and missing fields must not wipe the user's settings.
        let s: Settings = serde_json::from_str(r#"{"volume": 40, "quality": "low", "bogus": 1}"#).unwrap();
        assert_eq!(s.volume, 40.0);
        assert_eq!(s.quality, Quality::Low);
        assert!(s.gapless);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
