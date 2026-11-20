//! Streaming sources. Every provider resolves a track to something our own audio engine can
//! decode, so visuals, lyrics sync, EQ and stats work the same for every source.

pub mod soundcloud;
pub mod spotify;
pub mod yandex;

use serde::{Deserialize, Serialize};

/// A track as a remote service describes it. Becomes a row in `tracks` with
/// `id = "<source>:<remote_id>"` and `source = <source>`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTrack {
    /// "yandex" | "soundcloud" | "spotify"
    pub source: String,
    pub remote_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    /// Direct URL of a square cover, ideally 400–1000 px.
    pub cover_url: Option<String>,
    pub isrc: Option<String>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    /// The (first) artist's id at the service, for artist pages.
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteArtist {
    pub source: String,
    pub remote_id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub genres: Vec<String>,
    /// Likes / followers, whatever the service counts.
    pub followers: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAlbum {
    pub source: String,
    pub remote_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub cover_url: Option<String>,
    pub track_count: Option<u32>,
    /// "album" | "single" | "compilation" …
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtistPage {
    pub artist: RemoteArtist,
    pub description: Option<String>,
    pub top_tracks: Vec<RemoteTrack>,
    pub albums: Vec<RemoteAlbum>,
    pub similar: Vec<RemoteArtist>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPage {
    pub album: RemoteAlbum,
    pub tracks: Vec<RemoteTrack>,
}

/// How to fetch the audio bytes of a track.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamSource {
    /// A single progressive file (mp3 / aac / m4a / flac / ogg) over HTTP(S).
    Http { url: String, headers: Vec<(String, String)>, mime: Option<String> },
    /// An HLS media playlist whose segments, concatenated, form a decodable stream
    /// (e.g. MPEG audio segments). The engine downloads segments in order.
    Hls { playlist_url: String, headers: Vec<(String, String)>, mime: Option<String> },
    /// Played by the librespot session (Spotify Premium), not over HTTP.
    Spotify { uri: String },
}

pub trait Provider: Send + Sync {
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    /// True when the provider has the credentials it needs for full-length playback.
    fn authorized(&self) -> bool;
    fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteTrack>>;
    /// The user's liked / saved tracks (requires auth). Newest first.
    fn liked(&self) -> anyhow::Result<Vec<RemoteTrack>>;
    fn stream(&self, remote_id: &str) -> anyhow::Result<StreamSource>;
    /// Lyrics as LRC (preferred, synced) or plain text. `Ok(None)` when the service has none.
    fn lyrics(&self, remote_id: &str) -> anyhow::Result<Option<String>>;
    fn search_artists(&self, _query: &str, _limit: usize) -> anyhow::Result<Vec<RemoteArtist>> {
        Ok(vec![])
    }
    fn artist(&self, _remote_id: &str) -> anyhow::Result<ArtistPage> {
        anyhow::bail!("artist pages are not supported by this source")
    }
    fn album(&self, _remote_id: &str) -> anyhow::Result<AlbumPage> {
        anyhow::bail!("album pages are not supported by this source")
    }
    /// Tracks that sound like this one (the service's own recommendation engine).
    fn similar(&self, _remote_id: &str) -> anyhow::Result<Vec<RemoteTrack>> {
        Ok(vec![])
    }
}
