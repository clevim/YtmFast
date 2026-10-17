//! Lyrics, time-synced when possible. YouTube Music only hands its timed lyrics (Musixmatch) to the
//! Android app, so that one request goes out as that client; LRCLIB fills in when YTM has none.
//! With "word by word" on, KuGou's KRC (each word's timing, for karaoke-style lyrics) is tried
//! first.

use rustypipe::client::RustyPipeQuery;
use rustypipe::model::TrackItem;
use serde_json::{Value, json};
use crate::i18n::tr;

/// One word of a line: (start, end, text), in seconds.
pub type Word = (f64, f64, String);

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    /// (start in seconds, line). Empty = only `plain` is known.
    pub synced: Vec<(f64, String)>,
    pub plain: String,
    pub source: String,
    /// Per synced line, its words. Empty = no word timing.
    pub words: Vec<Vec<Word>>,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.synced.is_empty() && self.plain.trim().is_empty()
    }

    /// Index of the line being sung at `t` seconds.
    pub fn line_at(&self, t: f64) -> Option<usize> {
        self.synced.iter().rposition(|(s, _)| *s <= t)
    }
}

/// Any failure ends as empty lyrics instead of an endless "loading". `words`: try KuGou's
/// word-timed lyrics first.
pub async fn fetch(q: &RustyPipeQuery, t: &TrackItem, words: bool) -> Lyrics {
    if words {
        let (artist, name, dur) = (t.artists.first().map(|a| a.name.clone()).unwrap_or_default(), t.name.clone(), t.duration);
        if let Some(l) = tokio::task::spawn_blocking(move || kugou(&artist, &name, dur)).await.ok().flatten() {
            return l;
        }
    }
    let lyrics_id = crate::music_details(q, &t.id).await.ok().and_then(|d| d.lyrics_id);
    if let Some(lid) = lyrics_id.clone() {
        let timed = tokio::task::spawn_blocking(move || ytm_timed(&lid)).await.ok().flatten();
        if let Some(l) = timed {
            return l;
        }
    }
    let (artist, name, dur) = (t.artists.first().map(|a| a.name.clone()).unwrap_or_default(), t.name.clone(), t.duration);
    let lrclib = tokio::task::spawn_blocking(move || lrclib(&artist, &name, dur)).await.ok().flatten();
    if let Some(l) = lrclib.as_ref().filter(|l| !l.synced.is_empty()) {
        return l.clone();
    }
    if let Some(lid) = lyrics_id
        && let Ok(l) = q.music_lyrics(&lid).await
    {
        return Lyrics { synced: vec![], plain: l.body, source: l.footer, words: vec![] };
    }
    lrclib.unwrap_or_default()
}

fn post_json(url: &str, headers: &[(&str, &str)], body: &Value) -> Option<Value> {
    let mut req = ehttp::Request::post(url, serde_json::to_vec(body).ok()?);
    req.headers = ehttp::Headers::new(&[("Accept", "*/*"), ("Content-Type", "application/json")]);
    for (k, v) in headers {
        req.headers.insert(*k, *v);
    }
    let r = ehttp::fetch_blocking(&req).ok().filter(|r| r.ok)?;
    serde_json::from_slice(&r.bytes).ok()
}

fn ytm_timed(lyrics_id: &str) -> Option<Lyrics> {
    let body = json!({
        "context": { "client": { "clientName": "ANDROID_MUSIC", "clientVersion": "7.21.50", "androidSdkVersion": 34 } },
        "browseId": lyrics_id,
    });
    let ua = [("User-Agent", "com.google.android.apps.youtube.music/7.21.50 (Linux; U; Android 14) gzip")];
    let v = post_json("https://music.youtube.com/youtubei/v1/browse?prettyPrint=false", &ua, &body)?;
    parse_ytm(&v)
}

fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find(x, key)),
        _ => None,
    }
}

