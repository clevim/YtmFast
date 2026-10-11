//! Desktop notification for a new song while the window is out of sight. Linux: freedesktop
//! Notifications over the D-Bus connection the app already has, each one replacing the last.
//! Windows: a toast (notify-rust). macOS: Notification Center through the system's osascript.

#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_os = "linux")]
use zbus::zvariant::Value;

#[cfg(target_os = "linux")]
static CONN: OnceLock<zbus::Connection> = OnceLock::new();
/// Id of our last notification, so the next replaces it.
#[cfg(target_os = "linux")]
static LAST: AtomicU32 = AtomicU32::new(0);

#[cfg(windows)]
pub fn track(rt: &tokio::runtime::Runtime, title: String, artist: String) {
    rt.spawn_blocking(move || notify_rust::Notification::new().appname("YtmFast").summary(&title).body(&artist).show().ok());
}

#[cfg(target_os = "macos")]
pub fn track(rt: &tokio::runtime::Runtime, title: String, artist: String) {
    // AppleScript string literals: backslashes and quotes escaped, nothing else is interpreted.
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!("display notification {} with title {}", quote(&artist), quote(&title));
    rt.spawn_blocking(move || std::process::Command::new("osascript").args(["-e", &script]).output().ok());
}

#[cfg(target_os = "linux")]
pub fn track(rt: &tokio::runtime::Runtime, title: String, artist: String) {
    rt.spawn(async move {
        let conn = match CONN.get() {
            Some(c) => c.clone(),
            None => {
                let Ok(c) = zbus::Connection::session().await else { return };
                CONN.get_or_init(|| c).clone()
            }
        };
        let hints: HashMap<&str, Value> = HashMap::from([("desktop-entry", Value::from("ytmfast")), ("transient", Value::from(true))]);
        let body = ("YtmFast", LAST.load(Ordering::Relaxed), "ytmfast", title, artist, Vec::<&str>::new(), hints, 4000i32);
        let reply = conn.call_method(Some("org.freedesktop.Notifications"), "/org/freedesktop/Notifications", Some("org.freedesktop.Notifications"), "Notify", &body).await;
        if let Some(id) = reply.ok().and_then(|m| m.body().deserialize::<u32>().ok()) {
            LAST.store(id, Ordering::Relaxed);
        }
    });
}
