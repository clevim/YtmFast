//! YouTube's "most replayed" heat for a song: drawn as a ridge over the progress bar on hover,
//! with the part people come back to. Public data from the `WEB` client's `next` (no session):
//! `macroMarkersListEntity` with 100 `MARKER_TYPE_HEATMAP` markers.

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct Heat {
    /// (middle of the marker in seconds, 0..1 lightly smoothed), earliest first.
    pub points: Vec<(f64, f32)>,
    /// Start of the most replayed part, in seconds (the very start of a song always reads hot,
    /// so it's looked for after the first 5 %).
    pub peak: Option<f64>,
    pub length: f64,
}

/// Blocking: one request of a few hundred KB.
pub fn fetch(video_id: &str) -> Option<Heat> {
    let body = json!({ "context": { "client": { "clientName": "WEB", "clientVersion": "2.20250930.01.00", "hl": "en" } }, "videoId": video_id });
    let mut req = ehttp::Request::post("https://www.youtube.com/youtubei/v1/next?prettyPrint=false", serde_json::to_vec(&body).ok()?);
    req.headers = ehttp::Headers::new(&[
        ("Content-Type", "application/json"),
        ("Origin", "https://www.youtube.com"),
        ("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36"),
    ]);
    let r = ehttp::fetch_blocking(&req).ok().filter(|r| r.ok)?;
    parse(&serde_json::from_slice(&r.bytes).ok()?)
}

fn find<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                }
                find(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| find(x, key, out)),
        _ => {}
    }
}

fn parse(v: &Value) -> Option<Heat> {
    let mut lists = vec![];
    find(v, "macroMarkersListEntity", &mut lists);
    let list = lists.into_iter().find(|l| l["markersList"]["markerType"] == "MARKER_TYPE_HEATMAP")?;
    let num = |x: &Value| x.as_f64().or_else(|| x.as_str()?.parse().ok());
    let raw: Vec<(f64, f64, f32)> = list["markersList"]["markers"]
        .as_array()?
        .iter()
        .filter_map(|m| Some((num(&m["startMillis"])? / 1000.0, num(&m["durationMillis"])? / 1000.0, num(&m["intensityScoreNormalized"])? as f32)))
        .collect();
    if raw.len() < 8 {
        return None;
    }
    let length = raw.last().map(|m| m.0 + m.1)?;
    let at = |i: isize| raw[i.clamp(0, raw.len() as isize - 1) as usize].2;
    let points: Vec<(f64, f32)> = (0..raw.len()).map(|i| (raw[i].0 + raw[i].1 / 2.0, (at(i as isize - 1) + 2.0 * at(i as isize) + at(i as isize + 1)) / 4.0)).collect();
    let hottest = raw.iter().filter(|m| m.0 >= length * 0.05).max_by(|a, b| a.2.total_cmp(&b.2)).filter(|m| m.2 > 0.0);
    let peak = hottest.map(|m| m.0);
    // Scaled to the peak after the start, so the always-hot first seconds don't flatten the rest.
    let top = hottest.map_or(1.0, |m| m.2).max(0.05);
    let points: Vec<(f64, f32)> = points.into_iter().map(|(t, v): (f64, f32)| (t, (v / top).min(1.0))).collect();
    Some(Heat { points, peak, length })
}

#[cfg(test)]
mod tests {
    #[test]
    fn heat_from_markers() {
        let markers: Vec<_> = (0..10).map(|i| serde_json::json!({ "startMillis": (i * 1000).to_string(), "durationMillis": "1000", "intensityScoreNormalized": if i == 0 { 1.0 } else if i == 6 { 0.9 } else { 0.2 } })).collect();
        let v = serde_json::json!({ "a": [{ "macroMarkersListEntity": { "markersList": { "markerType": "MARKER_TYPE_HEATMAP", "markers": markers } } }] });
        let h = super::parse(&v).unwrap();
        assert_eq!(h.points.len(), 10);
        assert_eq!(h.length, 10.0);
        // The hot start is skipped: the peak is the replayed part later on.
        assert_eq!(h.peak, Some(6.0));
    }
}
