//! YouTube Music's own home feed ("Ouvir de novo", "Lançamentos", mixes...), personalized when
//! logged in. rustypipe has no home endpoint, so this reads the raw `FEmusic_home` browse page.

use rustypipe::client::{ClientType, RustyPipeQuery};
use rustypipe::error::Error;
use rustypipe::model::Thumbnail;
use serde_json::{Value, json};

use crate::Act;

#[derive(Clone, Debug)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<Card>,
}

#[derive(Clone, Debug)]
pub struct Card {
    pub title: String,
    pub sub: String,
    pub thumb: Vec<Thumbnail>,
    open: Open,
}

#[derive(Clone, Debug, PartialEq)]
enum Open {
    /// Video id, and whether it's a music video (two-row cards) rather than an album track.
    Song(String, bool),
    Playlist(String),
    Album(String),
    Artist(String),
}

impl Card {
    fn radio_id(&self) -> String {
        match &self.open {
            Open::Song(v, _) => format!("RDAMVM{v}"),
            _ => String::new(),
        }
    }

    /// The song as a queue entry, from what the card shows (artist = text before the first " • ").
    fn track(&self) -> Option<rustypipe::model::TrackItem> {
        let Open::Song(id, video) = &self.open else { return None };
        let artist = self.sub.split(" • ").next().unwrap_or_default();
        serde_json::from_value(json!({
            "id": id,
            "name": self.title,
            "duration": null,
            "cover": self.thumb,
            "artists": [{ "id": null, "name": artist }],
            "artist_id": null,
            "album": null,
            "view_count": null,
            "track_type": if *video { "video" } else { "track" },
            "track_nr": null,
            "by_va": false,
        }))
        .ok()
    }

    /// Artists (and channels) are shown round, as on YouTube Music.
    pub fn round(&self) -> bool {
        matches!(self.open, Open::Artist(_))
    }

    pub(crate) fn act(&self) -> Act {
        match &self.open {
            // Plays at once; its radio fills the queue in the background.
            Open::Song(..) => match self.track() {
                Some(t) => Act::Radio(t),
                None => Act::RadioOf(self.radio_id()),
            },
            Open::Playlist(id) => Act::Playlist(id.clone(), self.title.clone()),
            Open::Album(id) => Act::Album(id.clone(), self.title.clone()),
            Open::Artist(id) => Act::Artist(id.clone()),
        }
    }
}

/// YouTube answers a session it dropped like a guest's, without an error: the only sign is
/// `logged_in = 0` among the response's tracking parameters.
pub fn answered_as_guest(v: &Value) -> bool {
    v["responseContext"]["serviceTrackingParams"].as_array().is_some_and(|services| {
        services.iter().filter_map(|s| s["params"].as_array()).flatten().any(|p| p["key"] == "logged_in" && p["value"] == "0")
    })
}

/// The home shelves, and whether YouTube answered as to a guest.
pub async fn fetch(q: &RustyPipeQuery) -> Result<(Vec<Shelf>, bool), Error> {
    let mut shelves = vec![];
    let mut page: Value = serde_json::from_str(&q.raw(ClientType::DesktopMusic, "browse", &json!({ "browseId": "FEmusic_home" })).await?).unwrap_or_default();
    let guest = answered_as_guest(&page);
    // Continuation tokens only work with the visitor id that got the first page.
    let q = q.clone().visitor_data_opt(page["responseContext"]["visitorData"].as_str());
    // ponytail: 5 continuation pages (~30 shelves), about what the website loads while scrolling
    for _ in 0..5 {
        collect(&page, &mut shelves);
        let Some(token) = find(&page, "nextContinuationData").and_then(|c| c["continuation"].as_str()).map(String::from) else { break };
        page = serde_json::from_str(&q.raw(ClientType::DesktopMusic, "browse", &json!({ "continuation": token })).await?).unwrap_or_default();
    }
    Ok((shelves, guest))
}

/// First value under `key`, depth first.
fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find(x, key)),
        _ => None,
    }
}

