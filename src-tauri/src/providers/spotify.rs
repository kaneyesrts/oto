//! Spotify via librespot (Spotify Connect protocol; **Premium only**).
//!
//! * Login: OAuth (PKCE) in the system browser with librespot's client id; the refresh token
//!   is stored locally and exchanged for access tokens as needed.
//! * Search / liked tracks: Spotify Web API with that access token.
//! * Playback: a librespot session + player whose output sink pushes decoded PCM into a
//!   queue; `SpotifySource` drains that queue into our own engine, so EQ, the analyser,
//!   stats and lyrics sync work exactly as for every other source.

use super::{AlbumPage, ArtistPage, Provider, RemoteAlbum, RemoteArtist, RemoteTrack, ShareInfo, StreamSource};
use librespot_core::{authentication::Credentials, config::SessionConfig, session::Session, spotify_uri::SpotifyUri};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    config::PlayerConfig,
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};
use parking_lot::{Condvar, Mutex};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// librespot's public "keymaster" client id and its registered loopback redirect.
/// librespot's shared client id. Works without registering anything, but thousands of
/// clients share its Web API rate limit; users can set their own app's id instead.
pub const DEFAULT_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const SCOPES: &[&str] = &["streaming", "user-library-read", "user-read-private", "user-read-email", "playlist-read-private"];
const API: &str = "https://api.spotify.com/v1";

/// Opens the browser for login and waits for the redirect. Returns the refresh token.
pub fn login(client_id: &str) -> anyhow::Result<String> {
    let client = librespot_oauth::OAuthClientBuilder::new(client_id, REDIRECT_URI, SCOPES.to_vec())
        .open_in_browser()
        .with_custom_message("Готово. Можно вернуться в oto.")
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let tok = client.get_access_token().map_err(|e| anyhow::anyhow!("{e}"))?;
    if tok.refresh_token.is_empty() {
        anyhow::bail!("Spotify не выдал refresh token");
    }
    Ok(tok.refresh_token)
}

pub struct Spotify {
    /// The app (client id) the refresh token belongs to.
    client_id: String,
    refresh: Option<String>,
    access: Mutex<Option<(String, Instant)>>,
    backend: Mutex<Option<Arc<Backend>>>,
    http: reqwest::blocking::Client,
}

impl Spotify {
    pub fn new(refresh: Option<String>, client_id: Option<String>) -> Self {
        Self {
            client_id: client_id.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string()),
            refresh: refresh.filter(|r| !r.trim().is_empty()),
            access: Mutex::new(None),
            backend: Mutex::new(None),
            http: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(15))
                .build()
                .expect("http client"),
        }
    }

    fn token(&self) -> anyhow::Result<String> {
        if let Some((t, exp)) = self.access.lock().as_ref() {
            if Instant::now() + Duration::from_secs(60) < *exp {
                return Ok(t.clone());
            }
        }
        let refresh = self.refresh.as_ref().ok_or_else(|| anyhow::anyhow!("нужен вход в Spotify"))?;
        let client = librespot_oauth::OAuthClientBuilder::new(&self.client_id, REDIRECT_URI, SCOPES.to_vec())
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let tok = client.refresh_token(refresh).map_err(|e| anyhow::anyhow!("{e}"))?;
        *self.access.lock() = Some((tok.access_token.clone(), tok.expires_at));
        Ok(tok.access_token)
    }

    /// GET with Spotify's rate limit honoured: a short `Retry-After` is waited out and the
    /// request repeated; a long one becomes a readable error instead of a bare "HTTP 429".
    fn get(&self, url: &str) -> anyhow::Result<Value> {
        let mut attempt = 0;
        loop {
            let r = self.http.get(url).bearer_auth(self.token()?).send()?;
            let status = r.status().as_u16();
            if r.status().is_success() {
                return Ok(r.json()?);
            }
            attempt += 1;
            if status == 429 {
                let wait = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(5);
                if wait <= 30 && attempt <= 6 {
                    std::thread::sleep(Duration::from_secs(wait.max(1)));
                    continue;
                }
                let mins = wait.div_ceil(60).max(1);
                anyhow::bail!("Spotify временно ограничил запросы, попробуй примерно через {mins} мин");
            }
            if status >= 500 && attempt <= 2 {
                std::thread::sleep(Duration::from_millis(800));
                continue;
            }
            anyhow::bail!("Spotify API: HTTP {}", r.status());
        }
    }

    pub fn account_name(&self) -> anyhow::Result<String> {
        let me = self.get(&format!("{API}/me"))?;
        let product = me["product"].as_str().unwrap_or("");
        let name = me["display_name"].as_str().or(me["id"].as_str()).unwrap_or("Spotify").to_string();
        Ok(if product == "premium" { name } else { format!("{name} (нужен Premium)") })
    }

    /// The playback backend, connecting the librespot session on first use.
    pub fn backend(&self) -> anyhow::Result<Arc<Backend>> {
        if let Some(b) = self.backend.lock().as_ref() {
            if !b.session.is_invalid() {
                return Ok(b.clone());
            }
        }
        let b = Arc::new(Backend::connect(&self.token()?)?);
        *self.backend.lock() = Some(b.clone());
        Ok(b)
    }

    /// Stops Spotify playback if a session exists (used when another source takes over).
    pub fn stop(&self) {
        if let Some(b) = self.backend.lock().as_ref() {
            b.stop();
        }
    }
}

