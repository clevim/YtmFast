//! Single instance + command-line control (`ytmfast play-pause`, `ytmfast now-playing`, links...).
//!
//! Unix: a socket in a per-user 0700 directory. Windows: loopback TCP plus a random token kept in
//! the per-user state folder, so other local users can't drive the player.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use crate::{Msg, Waker};
use crate::i18n::tr;

pub const HELP: &str = "\
Uso: ytmfast [COMANDO | LINK]

Sem argumentos abre o app (ou traz para frente o que já está aberto).
LINK: endereço do YouTube Music / YouTube (música ou playlist) para tocar.

Comandos (controlam o app aberto):
  play-pause, play, pause, stop, next, previous
  seek <segundos>        avança (ou volta, se negativo)
  seek-to <segundos>     vai para a posição
  volume <0-100>, volume-up, volume-down, mute
  shuffle, repeat, like, show, quit
  now-playing [--raw]    faixa atual (--raw: JSON)
  play-uri <link>        toca um link
  --tray                 inicia escondido na bandeja
";

const HELP_EN: &str = "\
Usage: ytmfast [COMMAND | LINK]

With no arguments, opens the app (or brings the open one to the front).
LINK: a YouTube Music / YouTube address (song or playlist) to play.

Commands (control the running app):
  play-pause, play, pause, stop, next, previous
  seek <seconds>         forward (or back, if negative)
  seek-to <seconds>      jump to the position
  volume <0-100>, volume-up, volume-down, mute
  shuffle, repeat, like, show, quit
  now-playing [--raw]    current track (--raw: JSON)
  play-uri <link>        play a link
  --tray                 start hidden in the tray
";

pub fn help() -> &'static str {
    if crate::i18n::english() { HELP_EN } else { HELP }
}

const VERBS: [&str; 19] = [
    "play-pause", "play", "pause", "stop", "next", "previous", "seek", "seek-to", "volume", "volume-up", "volume-down",
    "mute", "shuffle", "repeat", "like", "show", "quit", "now-playing", "play-uri",
];
const MAX_LINE: u64 = 4096;

pub fn is_link(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// What a link points to.
#[derive(Debug, PartialEq)]
pub enum Link {
    Track(String),
    Playlist(String),
}

pub fn parse_link(url: &str) -> Option<Link> {
    let query = url.split_once('?').map_or("", |(_, q)| q);
    let param = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')).map(|v| v.split('#').next().unwrap_or(v).to_string());
    let valid = |id: &str| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if let Some(v) = param("v").filter(|v| valid(v)) {
        return Some(Link::Track(v));
    }
    if let Some(rest) = url.split("youtu.be/").nth(1) {
        let id = rest.split(['?', '&', '#', '/']).next().unwrap_or("");
        if valid(id) {
            return Some(Link::Track(id.to_string()));
        }
    }
    param("list").filter(|l| valid(l)).map(Link::Playlist)
}

#[cfg(unix)]
fn socket_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("ytmfast-{}", std::env::var("USER").unwrap_or_default())));
    base.join("ytmfast").join("ytmfast.sock")
}

/// The socket's folder must be ours alone. Without XDG_RUNTIME_DIR it sits in the shared temp
/// folder, where another user could have created it first to listen in or answer for us.
#[cfg(unix)]
fn dir_is_ours(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // SAFETY: getuid has no arguments and can't fail.
    let uid = unsafe { libc::getuid() };
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && m.uid() == uid && m.permissions().mode() & 0o077 == 0)
}

#[cfg(windows)]
fn key_path() -> std::path::PathBuf {
    crate::settings::state_dir().join("instance.key")
}

/// Sends one command to the running instance. None = no instance is running.
fn request(line: &str) -> Option<String> {
    #[cfg(unix)]
    let mut stream = {
        let path = socket_path();
        if !path.parent().is_some_and(dir_is_ours) {
            return None;
        }
        std::os::unix::net::UnixStream::connect(path).ok()?
    };
    #[cfg(windows)]
    let (mut stream, token) = {
        let key = std::fs::read_to_string(key_path()).ok()?;
        let (port, token) = key.trim().split_once(' ')?;
        (std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().ok()?)).ok()?, token.to_string())
    };
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    #[cfg(windows)]
    writeln!(stream, "{token}").ok()?;
    writeln!(stream, "{line}").ok()?;
    let mut out = String::new();
    stream.take(1 << 20).read_to_string(&mut out).ok()?;
    Some(out)
}