fn collect(v: &Value, out: &mut Vec<Shelf>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "musicCarouselShelfRenderer" || k == "musicImmersiveCarouselShelfRenderer" {
                    out.extend(shelf(x));
                } else {
                    collect(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        _ => {}
    }
}

fn runs(v: &Value) -> String {
    v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default()
}

fn thumbs(v: &Value) -> Vec<Thumbnail> {
    find(v, "thumbnails").and_then(|t| serde_json::from_value(t.clone()).ok()).unwrap_or_default()
}

fn shelf(v: &Value) -> Option<Shelf> {
    let head = find(&v["header"], "title")?;
    let title = runs(head);
    let items: Vec<Card> = v["contents"].as_array()?.iter().filter_map(card).collect();
    if title.is_empty() || items.is_empty() {
        return None;
    }
    Some(Shelf { title, items })
}

fn card(v: &Value) -> Option<Card> {
    if let Some(r) = v.get("musicTwoRowItemRenderer") {
        let nav = &r["navigationEndpoint"];
        let open = if let Some(b) = nav.get("browseEndpoint") {
            browse(b)?
        } else if let Some(w) = nav.get("watchEndpoint") {
            match (w["videoId"].as_str(), w["playlistId"].as_str()) {
                (Some(v), _) => Open::Song(v.into(), true),
                (None, Some(p)) => Open::Playlist(p.into()),
                _ => return None,
            }
        } else if let Some(p) = nav.get("watchPlaylistEndpoint").and_then(|w| w["playlistId"].as_str()) {
            Open::Playlist(p.into())
        } else {
            return None;
        };
        return Some(Card { title: runs(&r["title"]), sub: runs(&r["subtitle"]), thumb: thumbs(&r["thumbnailRenderer"]), open });
    }
    let r = v.get("musicResponsiveListItemRenderer")?;
    let col = |i: usize| &r["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"];
    let id = r["playlistItemData"]["videoId"].as_str().or_else(|| find(col(0), "videoId")?.as_str())?;
    Some(Card { title: runs(col(0)), sub: runs(col(1)), thumb: thumbs(&r["thumbnail"]), open: Open::Song(id.into(), false) })
}

fn browse(b: &Value) -> Option<Open> {
    let id = b["browseId"].as_str()?;
    let kind = find(b, "pageType").and_then(Value::as_str).unwrap_or("");
    Some(match kind {
        "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => Open::Album(id.into()),
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => Open::Artist(id.into()),
        "MUSIC_PAGE_TYPE_PLAYLIST" => Open::Playlist(id.strip_prefix("VL").unwrap_or(id).into()),
        // Moods, podcasts, charts pages: not something this app shows.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shelves() {
        let thumb = json!({ "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [{ "url": "https://x/a.jpg", "width": 60, "height": 60 }] } } });
        let page = json!({ "contents": { "sectionListRenderer": { "contents": [
            { "musicCarouselShelfRenderer": {
                "header": { "musicCarouselShelfBasicHeaderRenderer": { "title": { "runs": [{ "text": "Ouvir de novo" }] } } },
                "contents": [
                    { "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Álbum" }] }, "subtitle": { "runs": [{ "text": "Álbum" }, { "text": " • " }, { "text": "Banda" }] },
                        "thumbnailRenderer": thumb,
                        "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREb_x", "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ALBUM" } } } } } },
                    { "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Mix" }] }, "navigationEndpoint": { "watchPlaylistEndpoint": { "playlistId": "RDTMAKx" } } } },
                    { "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Humor" }] }, "navigationEndpoint": { "browseEndpoint": { "browseId": "FEmusic_moods" } } } },
                    { "musicResponsiveListItemRenderer": { "thumbnail": thumb, "playlistItemData": { "videoId": "vid1" },
                        "flexColumns": [{ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Faixa" }] } } },
                                        { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Artista" }] } } }] } }
                ] } },
            { "musicCarouselShelfRenderer": { "header": { "musicCarouselShelfBasicHeaderRenderer": { "title": { "runs": [{ "text": "Vazia" }] } } }, "contents": [] } }
        ], "continuations": [{ "nextContinuationData": { "continuation": "TOKEN" } }] } } });
        let mut out = vec![];
        collect(&page, &mut out);
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.title, "Ouvir de novo");
        assert_eq!(s.items.iter().map(|c| c.open.clone()).collect::<Vec<_>>(), [Open::Album("MPREb_x".into()), Open::Playlist("RDTMAKx".into()), Open::Song("vid1".into(), false)]);
        assert_eq!(s.items[0].sub, "Álbum • Banda");
        assert_eq!(s.items[0].thumb.len(), 1);
        assert_eq!(s.items[2].title, "Faixa");
        assert!(!s.items[0].round());
        // A song card becomes a playable track at once (not just a radio request).
        let t = s.items[2].track().expect("track from card");
        assert_eq!((t.id.as_str(), t.name.as_str(), t.artists[0].name.as_str()), ("vid1", "Faixa", "Artista"));
        assert!(matches!(s.items[2].act(), Act::Radio(_)));
        assert_eq!(find(&page, "nextContinuationData").and_then(|c| c["continuation"].as_str()), Some("TOKEN"));
        let guest = json!({ "responseContext": { "serviceTrackingParams": [{ "service": "GFEEDBACK", "params": [{ "key": "logged_in", "value": "0" }] }] } });
        assert!(answered_as_guest(&guest));
        assert!(!answered_as_guest(&json!({ "responseContext": { "serviceTrackingParams": [{ "params": [{ "key": "logged_in", "value": "1" }] }] } })));
    }
}
