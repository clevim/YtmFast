//! ListenBrainz scrobbling: "playing now" when a song starts, and a listen once it counts (the
//! same rule as the local history). Only the song's names, length and link go out.

use rustypipe::model::TrackItem;
use serde_json::{Value, json};

fn metadata(t: &TrackItem) -> Value {
    json!({
        "artist_name": crate::ui::artists(t),
        "track_name": t.name,
        "release_name": t.album.as_ref().map(|a| a.name.as_str()),
        "additional_info": {
            "media_player": "YtmFast",
            "submission_client": "YtmFast",
            "submission_client_version": env!("CARGO_PKG_VERSION"),
            "music_service": "music.youtube.com",
            "origin_url": crate::yt_url(t),
            "duration_ms": t.duration.map(|d| u64::from(d) * 1000),
        },
    })
}

fn submit(token: &str, body: Value) {
    let token = token.trim();
    if !crate::settings::valid_token(token) {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(&body) else { return };
    let mut req = ehttp::Request::post("https://api.listenbrainz.org/1/submit-listens", bytes);
    let auth = format!("Token {token}");
    req.headers = ehttp::Headers::new(&[("Content-Type", "application/json"), ("Authorization", &auth)]);
    // Fire and forget: a missed scrobble isn't worth retrying.
    ehttp::fetch(req, |_| {});
}

pub fn now_playing(token: &str, t: &TrackItem) {
    submit(token, json!({ "listen_type": "playing_now", "payload": [{ "track_metadata": metadata(t) }] }));
}

/// `at`: Unix seconds when the song started.
pub fn listen(token: &str, t: &TrackItem, at: u64) {
    submit(token, json!({ "listen_type": "single", "payload": [{ "listened_at": at, "track_metadata": metadata(t) }] }));
}
