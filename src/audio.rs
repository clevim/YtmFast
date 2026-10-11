//! Maps settings onto mpv properties. mpv/ffmpeg do the actual DSP.

use libmpv2::Mpv;

use crate::settings::{EQ_FREQS, Quality, Repeat, Settings, cookie_file, valid_proxy, write_private, ytdlp_cookie_file};

pub fn ytdl_format(q: Quality) -> &'static str {
    match q {
        Quality::High => "bestaudio/best",
        Quality::Normal => "bestaudio[abr<=130]/bestaudio",
        Quality::Low => "worstaudio/bestaudio[abr<=70]",
    }
}

/// Loudness compensation at full strength (volume 10% and below), per band: the ear loses bass and
/// treble at low levels (equal-loudness contours), so they come back as the volume goes down.
const LOUDNESS_DB: [f32; 10] = [9.0, 5.0, 2.0, 0.5, 0.0, 0.0, 1.0, 3.0, 3.5, 4.0];

/// How much loudness compensation applies at `volume` (0..100): none from 70% up.
pub fn loudness_amount(volume: f64) -> f32 {
    ((70.0 - volume) / 60.0).clamp(0.0, 1.0) as f32
}

/// mpv label of the equalizer's filter graph, for live `af-command` updates.
pub const EQ_LABEL: &str = "ytmeq";

/// The equalizer as (level in dB, firequalizer points), or None when it's off or flat.
pub fn eq_curve(s: &Settings, volume: f64) -> Option<(f32, String)> {
    if !s.eq_on {
        return None;
    }
    let k = if s.eq_loudness { loudness_amount(volume) } else { 0.0 };
    let bands: Vec<f32> = s.eq_bands_db.iter().zip(LOUDNESS_DB).map(|(g, l)| g + l * k).collect();
    // Loudness on keeps the filter in place even when flat, so volume changes only update it.
    if !s.eq_loudness && s.eq_preamp_db == 0.0 && bands.iter().all(|g| *g == 0.0) {
        return None;
    }
    // Headroom: the biggest boost comes off the level first, so boosted bands never clip and no
    // limiter has to squash the music.
    let gain = s.eq_preamp_db - bands.iter().cloned().fold(0.0, f32::max);
    let points: Vec<String> = EQ_FREQS.iter().zip(&bands).map(|(hz, g)| format!("entry({hz},{g:.1})")).collect();
    Some((gain, points.join(";")))
}

/// The `af` chain: equalizer, balance and loudness normalisation.
pub fn af_chain(s: &Settings, volume: f64) -> String {
    let mut f: Vec<String> = vec![];
    if let Some((gain, points)) = eq_curve(s, volume) {
        f.push(format!("volume={gain:.1}dB"));
        // One smooth curve through the ten points (linear-phase FIR). Separate peaking filters per
        // band overlap, and the close 12k/14k/16k bands stacked into a much deeper cut than set.
        f.push(format!("firequalizer=gain_entry='{points}'"));
        // Only a positive preamp with flat bands can still go over full scale.
        if gain > 0.0 {
            f.push("alimiter=limit=0.98:attack=5:release=200:level=disabled".into());
        }
    }
    if s.balance.abs() > 0.01 {
        f.push(format!("stereotools=balance_out={:.2}", s.balance));
    }
    if s.normalize {
        f.push("dynaudnorm=f=250:g=15".into());
    }
    if f.is_empty() { String::new() } else { format!("@{EQ_LABEL}:lavfi=[{}]", f.join(",")) }
}

/// Follows the volume with the loudness curve without rebuilding the chain (no dropout).
/// `sent` remembers what the filter has; returns false when the chain needs a rebuild instead.
pub fn update_eq(mpv: &Mpv, s: &Settings, volume: f64, sent: &mut String) -> bool {
    let Some((gain, points)) = eq_curve(s, volume) else { return true };
    let key = format!("{gain:.1}|{points}");
    if *sent == key {
        return true;
    }
    // The last argument names the filter inside the graph: mpv's default ("all") fails as soon as
    // one filter doesn't know the command.
    let ok = mpv.command("af-command", &[EQ_LABEL, "gain_entry", &points, "firequalizer"]).is_ok()
        && mpv.command("af-command", &[EQ_LABEL, "volume", &format!("{gain:.1}dB"), "volume"]).is_ok();
    *sent = key;
    ok
}

pub fn ytdl_raw_options(s: &Settings) -> String {
    let mut o = vec![];
    let cookies = ytdlp_cookie_file().display().to_string();
    // mpv splits this list on commas: a path or proxy containing one would inject options.
    if ytdlp_cookie_file().exists() && !cookies.contains(',') {
        o.push(format!("cookies={cookies}"));
    }
    if valid_proxy(s.proxy.trim()) {
        o.push(format!("proxy={}", s.proxy.trim()));
    }
    o.join(",")
}

