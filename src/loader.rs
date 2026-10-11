//! Image bytes loader with a disk cache, so covers survive restarts instead of being re-downloaded.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
};

/// Covers are a few hundred KB; anything far bigger isn't a cover.
const MAX_IMAGE: usize = 16 << 20;

/// Disk budget for cached covers; the oldest go first when it's exceeded.
const MAX_CACHE: u64 = 300 << 20;

/// Deletes the least recently written files until `dir` fits in `max` bytes.
fn prune(dir: &std::path::Path, max: u64) {
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= max {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// Raw image bytes kept in memory (they're also on disk, and read back in microseconds): past this
/// the oldest go.
const MAX_HELD: usize = 8 << 20;
/// Threads fetching / reading images: enough to fill a screen of covers quickly.
const WORKERS: usize = 4;

enum Entry {
    Pending,
    /// Bytes and an insertion counter (older = smaller), for evicting the oldest.
    Ready(Arc<[u8]>, u64),
    /// Failure and when it happened: retried after `RETRY` (a network blip shouldn't stick).
    Failed(String, std::time::Instant),
}

const RETRY: std::time::Duration = std::time::Duration::from_secs(30);

type Cache = Arc<Mutex<HashMap<String, Entry>>>;
type Job = (String, PathBuf, egui::Context);

pub struct DiskLoader {
    dir: PathBuf,
    cache: Cache,
    jobs: std::sync::mpsc::Sender<Job>,
}

/// Covers written to disk between two checks of the cache's size (also checked at start-up).
const PRUNE_EVERY: u64 = 200;

/// Reads `path`, or downloads `uri` (and keeps it on disk).
fn fetch(uri: &str, path: &std::path::Path) -> Entry {
    let now = std::time::Instant::now();
    match std::fs::read(path) {
        Ok(b) => Entry::Ready(b.into(), 0),
        Err(_) => match ehttp::fetch_blocking(&ehttp::Request::get(uri)) {
            Ok(r) if r.ok && r.bytes.len() > MAX_IMAGE => Entry::Failed("imagem grande demais".into(), now),
            Ok(r) if r.ok => {
                // Written aside and renamed: a crash mid-write must not leave a broken cover behind.
                let tmp = path.with_extension("part");
                if std::fs::write(&tmp, &r.bytes).is_ok() {
                    std::fs::rename(&tmp, path).ok();
                }
                Entry::Ready(r.bytes.into(), 0)
            }
            Ok(r) => Entry::Failed(format!("HTTP {}", r.status), now),
            Err(e) => Entry::Failed(e, now),
        },
    }
}

/// Stores a finished entry, then drops the oldest bytes while over `MAX_HELD`.
fn store(cache: &mut HashMap<String, Entry>, uri: String, entry: Entry, seq: u64) {
    let entry = match entry {
        Entry::Ready(b, _) => Entry::Ready(b, seq),
        e => e,
    };
    cache.insert(uri, entry);
    let mut held: Vec<(u64, usize, String)> = cache
        .iter()
        .filter_map(|(k, e)| if let Entry::Ready(b, s) = e { Some((*s, b.len(), k.clone())) } else { None })
        .collect();
    let mut total: usize = held.iter().map(|h| h.1).sum();
    if total <= MAX_HELD {
        return;
    }
    held.sort();
    for (_, len, key) in held {
        if total <= MAX_HELD {
            break;
        }
        cache.remove(&key);
        total -= len;
    }
}

impl DiskLoader {
    pub fn new(dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).ok();
        let d = dir.clone();
        std::thread::spawn(move || prune(&d, MAX_CACHE));
        let cache: Cache = Default::default();
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
        for _ in 0..WORKERS {
            let (rx, cache, seq, dir) = (rx.clone(), cache.clone(), seq.clone(), dir.clone());
            std::thread::spawn(move || {
                loop {
                    // The lock only covers taking a job, not doing it.
                    let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok((uri, path, ctx)) = job else { break };
                    let entry = fetch(&uri, &path);
                    let n = seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // A long session keeps adding covers: hold the disk budget then too.
                    if n % PRUNE_EVERY == 0 {
                        prune(&dir, MAX_CACHE);
                    }
                    store(&mut cache.lock().unwrap_or_else(|e| e.into_inner()), uri, entry, n);
                    ctx.request_repaint();
                }
            });
        }
        Self { dir, cache, jobs }
    }

    fn file(&self, uri: &str) -> PathBuf {
        let mut h = std::hash::DefaultHasher::new();
        uri.hash(&mut h);
        self.dir.join(format!("{:016x}", h.finish()))
    }
}

impl BytesLoader for DiskLoader {
    fn id(&self) -> &str {
        egui::generate_loader_id!(DiskLoader)
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !uri.starts_with("https://") && !uri.starts_with("http://") {
            return Err(LoadError::NotSupported);
        }
        let mut cache = self.cache.lock().unwrap();
        match cache.get(uri) {
            Some(Entry::Ready(b, _)) => return Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(b.clone()), mime: None }),
            Some(Entry::Pending) => return Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Failed(e, at)) if at.elapsed() < RETRY => return Err(LoadError::Loading(e.clone())),
            Some(Entry::Failed(..)) | None => {}
        }
        cache.insert(uri.to_owned(), Entry::Pending);
        self.jobs.send((uri.to_owned(), self.file(uri), ctx.clone())).ok();
        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().unwrap().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache.lock().unwrap().values().map(|e| if let Entry::Ready(b, _) = e { b.len() } else { 0 }).sum()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prune_keeps_the_newest() {
        let dir = std::env::temp_dir().join(format!("ytmfast-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            std::fs::write(dir.join(name), vec![0u8; 100]).unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i as u64);
            std::fs::File::options().write(true).open(dir.join(name)).unwrap().set_modified(t).unwrap();
        }
        super::prune(&dir, 150);
        // In-memory bytes: the oldest go first past the budget.
        let mut cache = std::collections::HashMap::new();
        for (i, k) in ["a", "b", "c"].iter().enumerate() {
            super::store(&mut cache, k.to_string(), super::Entry::Ready(vec![0u8; super::MAX_HELD / 2].into(), 0), i as u64 + 1);
        }
        let mut kept: Vec<_> = cache.keys().cloned().collect();
        kept.sort();
        assert_eq!(kept, ["b", "c"]);
        let left: Vec<_> = ["old", "mid", "new"].iter().filter(|n| dir.join(n).exists()).collect();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(left, [&"new"]);
    }
}
