//! Crossfade between tracks. mpv plays one file at a time, so a second, audio-only mpv preloads the
//! next track, fades it in while the main one fades out (both through `volume-gain`, so the user's
//! volume is untouched), and once the main player has moved on to that track and caught up with
//! it, the helper goes quiet again.

use std::time::Instant;

use libmpv2::Mpv;

use crate::audio;
use crate::settings::Settings;

/// Preload this long before the fade starts: yt-dlp needs a few seconds to open a stream.
const LEAD: f64 = 12.0;
const SILENT: f64 = -96.0;

enum State {
    Idle,
    /// Next track loaded and paused in the helper.
    Armed { id: String, from: i64 },
    /// `k`: how far the fade is, 0..1.
    Fading { id: String, from: i64, k: f32 },
    /// Main player is on the next track; once it has opened it, it seeks to where the helper is.
    Handoff { id: String, target: Option<f64>, since: Instant, from: i64 },
}

pub struct Fader {
    mpv: Mpv,
    state: State,
}

/// What the main player is doing this tick.
pub struct Now<'a> {
    pub pos: i64,
    pub paused: bool,
    pub time: f64,
    pub dur: f64,
    pub current: Option<&'a str>,
    /// (id, url) of the track after the current one.
    pub next: Option<(&'a str, String)>,
}

fn gain_db(amplitude: f64) -> f64 {
    if amplitude <= 0.0016 { SILENT } else { (20.0 * amplitude.log10()).max(SILENT) }
}

/// A fade in progress, for the interface: `k` 0..1, the outgoing track's queue position, and the
/// incoming track's time and length (it's what is heard once past the middle).
pub struct Display {
    pub k: f32,
    pub from: i64,
    pub time: f64,
    pub dur: f64,
}

impl Fader {
    pub fn display(&self) -> Option<Display> {
        let (k, from) = match self.state {
            State::Fading { k, from, .. } => (k, from),
            State::Handoff { from, .. } => (1.0, from),
            _ => return None,
        };
        let time = self.mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
        let dur = self.mpv.get_property::<f64>("duration").unwrap_or(0.0);
        Some(Display { k, from, time, dur })
    }

    pub fn new() -> Option<Self> {
        let mpv = Mpv::with_initializer(|i| {
            i.set_property("vid", "no")?;
            i.set_property("video", "no")?;
            i.set_property("ytdl", "yes")?;
            i.set_property("idle", "yes")?;
            // Not "ytmfast": the Winamp analyser captures that node only.
            i.set_property("audio-client-name", "ytmfast-crossfade")?;
            Ok(())
        })
        .ok()?;
        Some(Self { mpv, state: State::Idle })
    }

    fn abort(&mut self, main: &Mpv) {
        if !matches!(self.state, State::Idle) {
            main.set_property("volume-gain", 0.0).ok();
            self.mpv.command("stop", &[]).ok();
            self.state = State::Idle;
        }
    }

    /// Stop any fade in progress (settings changed, video turned on, ...).
    pub fn cancel(&mut self, main: &Mpv) {
        self.abort(main);
    }

    pub fn tick(&mut self, main: &Mpv, s: &Settings, now: Now) {
        let secs = f64::from(s.crossfade_secs);
        let left = now.dur - now.time;
        // The helper follows the main volume and mute.
        if !matches!(self.state, State::Idle) {
            self.mpv.set_property("volume", main.get_property::<f64>("volume").unwrap_or(100.0)).ok();
            self.mpv.set_property("mute", main.get_property::<bool>("mute").unwrap_or(false)).ok();
        }
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => {
                let Some((id, url)) = now.next else { return };
                if secs > 0.0 && !now.paused && now.dur > secs * 2.0 + 5.0 && now.time > 1.0 && left <= secs + LEAD && left > secs {
                    audio::apply(&self.mpv, s);
                    self.mpv.set_property("loop-file", "no").ok();
                    self.mpv.set_property("loop-playlist", "no").ok();
                    self.mpv.set_property("pause", true).ok();
                    self.mpv.set_property("volume-gain", SILENT).ok();
                    self.mpv.command("loadfile", &[&url, "replace"]).ok();
                    self.state = State::Armed { id: id.to_string(), from: now.pos };
                }
            }
            State::Armed { id, from } => {
                self.state = State::Armed { id: id.clone(), from };
                if now.pos != from || now.paused || now.next.is_none_or(|(n, _)| n != id) || left > secs + LEAD + 2.0 {
                    return self.abort(main);
                }
                if left <= secs {
                    self.mpv.set_property("pause", false).ok();
                    self.state = State::Fading { id, from, k: 0.0 };
                }
            }
            State::Fading { id, from, .. } => {
                self.state = State::Fading { id: id.clone(), from, k: 0.0 };
                if now.pos == from {
                    // Seeked back out of the fade, or paused: drop it.
                    if now.paused || left > secs + 1.0 {
                        return self.abort(main);
                    }
                    // Equal-power curves: the sum stays about as loud as one track.
                    let frac = ((secs - left) / secs).clamp(0.0, 1.0);
                    self.state = State::Fading { id: id.clone(), from, k: frac as f32 };
                    let k = frac * std::f64::consts::FRAC_PI_2;
                    main.set_property("volume-gain", gain_db(k.cos())).ok();
                    self.mpv.set_property("volume-gain", gain_db(k.sin())).ok();
                } else if now.current == Some(id.as_str()) {
                    // Main moved on (track ended, or Next): muted until it catches up with the helper.
                    self.mpv.set_property("volume-gain", 0.0).ok();
                    main.set_property("volume-gain", SILENT).ok();
                    self.state = State::Handoff { id, target: None, since: Instant::now(), from };
                } else {
                    self.abort(main);
                }
            }
            State::Handoff { id, target, since, from } => {
                self.state = State::Handoff { id: id.clone(), target, since, from };
                // Paused, too slow, or the main player is on another track (the expected one failed to
                // load and mpv moved on): the helper must not seek the wrong song.
                if now.paused || since.elapsed().as_secs_f64() > 8.0 || now.current != Some(id.as_str()) {
                    return self.abort(main);
                }
                match target {
                    // A seek sent while the file is still opening is dropped: wait for its duration.
                    None if now.dur > 0.0 => {
                        let t = self.mpv.get_property::<f64>("time-pos").unwrap_or(0.0) + 0.4;
                        main.command("seek", &[&format!("{t:.2}"), "absolute"]).ok();
                        self.state = State::Handoff { id, target: Some(t), since, from };
                    }
                    Some(t) if !main.get_property::<bool>("seeking").unwrap_or(true) && now.time >= t - 0.1 => self.abort(main),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gains() {
        assert_eq!(super::gain_db(0.0), super::SILENT);
        assert!(super::gain_db(1.0).abs() < 1e-9);
        assert!((super::gain_db(0.5) + 6.02).abs() < 0.01);
    }
}