/// Handles the command line before the UI starts. Some(code) = done, exit with it.
/// None = start the app (with `link` to play, if any).
pub fn client(args: &[String]) -> Option<i32> {
    let Some(first) = args.first() else {
        // Second launch: raise the existing window instead of opening another player.
        return request("show").map(|_| 0);
    };
    // Login autostart: nothing to do if the app is already running.
    if first == "--tray" {
        return request("now-playing --raw").map(|_| 0);
    }
    if first == "-h" || first == "--help" {
        print!("{}", help());
        return Some(0);
    }
    if is_link(first) {
        return request(&format!("play-uri {first}")).map(|_| 0);
    }
    if !VERBS.contains(&first.as_str()) {
        eprint!("{}", tf!("Comando desconhecido: {}\n\n{}", first, help()));
        return Some(2);
    }
    match request(&args.join(" ")) {
        Some(reply) => {
            let reply = reply.trim_end();
            if let Some(err) = reply.strip_prefix("erro: ") {
                eprintln!("{err}");
                return Some(1);
            }
            if !reply.is_empty() && reply != "ok" {
                println!("{reply}");
            }
            Some(0)
        }
        None => {
            eprintln!("{}", tr("O YtmFast não está aberto."));
            Some(1)
        }
    }
}

/// Accepts commands for the running app. Each is handed to the UI as `Msg::Ipc` with a reply channel.
pub fn serve(tx: Sender<Msg>, waker: Waker) {
    #[cfg(unix)]
    let listener = {
        use std::os::unix::fs::PermissionsExt;
        let path = socket_path();
        let dir = path.parent().expect("socket dir");
        std::fs::create_dir_all(dir).ok();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok();
        if !dir_is_ours(dir) {
            return;
        }
        // Only reached when no instance answered, so a leftover socket is stale.
        std::fs::remove_file(&path).ok();
        match std::os::unix::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(_) => return,
        }
    };
    #[cfg(windows)]
    let (listener, token) = {
        use std::hash::BuildHasher;
        let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", 0)) else { return };
        // RandomState is keyed from the OS RNG: two hashes = 128 unguessable bits.
        let r = || std::collections::hash_map::RandomState::new().hash_one(std::process::id());
        let token = format!("{:016x}{:016x}", r(), r());
        let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
        std::fs::create_dir_all(crate::settings::state_dir()).ok();
        std::fs::write(key_path(), format!("{port} {token}")).ok();
        (l, token)
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let Ok(read_half) = stream.try_clone() else { continue };
            let mut reader = BufReader::new(read_half.take(MAX_LINE));
            #[cfg(windows)]
            {
                let mut t = String::new();
                if reader.read_line(&mut t).is_err() || t.trim() != token {
                    continue;
                }
            }
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let (reply_tx, reply_rx) = channel();
            if tx.send(Msg::Ipc(line.trim().to_string(), reply_tx)).is_err() {
                break;
            }
            waker.wake();
            // "erro: " is the protocol's marker (the client looks for it): never translated.
            let reply = reply_rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| format!("erro: {}", tr("sem resposta")));
            let mut stream = stream;
            writeln!(stream, "{reply}").ok();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert_eq!(parse_link("https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=RDAMVM"), Some(Link::Track("dQw4w9WgXcQ".into())));
        assert_eq!(parse_link("https://youtu.be/dQw4w9WgXcQ?si=x"), Some(Link::Track("dQw4w9WgXcQ".into())));
        assert_eq!(parse_link("https://music.youtube.com/playlist?list=PLabc_-1"), Some(Link::Playlist("PLabc_-1".into())));
        assert_eq!(parse_link("https://music.youtube.com/watch?v=bad;rm"), None);
        assert_eq!(parse_link("https://example.com/"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let d = std::env::temp_dir().join(format!("ytmfast-ipc-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert!(dir_is_ours(&d));
            // Group or others can get in: refused.
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert!(!dir_is_ours(&d));
            std::fs::remove_dir(&d).ok();
            assert!(!dir_is_ours(std::path::Path::new("/")));
        }
    }
}
