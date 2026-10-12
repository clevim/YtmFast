//! Real PCM for the Winamp analyser on Linux: `pw-record` captures our own mpv
//! stream (PipeWire node "ytmfast", from `audio-client-name`) straight off the graph.

use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 44_100;
const KEEP: usize = 16_384;

pub fn available() -> bool {
    Command::new("pw-record").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// True only if our own playback stream is in the graph right now. Second guard (with
/// `node.dont-fallback`) so capture can never land on an input device such as the microphone.
fn own_stream_exists() -> bool {
    let Ok(out) = Command::new("pw-dump").stdin(Stdio::null()).stderr(Stdio::null()).output() else { return false };
    let Ok(objs) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else { return false };
    objs.iter().any(|o| {
        let p = &o["info"]["props"];
        p["node.name"] == "ytmfast" && p["media.class"] == "Stream/Output/Audio"
    })
}

pub struct PwTap {
    child: Child,
    buf: Arc<Mutex<VecDeque<f32>>>,
}

impl PwTap {
    /// None if pw-record can't start. It exits on its own when the node isn't there.
    pub fn start() -> Option<Self> {
        if !own_stream_exists() {
            return None;
        }
        let mut child = Command::new("pw-record")
            .args(["--raw", "--target", "ytmfast", "--format", "f32", "--channels", "1", "--latency", "256"])
            .args(["--rate", &RATE.to_string()])
            // Without dont-fallback a missing target silently becomes the default source: the microphone.
            .args(["-P", "{ node.dont-fallback = true node.dont-reconnect = true node.passive = true }", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let buf = Arc::new(Mutex::new(VecDeque::with_capacity(KEEP)));
        let sink = buf.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = out.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = sink.lock().unwrap();
                // ponytail: assumes reads land on 4-byte boundaries, which pipes of f32 frames do in practice
                b.extend(chunk[..n].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
                let excess = b.len().saturating_sub(KEEP);
                b.drain(..excess);
            }
        });
        Some(Self { child, buf })
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Everything captured since the last call. `gain` undoes the player volume (the capture is post-volume).
    pub fn take(&self, gain: f32) -> Vec<f32> {
        self.buf.lock().unwrap().drain(..).map(|s| if s.is_finite() { (s * gain).clamp(-1.0, 1.0) } else { 0.0 }).collect()
    }
}

impl Drop for PwTap {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// mpv's volume curve is cubic; this brings the captured level back to full scale.
pub fn volume_gain(volume: f64) -> f32 {
    let v = (volume / 100.0) as f32;
    if v < 0.02 { 0.0 } else { (1.0 / (v * v * v)).min(1000.0) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gain_undoes_cubic_volume() {
        assert_eq!(super::volume_gain(100.0), 1.0);
        assert!((super::volume_gain(50.0) - 8.0).abs() < 1e-4);
        assert_eq!(super::volume_gain(0.0), 0.0);
    }
}
