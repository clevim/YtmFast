//! MPRIS on Linux, our own instead of souvlaki's: Plasma ties a player to its taskbar entry and
//! tray media card through `DesktopEntry`, which souvlaki doesn't publish, and the position has to
//! follow seeks. The event and metadata types mirror souvlaki's (used on Windows / macOS), so the
//! app code is the same everywhere.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekDirection {
    Forward,
    Backward,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MediaPosition(pub Duration);

#[derive(Clone, PartialEq, Debug)]
pub enum MediaControlEvent {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    Seek(SeekDirection),
    SeekBy(SeekDirection, Duration),
    SetPosition(MediaPosition),
    SetVolume(f64),
    OpenUri(String),
    Raise,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaPlayback {
    Stopped,
    Paused { progress: Option<MediaPosition> },
    Playing { progress: Option<MediaPosition> },
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct MediaMetadata<'a> {
    pub title: Option<&'a str>,
    pub album: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub cover_url: Option<&'a str>,
    pub duration: Option<Duration>,
}
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";

type Handler = Arc<dyn Fn(MediaControlEvent) + Send + Sync>;

#[derive(Default)]
struct State {
    status: &'static str,
    meta: HashMap<String, OwnedValue>,
    /// Microseconds, as of `at`.
    position: i64,
    at: Option<std::time::Instant>,
    volume: f64,
}

type Shared = Arc<Mutex<State>>;

struct Root(Handler);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.0)(MediaControlEvent::Raise);
    }
    fn quit(&self) {
        (self.0)(MediaControlEvent::Quit);
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "YtmFast"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "ytmfast"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["https".into()]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

struct Player {
    h: Handler,
    st: Shared,
}

impl Player {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        (self.h)(MediaControlEvent::Next);
    }
    fn previous(&self) {
        (self.h)(MediaControlEvent::Previous);
    }
    fn pause(&self) {
        (self.h)(MediaControlEvent::Pause);
    }
    fn play_pause(&self) {
        (self.h)(MediaControlEvent::Toggle);
    }
    fn stop(&self) {
        (self.h)(MediaControlEvent::Stop);
    }
    fn play(&self) {
        (self.h)(MediaControlEvent::Play);
    }
    fn seek(&self, offset: i64) {
        let dir = if offset >= 0 { SeekDirection::Forward } else { SeekDirection::Backward };
        (self.h)(MediaControlEvent::SeekBy(dir, Duration::from_micros(offset.unsigned_abs())));
    }
    fn set_position(&self, _track: ObjectPath<'_>, position: i64) {
        (self.h)(MediaControlEvent::SetPosition(MediaPosition(Duration::from_micros(position.max(0) as u64))));
    }
    fn open_uri(&self, uri: String) {
        (self.h)(MediaControlEvent::OpenUri(uri));
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let s = self.st().status;
        if s.is_empty() { "Stopped".into() } else { s.into() }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.st().meta.iter().filter_map(|(k, v)| Some((k.clone(), v.try_clone().ok()?))).collect()
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.st().position
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.st().volume
    }
    #[zbus(property)]
    fn set_volume(&mut self, v: f64) {
        (self.h)(MediaControlEvent::SetVolume(v.clamp(0.0, 1.0)));
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

enum Change {
    Status,
    Meta,
    Volume,
    Seeked(i64),
}

pub struct Controls {
    st: Shared,
    tx: tokio::sync::mpsc::UnboundedSender<Change>,
}

impl Controls {
    /// None when there's no session bus.
    pub fn spawn(rt: &tokio::runtime::Runtime, handler: impl Fn(MediaControlEvent) + Send + Sync + 'static) -> Option<Self> {
        let h: Handler = Arc::new(handler);
        let st: Shared = Arc::new(Mutex::new(State { volume: 1.0, ..Default::default() }));
        let player = Player { h: h.clone(), st: st.clone() };
        let conn = rt
            .block_on(async {
                let conn = zbus::connection::Builder::session()?.serve_at(PATH, Root(h))?.serve_at(PATH, player)?.build().await?;
                // A second copy (or a stale one) holds the plain name: take a per-process one.
                if conn.request_name("org.mpris.MediaPlayer2.ytmfast").await.is_err() {
                    conn.request_name(format!("org.mpris.MediaPlayer2.ytmfast.instance{}", std::process::id())).await?;
                }
                zbus::Result::Ok(conn)
            })
            .ok()?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        rt.spawn(async move {
            let Ok(iface) = conn.object_server().interface::<_, Player>(PATH).await else { return };
            while let Some(change) = rx.recv().await {
                let p = iface.get().await;
                let e = iface.signal_emitter();
                let _ = match change {
                    Change::Status => p.playback_status_changed(e).await,
                    Change::Meta => p.metadata_changed(e).await,
                    Change::Volume => p.volume_changed(e).await,
                    Change::Seeked(us) => Player::seeked(e, us).await,
                };
            }
        });
        Some(Self { st, tx })
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_metadata(&mut self, m: MediaMetadata) -> Result<(), ()> {
        let mut meta: HashMap<String, OwnedValue> = HashMap::new();
        let mut put = |k: &str, v: Value| {
            if let Ok(v) = OwnedValue::try_from(v) {
                meta.insert(k.into(), v);
            }
        };
        put("mpris:trackid", Value::from(ObjectPath::from_static_str_unchecked("/org/ytmfast/track")));
        if let Some(t) = m.title {
            put("xesam:title", Value::from(t));
        }
        if let Some(a) = m.artist {
            put("xesam:artist", Value::from(vec![a]));
        }
        if let Some(a) = m.album {
            put("xesam:album", Value::from(a));
        }
        if let Some(u) = m.cover_url {
            put("mpris:artUrl", Value::from(u));
        }
        if let Some(d) = m.duration {
            put("mpris:length", Value::from(d.as_micros() as i64));
        }
        self.st().meta = meta;
        self.tx.send(Change::Meta).map_err(drop)
    }

    pub fn set_playback(&mut self, p: MediaPlayback) -> Result<(), ()> {
        let (status, progress) = match p {
            MediaPlayback::Stopped => ("Stopped", None),
            MediaPlayback::Paused { progress } => ("Paused", progress),
            MediaPlayback::Playing { progress } => ("Playing", progress),
        };
        let mut st = self.st();
        st.status = status;
        if let Some(MediaPosition(d)) = progress {
            st.position = d.as_micros() as i64;
        }
        drop(st);
        self.tx.send(Change::Status).map_err(drop)
    }

    /// Called every tick: keeps Position current and tells listeners about jumps (seeks).
    pub fn set_position(&mut self, secs: f64) {
        let us = (secs.max(0.0) * 1e6) as i64;
        let now = std::time::Instant::now();
        let mut st = self.st();
        let dt = st.at.map_or(0, |a| now.duration_since(a).as_micros() as i64);
        let expected = st.position + if st.status == "Playing" { dt } else { 0 };
        st.position = us;
        st.at = Some(now);
        drop(st);
        if (us - expected).abs() > 1_500_000 {
            self.tx.send(Change::Seeked(us)).ok();
        }
    }

    pub fn set_volume(&mut self, v: f64) {
        let mut st = self.st();
        if (st.volume - v).abs() > 0.004 {
            st.volume = v;
            drop(st);
            self.tx.send(Change::Volume).ok();
        }
    }
}