fn parse_ytm(v: &Value) -> Option<Lyrics> {
    let data = find(v, "timedLyricsModel")?.get("lyricsData")?;
    let synced: Vec<(f64, String)> = data["timedLyricsData"]
        .as_array()?
        .iter()
        .filter_map(|l| {
            let ms: f64 = l["cueRange"]["startTimeMilliseconds"].as_str()?.parse().ok()?;
            Some((ms / 1000.0, l["lyricLine"].as_str()?.to_string()))
        })
        .collect();
    if synced.is_empty() {
        return None;
    }
    let source = data["sourceMessage"].as_str().unwrap_or_default().to_string();
    Some(Lyrics { plain: synced.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n"), synced, source, words: vec![] })
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn lrclib(artist: &str, name: &str, dur: Option<u32>) -> Option<Lyrics> {
    let mut url = format!("https://lrclib.net/api/get?artist_name={}&track_name={}", enc(artist), enc(name));
    if let Some(d) = dur {
        url += &format!("&duration={d}");
    }
    if let Some(l) = get_json(&url).as_ref().and_then(lrclib_record) {
        return Some(l);
    }
    // The exact match can be a junk upload (a lone "probe" line): any search result of the same
    // length will do, timed ones first.
    let url = format!("https://lrclib.net/api/search?artist_name={}&track_name={}", enc(artist), enc(name));
    let found = get_json(&url)?;
    let near = |r: &&Value| dur.is_none_or(|d| r["duration"].as_f64().is_some_and(|x| (x - f64::from(d)).abs() <= 3.0));
    let mut all: Vec<Lyrics> = found.as_array()?.iter().filter(near).filter_map(lrclib_record).collect();
    all.sort_by_key(|l| l.synced.is_empty());
    all.into_iter().next()
}

/// One LRCLIB entry, unless it's empty or junk (under 8 words).
fn lrclib_record(v: &Value) -> Option<Lyrics> {
    let synced = v["syncedLyrics"].as_str().map(parse_lrc).unwrap_or_default();
    let plain = v["plainLyrics"].as_str().unwrap_or_default().to_string();
    let words = plain.split_whitespace().count().max(synced.iter().map(|(_, l)| l.split_whitespace().count()).sum());
    (words >= 8).then(|| Lyrics { synced, plain, source: tr("Fonte: LRCLIB").into(), words: vec![] })
}

/// "[mm:ss.xx] line" rows; rows with several stamps repeat the line.
fn parse_lrc(s: &str) -> Vec<(f64, String)> {
    let mut out = vec![];
    for row in s.lines() {
        let mut rest = row.trim();
        let mut stamps = vec![];
        while let Some(r) = rest.strip_prefix('[') {
            let Some((tag, after)) = r.split_once(']') else { break };
            let Some((m, sec)) = tag.split_once(':') else { break };
            let (Ok(m), Ok(sec)) = (m.parse::<f64>(), sec.parse::<f64>()) else { break };
            stamps.push(m * 60.0 + sec);
            rest = after;
        }
        out.extend(stamps.into_iter().map(|t| (t, rest.trim().to_string())));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn get_json(url: &str) -> Option<Value> {
    let mut req = ehttp::Request::get(url);
    req.headers.insert("User-Agent", "ytmfast");
    let r = ehttp::fetch_blocking(&req).ok().filter(|r| r.ok)?;
    serde_json::from_slice(&r.bytes).ok()
}

/// Lowercase letters and digits only: "Never Gonna Give You Up (Remastered)" vs KuGou's spelling.
fn loose(s: &str) -> String {
    s.split(['(', '[', '-']).next().unwrap_or(s).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// KuGou: search by "artist - title" and length, take the closest length within 3 s whose names
/// match, download its KRC.
fn kugou(artist: &str, title: &str, dur: Option<u32>) -> Option<Lyrics> {
    let dur = f64::from(dur?);
    let search = format!("https://krcs.kugou.com/search?ver=1&man=yes&client=mobi&keyword={}&duration={}&hash=", enc(&format!("{artist} - {title}")), dur * 1000.0);
    let found = get_json(&search)?;
    let (want_title, want_artist) = (loose(title), loose(artist));
    // A title made only of brackets ("(Intro)") leaves nothing to match: any song would pass.
    if want_title.is_empty() {
        return None;
    }
    let num = |v: &Value| v.as_f64().or_else(|| v.as_str()?.parse().ok());
    let best = found["candidates"]
        .as_array()?
        .iter()
        .filter(|c| {
            let (song, singer) = (loose(c["song"].as_str().unwrap_or("")), loose(c["singer"].as_str().unwrap_or("")));
            !song.is_empty() && (song.contains(&want_title) || want_title.contains(&song)) && (singer.contains(&want_artist) || want_artist.contains(&singer))
        })
        .filter_map(|c| Some(((num(&c["duration"])? / 1000.0 - dur).abs(), c)))
        .filter(|(off, _)| *off <= 3.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))?
        .1;
    let id = best["id"].as_str().map(String::from).or_else(|| num(&best["id"]).map(|n| (n as u64).to_string()))?;
    let key = best["accesskey"].as_str()?;
    let download = format!("https://lyrics.kugou.com/download?ver=1&client=pc&fmt=krc&charset=utf8&id={}&accesskey={}", enc(&id), enc(key));
    let content = get_json(&download)?["content"].as_str()?.to_string();
    let mut lines = parse_krc(&krc_text(&content)?);
    // KuGou opens with credits ("Title - Artist", "Lyrics by：…", "作词：…"): not sung, dropped.
    let credit = |text: &str| {
        let low = text.trim().to_lowercase();
        let tagged = ["lyrics by", "composed by", "written by", "produced by", "arranged by", "作词", "作曲", "编曲", "制作", "词：", "曲："].iter().any(|k| low.starts_with(k));
        let alnum = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        let header = low.contains(" - ") && alnum(&low).contains(&loose(title)) && alnum(&low).contains(&loose(artist));
        tagged || header || text.contains('：')
    };
    lines.retain(|(_, w)| !credit(&w.iter().map(|x| x.2.as_str()).collect::<String>()));
    if lines.is_empty() {
        return None;
    }
    let synced: Vec<(f64, String)> = lines.iter().map(|(t, w)| (*t, w.iter().map(|x| x.2.as_str()).collect::<String>().trim().to_string())).collect();
    let plain = synced.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n");
    Some(Lyrics { synced, plain, source: tr("Fonte: KuGou").into(), words: lines.into_iter().map(|(_, w)| w).collect() })
}

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let (mut out, mut acc, mut bits) = (Vec::with_capacity(s.len() * 3 / 4), 0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// KRC download -> text: base64 of "krc1" + a zlib stream XORed with a fixed key. Capped at
/// 1 MiB inflated (real lyrics are ~20 KB; a hostile stream could ask for gigabytes).
fn krc_text(b64: &str) -> Option<String> {
    const KEY: [u8; 16] = [0x40, 0x47, 0x61, 0x77, 0x5e, 0x32, 0x74, 0x47, 0x51, 0x36, 0x31, 0x2d, 0xce, 0xd2, 0x6e, 0x69];
    let raw = base64(b64);
    if raw.len() < 8 {
        return None;
    }
    let z: Vec<u8> = raw[4..].iter().enumerate().map(|(i, b)| b ^ KEY[i % 16]).collect();
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(&z[2..], 1 << 20).ok()?;
    let text = String::from_utf8_lossy(&out);
    Some(text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_string())
}

/// KRC lines: `[start,length]<offset,length,0>text<offset,length,0>text...` (ms), into
/// (line start, words as (start, end, text)) in seconds.
fn parse_krc(text: &str) -> Vec<(f64, Vec<Word>)> {
    let mut lines = vec![];
    for raw in text.lines() {
        let Some(rest) = raw.trim().strip_prefix('[') else { continue };
        let Some((head, body)) = rest.split_once(']') else { continue };
        let Some(start) = head.split(',').next().and_then(|s| s.trim().parse::<f64>().ok()) else { continue };
        let mut words = vec![];
        for piece in body.split('<').skip(1) {
            let Some((tags, word)) = piece.split_once('>') else { continue };
            let mut n = tags.split(',').map(|s| s.trim().parse::<f64>().unwrap_or(0.0));
            let (off, len) = (n.next().unwrap_or(0.0), n.next().unwrap_or(0.0));
            if !word.is_empty() {
                let t0 = (start + off) / 1000.0;
                words.push((t0, t0 + len / 1000.0, word.to_string()));
            }
        }
        if words.iter().any(|w| !w.2.trim().is_empty()) {
            lines.push((start / 1000.0, words));
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrclib_junk_is_skipped() {
        let junk = serde_json::json!({ "syncedLyrics": "[00:00.00]probe", "plainLyrics": "probe" });
        assert!(super::lrclib_record(&junk).is_none());
        let good = serde_json::json!({ "syncedLyrics": "[00:19.67] We're no strangers to love\n[00:23.56] You know the rules and so do I", "plainLyrics": "" });
        assert_eq!(super::lrclib_record(&good).unwrap().synced.len(), 2);
    }

    #[test]
    fn lrc_and_ytm() {
        let l = parse_lrc("[ar:x]\n[00:01.50] um\n[00:10.00][01:00.25]dois\n");
        assert_eq!(l, [(1.5, "um".into()), (10.0, "dois".into()), (60.25, "dois".into())]);
        let ly = Lyrics { synced: l, ..Default::default() };
        assert_eq!(ly.line_at(0.5), None);
        assert_eq!(ly.line_at(12.0), Some(1));
        let v = json!({ "a": [{ "timedLyricsModel": { "lyricsData": { "sourceMessage": "Fonte: Musixmatch", "timedLyricsData": [
            { "lyricLine": "oi", "cueRange": { "startTimeMilliseconds": "1500" } }] } } }] });
        let y = parse_ytm(&v).unwrap();
        assert_eq!(y.synced, [(1.5, "oi".into())]);
        assert_eq!(y.source, "Fonte: Musixmatch");
        assert_eq!(enc("Tons de Marrom/é"), "Tons%20de%20Marrom%2F%C3%A9");
        // KRC: word timings relative to the line, in ms.
        let k = parse_krc("[id:1]\n[1000,2000]<0,500,0>Never <500,500,0>gonna\n");
        assert_eq!(k, [(1.0, vec![(1.0, 1.5, "Never ".to_string()), (1.5, 2.0, "gonna".to_string())])]);
        assert_eq!(base64("aGVsbG8="), b"hello");
        assert_eq!(loose("Never Gonna Give You Up (Remastered)"), "nevergonnagiveyouup");

    }
}