/// Fresh copy of the login for yt-dlp (it rewrites whatever jar it's given).
fn refresh_ytdlp_cookies() {
    let copy = ytdlp_cookie_file();
    match std::fs::read(cookie_file()) {
        Ok(bytes) => drop(write_private(&copy, &bytes)),
        Err(_) => drop(std::fs::remove_file(&copy)),
    }
}

/// Every audio setting onto mpv, rebuilding the filter chain (a brief dropout if it changed).
pub fn apply(mpv: &Mpv, s: &Settings) {
    refresh_ytdlp_cookies();
    let set = |k: &str, v: &str| {
        mpv.set_property(k, v).ok();
    };
    set("ytdl-format", ytdl_format(s.quality));
    set("ytdl-raw-options", &ytdl_raw_options(s));
    set("gapless-audio", if s.gapless { "yes" } else { "weak" });
    set("audio-device", &s.audio_device);
    set("audio-channels", if s.mono { "mono" } else { "auto-safe" });
    set("demuxer-max-bytes", &format!("{}MiB", s.buffer_mb.max(8)));
    // Already-played audio kept for seeking back (mpv's default is 50 MiB); beyond it a seek refetches.
    set("demuxer-max-back-bytes", "8MiB");
    // Read ahead one minute, not the whole song (or the whole clip, with video): plenty for a
    // network stream, and memory stays flat while playing.
    set("cache-secs", "60");
    set("http-proxy", if valid_proxy(s.proxy.trim()) { s.proxy.trim() } else { "" });
    set("af", &af_chain(s, mpv.get_property::<f64>("volume").unwrap_or(s.volume)));
    set("loop-playlist", if s.repeat == Repeat::All { "inf" } else { "no" });
    set("loop-file", if s.repeat == Repeat::One { "inf" } else { "no" });
    set("speed", &s.speed.clamp(0.5, 2.0).to_string());
}

/// The video view adds only the picture to the song already playing; the audio stream keeps going
/// untouched. 720p, no AV1: light to download and to decode (in hardware where possible).
const VIDEO_ONLY: &str = "bestvideo[height<=?720][vcodec!^=av01]/best[height<=?720]";

/// Direct URL of a track's video stream, through yt-dlp (with the login copy and proxy). Blocking.
pub fn video_url(s: &Settings, page_url: &str) -> Option<String> {
    let mut cmd = crate::settings::command(crate::settings::ytdlp());
    cmd.args(["--ignore-config", "--no-warnings", "--quiet", "-f", VIDEO_ONLY, "-g"]);
    if ytdlp_cookie_file().exists() {
        cmd.arg("--cookies").arg(ytdlp_cookie_file());
    }
    if valid_proxy(s.proxy.trim()) {
        cmd.args(["--proxy", s.proxy.trim()]);
    }
    let out = cmd.arg(page_url).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    let url = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    url.starts_with("https://").then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_chains() {
        let mut s = Settings::default();
        assert_eq!(af_chain(&s, 100.0), "");
        s.eq_on = true;
        s.eq_bands_db[0] = 3.0;
        s.eq_preamp_db = -2.0;
        s.normalize = true;
        // Boost: headroom equal to it (plus the preamp), one curve, no limiter.
        let curve = |g0: &str| format!("firequalizer=gain_entry='entry(60,{g0});entry(170,0.0);entry(310,0.0);entry(600,0.0);entry(1000,0.0);entry(3000,0.0);entry(6000,0.0);entry(12000,0.0);entry(14000,0.0);entry(16000,0.0)'");
        assert_eq!(af_chain(&s, 100.0), format!("@ytmeq:lavfi=[volume=-5.0dB,{},dynaudnorm=f=250:g=15]", curve("3.0")));
        // Cuts only: level untouched, no limiter.
        s.eq_bands_db[0] = -3.0;
        s.eq_preamp_db = 0.0;
        s.normalize = false;
        assert_eq!(af_chain(&s, 100.0), format!("@ytmeq:lavfi=[volume=0.0dB,{}]", curve("-3.0")));
        // Positive preamp, flat bands: the limiter guards full scale.
        s.eq_bands_db[0] = 0.0;
        s.eq_preamp_db = 3.0;
        assert_eq!(af_chain(&s, 100.0), format!("@ytmeq:lavfi=[volume=3.0dB,{},alimiter=limit=0.98:attack=5:release=200:level=disabled]", curve("0.0")));
        // Loudness: nothing at 70%+, full bass/treble lift at 10%.
        s.eq_preamp_db = 0.0;
        s.eq_loudness = true;
        assert_eq!(loudness_amount(80.0), 0.0);
        assert_eq!(loudness_amount(10.0), 1.0);
        let (gain, points) = eq_curve(&s, 10.0).unwrap();
        assert_eq!(gain, -9.0);
        assert!(points.starts_with("entry(60,9.0);entry(170,5.0)"));
        assert_eq!(eq_curve(&s, 90.0).unwrap().0, 0.0);
    }
}
