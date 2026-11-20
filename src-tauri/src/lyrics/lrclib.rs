//! LRCLIB (lrclib.net): free, open lyrics database with synced LRC. No key required.

use crate::db::Track;
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub track_name: Option<String>,
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

static CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("oto-player/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(8))
        .build()
        .expect("http client")
});

static NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*[\(\[](?:feat\.?|ft\.?|with|prod\.?|remaster(?:ed)?|\d{4} remaster(?:ed)?|explicit|clean|official|lyrics?|audio|video|hq|hd)[^\)\]]*[\)\]]|\s+-\s+(?:\d{4}\s+)?remaster(?:ed)?.*$")
        .unwrap()
});

pub fn clean_title(t: &str) -> String {
    NOISE.replace_all(t, "").trim().to_string()
}

pub fn main_artist(a: &str) -> String {
    let lower = a.to_lowercase();
    let mut cut = a.len();
    for sep in [", ", " & ", " feat", " ft.", " x ", "; ", " / "] {
        if let Some(i) = lower.find(sep) {
            cut = cut.min(i);
        }
    }
    a[..cut].trim().to_string()
}

pub fn find(track: &Track) -> Option<Hit> {
    let title = clean_title(&track.title);
    let artist = main_artist(&track.artist);
    let secs = (track.duration_ms as f64 / 1000.0).round() as u64;

    // Exact signature lookup first: LRCLIB matches on duration ±2 s.
    let mut q: Vec<(&str, String)> = vec![("track_name", title.clone()), ("artist_name", artist.clone()), ("duration", secs.to_string())];
    if !track.album.is_empty() {
        q.push(("album_name", track.album.clone()));
    }
    if let Ok(r) = CLIENT.get("https://lrclib.net/api/get").query(&q).send() {
        if r.status().is_success() {
            if let Ok(hit) = r.json::<Hit>() {
                if hit.synced_lyrics.is_some() || hit.instrumental {
                    return Some(hit);
                }
            }
        }
    }

    // Fuzzy search: pick the closest duration, preferring synced text.
    let r = CLIENT
        .get("https://lrclib.net/api/search")
        .query(&[("track_name", title.as_str()), ("artist_name", artist.as_str())])
        .send()
        .ok()?;
    let hits: Vec<Hit> = r.json().ok()?;
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let want_title = norm(&title);
    hits.into_iter()
        .filter(|h| h.track_name.as_deref().map(|t| norm(&clean_title(t)) == want_title || norm(t).contains(&want_title)).unwrap_or(false))
        .min_by_key(|h| {
            let dd = ((h.duration.unwrap_or(0.0) - secs as f64).abs() * 10.0) as i64;
            let synced_bonus = if h.synced_lyrics.is_some() { 0 } else { 10_000 };
            dd.min(5_000) + synced_bonus
        })
        .filter(|h| h.synced_lyrics.is_some() || h.plain_lyrics.is_some() || h.instrumental)
}