fn map_track(t: &Value) -> Option<RemoteTrack> {
    let id = t["id"].as_str()?;
    let artists: Vec<&str> = t["artists"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect();
    let album = &t["album"];
    Some(RemoteTrack {
        source: "spotify".into(),
        remote_id: id.to_string(),
        title: t["name"].as_str().unwrap_or_default().to_string(),
        artist: artists.join(", "),
        album: album["name"].as_str().unwrap_or_default().to_string(),
        duration_ms: t["duration_ms"].as_u64().unwrap_or(0),
        cover_url: album["images"].as_array().and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
        isrc: t["external_ids"]["isrc"].as_str().map(String::from),
        year: album["release_date"].as_str().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()),
        genre: None,
        artist_id: t["artists"][0]["id"].as_str().map(String::from),
        album_id: album["id"].as_str().map(String::from),
    })
}

fn map_artist(a: &Value) -> Option<RemoteArtist> {
    Some(RemoteArtist {
        source: "spotify".into(),
        remote_id: a["id"].as_str()?.to_string(),
        name: a["name"].as_str()?.to_string(),
        image_url: a["images"].as_array().and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
        genres: a["genres"].as_array().map(|g| g.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        followers: a["followers"]["total"].as_u64(),
    })
}

fn map_album(a: &Value) -> Option<RemoteAlbum> {
    Some(RemoteAlbum {
        source: "spotify".into(),
        remote_id: a["id"].as_str()?.to_string(),
        title: a["name"].as_str()?.to_string(),
        artist: a["artists"].as_array().map(|x| x.iter().filter_map(|v| v["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        year: a["release_date"].as_str().and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()),
        cover_url: a["images"].as_array().and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
        track_count: a["total_tracks"].as_u64().map(|n| n as u32),
        kind: a["album_type"].as_str().map(String::from),
    })
}

impl Provider for Spotify {
    fn id(&self) -> &'static str {
        "spotify"
    }
    fn authorized(&self) -> bool {
        self.refresh.is_some()
    }
    fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteTrack>> {
        let url = reqwest::Url::parse_with_params(
            &format!("{API}/search"),
            &[("q", query), ("type", "track"), ("limit", &limit.clamp(1, 50).to_string())],
        )?;
        let v = self.get(url.as_str())?;
        Ok(v["tracks"]["items"].as_array().map(|a| a.iter().filter_map(map_track).collect()).unwrap_or_default())
    }
    fn liked(&self) -> anyhow::Result<Vec<RemoteTrack>> {
        let mut out = vec![];
        let mut url = format!("{API}/me/tracks?limit=50");
        while out.len() < 3000 {
            let v = self.get(&url)?;
            out.extend(v["items"].as_array().into_iter().flatten().filter_map(|i| map_track(&i["track"])));
            match v["next"].as_str() {
                Some(n) => url = n.to_string(),
                None => break,
            }
            // Gentle pacing: a big library is many pages, and Spotify throttles bursts.
            std::thread::sleep(Duration::from_millis(300));
        }
        Ok(out)
    }
    fn share_info(&self, remote_id: &str, _album_id: Option<&str>) -> anyhow::Result<ShareInfo> {
        Ok(ShareInfo { url: Some(format!("https://open.spotify.com/track/{remote_id}")), cover: None })
    }

    fn stream(&self, remote_id: &str) -> anyhow::Result<StreamSource> {
        Ok(StreamSource::Spotify { uri: format!("spotify:track:{remote_id}") })
    }
    fn search_artists(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteArtist>> {
        let url = reqwest::Url::parse_with_params(&format!("{API}/search"), &[("q", query), ("type", "artist"), ("limit", &limit.clamp(1, 50).to_string())])?;
        let v = self.get(url.as_str())?;
        Ok(v["artists"]["items"].as_array().map(|a| a.iter().filter_map(map_artist).collect()).unwrap_or_default())
    }

    fn artist(&self, remote_id: &str) -> anyhow::Result<ArtistPage> {
        let a = self.get(&format!("{API}/artists/{remote_id}"))?;
        let artist = map_artist(&a).ok_or_else(|| anyhow::anyhow!("Spotify: artist not found"))?;
        let top_tracks = self
            .get(&format!("{API}/artists/{remote_id}/top-tracks"))
            .map(|v| v["tracks"].as_array().map(|t| t.iter().filter_map(map_track).collect()).unwrap_or_default())
            .unwrap_or_default();
        let albums = self
            .get(&format!("{API}/artists/{remote_id}/albums?include_groups=album,single&limit=50"))
            .map(|v| v["items"].as_array().map(|t| t.iter().filter_map(map_album).collect()).unwrap_or_default())
            .unwrap_or_default();
        Ok(ArtistPage { artist, description: None, top_tracks, albums, similar: vec![] })
    }

    fn album(&self, remote_id: &str) -> anyhow::Result<AlbumPage> {
        let a = self.get(&format!("{API}/albums/{remote_id}"))?;
        let album = map_album(&a).ok_or_else(|| anyhow::anyhow!("Spotify: album not found"))?;
        // Album track objects omit the album; give them this one so covers show.
        let tracks = a["tracks"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let mut t = t.clone();
                t["album"] = a.clone();
                map_track(&t)
            })
            .collect();
        Ok(AlbumPage { album, tracks })
    }

    fn lyrics(&self, _remote_id: &str) -> anyhow::Result<Option<String>> {
        // Spotify's lyrics come from a private endpoint; the cascade (LRCLIB, Yandex) covers it.
        Ok(None)
    }
}

// ---------------------------------------------------------------- playback

const RATE: u32 = 44_100;
/// ~1 s of stereo audio: enough to ride over network hiccups, small enough that seeks feel instant.
const CAP: usize = RATE as usize * 2;

pub struct PcmQueue {
    buf: Mutex<VecDeque<f32>>,
    cv: Condvar,
    /// Bumped on every flush (seek / new track) so a writer blocked on a full queue drops
    /// its stale packet instead of pushing old audio after the flush.
    generation: AtomicU64,
    ended: AtomicBool,
    failed: AtomicBool,
}

impl PcmQueue {
    fn flush(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.buf.lock().clear();
        self.ended.store(false, Ordering::SeqCst);
        self.failed.store(false, Ordering::SeqCst);
        self.cv.notify_all();
    }
}

struct QueueSink(Arc<PcmQueue>);

impl Sink for QueueSink {
    fn write(&mut self, packet: AudioPacket, _c: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else { return Ok(()) };
        let q = &self.0;
        let generation = q.generation.load(Ordering::SeqCst);
        let mut b = q.buf.lock();
        while b.len() + samples.len() > CAP {
            q.cv.wait_for(&mut b, Duration::from_millis(50));
            if q.generation.load(Ordering::SeqCst) != generation {
                return Ok(());
            }
        }
        b.extend(samples.iter().map(|&s| s as f32));
        Ok(())
    }
}

pub struct Backend {
    rt: tokio::runtime::Runtime,
    session: Session,
    player: Arc<Player>,
    pub queue: Arc<PcmQueue>,
}

impl Backend {
    fn connect(access_token: &str) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("oto-spotify").build()?;
        let session = rt.block_on(async {
            let s = Session::new(SessionConfig::default(), None);
            s.connect(Credentials::with_access_token(access_token), false).await.map(|_| s)
        })?;
        let queue = Arc::new(PcmQueue {
            buf: Mutex::new(VecDeque::with_capacity(CAP)),
            cv: Condvar::new(),
            generation: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        });
        let player = {
            let _guard = rt.enter();
            let q = queue.clone();
            // Premium streams up to 320 kbps; librespot defaults to 160.
            let cfg = PlayerConfig { bitrate: librespot_playback::config::Bitrate::Bitrate320, ..PlayerConfig::default() };
            Player::new(cfg, session.clone(), Box::new(NoOpVolume), move || Box::new(QueueSink(q)))
        };
        let mut events = player.get_player_event_channel();
        let q = queue.clone();
        rt.spawn(async move {
            while let Some(ev) = events.recv().await {
                match ev {
                    PlayerEvent::EndOfTrack { .. } => q.ended.store(true, Ordering::SeqCst),
                    PlayerEvent::Unavailable { .. } => {
                        q.failed.store(true, Ordering::SeqCst);
                        q.ended.store(true, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        });
        Ok(Self { rt, session, player, queue })
    }

    pub fn play(&self, uri: &str, position_ms: u32) -> anyhow::Result<()> {
        let uri = SpotifyUri::from_uri(uri).map_err(|e| anyhow::anyhow!("{e}"))?;
        self.queue.flush();
        let _g = self.rt.enter();
        self.player.load(uri, true, position_ms);
        Ok(())
    }

    pub fn seek(&self, position_ms: u32) {
        self.queue.flush();
        self.player.seek(position_ms);
    }

    pub fn stop(&self) {
        self.queue.flush();
        self.player.stop();
    }

    /// Waits until the first audio arrives (or the track turns out unavailable).
    pub fn wait_started(&self, timeout: Duration) -> anyhow::Result<()> {
        let t0 = Instant::now();
        loop {
            if !self.queue.buf.lock().is_empty() {
                return Ok(());
            }
            if self.queue.failed.load(Ordering::SeqCst) {
                anyhow::bail!("трек недоступен в Spotify (регион или подписка)");
            }
            if t0.elapsed() > timeout {
                anyhow::bail!("Spotify не начал воспроизведение");
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

/// Our engine's view of the Spotify stream.
pub struct SpotifySource {
    pub backend: Arc<Backend>,
    pub duration: Duration,
}

impl Iterator for SpotifySource {
    type Item = rodio::Sample;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let q = &self.backend.queue;
        let mut b = q.buf.lock();
        match b.pop_front() {
            Some(s) => {
                if b.len() < CAP / 2 {
                    q.cv.notify_one();
                }
                Some(s)
            }
            // Underrun: play silence rather than end; end only when librespot says so.
            None if q.ended.load(Ordering::SeqCst) => None,
            None => Some(0.0),
        }
    }
}

impl rodio::Source for SpotifySource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(self.duration)
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.backend.seek(pos.as_millis() as u32);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_web_api_track() {
        let v: Value = serde_json::from_str(
            r#"{"id":"4uLU6hMCjMI75M1A2tKUQC","name":"Never Gonna Give You Up","duration_ms":213573,
                "artists":[{"name":"Rick Astley"}],"external_ids":{"isrc":"GBARL9300135"},
                "album":{"name":"Whenever You Need Somebody","release_date":"1987-11-12","images":[{"url":"https://i.scdn.co/image/x"}]}}"#,
        )
        .unwrap();
        let t = map_track(&v).unwrap();
        assert_eq!(t.remote_id, "4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(t.artist, "Rick Astley");
        assert_eq!(t.year, Some(1987));
        assert_eq!(t.cover_url.as_deref(), Some("https://i.scdn.co/image/x"));
        assert_eq!(t.isrc.as_deref(), Some("GBARL9300135"));
    }

    #[test]
    fn stream_is_a_spotify_uri() {
        let s = Spotify::new(None);
        assert_eq!(s.stream("abc").unwrap(), StreamSource::Spotify { uri: "spotify:track:abc".into() });
        assert!(!s.authorized());
    }
}

#[cfg(test)]
mod net_tests {
    /// What Spotify answers this account right now (rate limit and how long to wait).
    #[test]
    #[ignore = "network + local account"]
    fn net_spotify_rate() {
        let creds = std::fs::read_to_string(std::env::var("APPDATA").unwrap() + "/app.oto.player/credentials.json").unwrap();
        let Some(refresh) = serde_json::from_str::<serde_json::Value>(&creds).unwrap()["spotify_refresh"].as_str().map(String::from) else {
            println!("not logged in to Spotify");
            return;
        };
        let sp = super::Spotify::new(Some(refresh), None);
        let tok = sp.token().expect("token");
        let r = sp.http.get(format!("{}/me/tracks?limit=1", super::API)).bearer_auth(tok).send().unwrap();
        println!("status {} retry-after {:?}", r.status(), r.headers().get("retry-after"));
    }
}
