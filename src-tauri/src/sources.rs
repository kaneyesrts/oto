//! Streaming sources at the app level: credentials, login windows, search, library import,
//! resolving a track to a playable stream, and lyrics lookups across services.

use crate::db::{now_ms, Db, Track};
use crate::providers::{soundcloud::SoundCloud, spotify::Spotify, yandex::Yandex, AlbumPage, ArtistPage, Provider, RemoteAlbum, RemoteArtist, RemoteTrack, StreamSource};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Credentials {
    pub yandex_token: Option<String>,
    pub soundcloud_oauth: Option<String>,
    pub spotify_refresh: Option<String>,
}

pub struct Sources {
    path: PathBuf,
    creds: RwLock<Credentials>,
    yandex: RwLock<Arc<Yandex>>,
    soundcloud: RwLock<Arc<SoundCloud>>,
    spotify: RwLock<Arc<Spotify>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub id: String,
    pub name: String,
    pub connected: bool,
    /// Can play without logging in (e.g. public SoundCloud tracks).
    pub works_anonymously: bool,
    pub account: Option<String>,
}

impl Sources {
    pub fn load(path: PathBuf) -> Self {
        let creds: Credentials = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self {
            yandex: RwLock::new(Arc::new(Yandex::new(creds.yandex_token.clone()))),
            soundcloud: RwLock::new(Arc::new(SoundCloud::new(creds.soundcloud_oauth.clone()))),
            spotify: RwLock::new(Arc::new(Spotify::new(creds.spotify_refresh.clone()))),
            creds: RwLock::new(creds),
            path,
        }
    }

    fn save(&self) {
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&self.path, serde_json::to_string_pretty(&*self.creds.read()).unwrap_or_default());
    }

    pub fn provider(&self, id: &str) -> Option<Arc<dyn Provider>> {
        match id {
            "yandex" => Some(self.yandex.read().clone()),
            "soundcloud" => Some(self.soundcloud.read().clone()),
            "spotify" => Some(self.spotify.read().clone()),
            _ => None,
        }
    }

    pub fn spotify(&self) -> Arc<Spotify> {
        self.spotify.read().clone()
    }

    pub fn set_spotify_refresh(&self, token: Option<String>) {
        self.spotify.read().stop();
        self.creds.write().spotify_refresh = token.clone();
        *self.spotify.write() = Arc::new(Spotify::new(token));
        self.save();
    }

    pub fn yandex(&self) -> Arc<Yandex> {
        self.yandex.read().clone()
    }

    pub fn set_yandex_token(&self, token: Option<String>) {
        self.creds.write().yandex_token = token.clone();
        *self.yandex.write() = Arc::new(Yandex::new(token));
        self.save();
    }

    pub fn set_soundcloud_oauth(&self, token: Option<String>) {
        self.creds.write().soundcloud_oauth = token.clone();
        *self.soundcloud.write() = Arc::new(SoundCloud::new(token));
        self.save();
    }

    pub fn status(&self) -> Vec<SourceStatus> {
        let c = self.creds.read().clone();
        let y = self.yandex();
        let sc = self.soundcloud.read().clone();
        let sp = self.spotify();
        vec![
            SourceStatus {
                id: "yandex".into(),
                name: "Яндекс Музыка".into(),
                connected: c.yandex_token.is_some(),
                // Search works without login; playback is then a 30 s preview.
                works_anonymously: true,
                account: c.yandex_token.as_ref().and_then(|_| y.account_name().ok()),
            },
            SourceStatus {
                id: "soundcloud".into(),
                name: "SoundCloud".into(),
                connected: c.soundcloud_oauth.is_some(),
                works_anonymously: true,
                account: c.soundcloud_oauth.as_ref().and_then(|_| sc.account_name().ok()),
            },
            SourceStatus {
                id: "spotify".into(),
                name: "Spotify".into(),
                connected: c.spotify_refresh.is_some(),
                works_anonymously: false,
                account: c.spotify_refresh.as_ref().and_then(|_| sp.account_name().ok()),
            },
        ]
    }
}

pub fn remote_id_of(source: &str, remote_id: &str) -> String {
    format!("{source}:{remote_id}")
}

/// RemoteTrack → library row (cover left as the remote URL until cached).
pub fn to_track(r: &RemoteTrack, saved: bool) -> Track {
    Track {
        id: remote_id_of(&r.source, &r.remote_id),
        path: String::new(),
        title: r.title.clone(),
        artist: r.artist.clone(),
        album: r.album.clone(),
        album_artist: r.artist.clone(),
        duration_ms: r.duration_ms,
        track_no: None,
        year: r.year,
        genre: r.genre.clone(),
        cover: r.cover_url.clone(),
        palette: vec![],
        liked: false,
        added_at: now_ms(),
        source: r.source.clone(),
        remote_id: r.remote_id.clone(),
        saved,
        alts: vec![],
        hidden: false,
        artist_ref: r.artist_id.as_ref().map(|i| format!("{}:{i}", r.source)).unwrap_or_default(),
        album_ref: r.album_id.as_ref().map(|i| format!("{}:{i}", r.source)).unwrap_or_default(),
    }
}

/// Downloads remote covers into the cover cache (and computes palettes), rewriting
/// `cover` to the local file. Remote URLs that fail stay as they are.
pub fn cache_covers(cover_dir: &std::path::Path, tracks: &mut [Track]) {
    let _ = std::fs::create_dir_all(cover_dir);
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap();
    for t in tracks.iter_mut() {
        let Some(url) = t.cover.clone().filter(|u| u.starts_with("http")) else { continue };
        let key = crate::library::track_id(&url);
        let file = cover_dir.join(format!("{key}.jpg"));
        let pal_file = cover_dir.join(format!("{key}.json"));
        if file.exists() {
            t.cover = Some(file.to_string_lossy().to_string());
            t.palette = std::fs::read_to_string(&pal_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            continue;
        }
        let Ok(bytes) = client.get(&url).send().and_then(|r| r.bytes()) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        let pal = crate::library::palette_of(&img);
        let img = if img.width() > 640 { img.resize(640, 640, image::imageops::FilterType::Lanczos3) } else { img };
        if img.to_rgb8().save_with_format(&file, image::ImageFormat::Jpeg).is_ok() {
            let _ = std::fs::write(&pal_file, serde_json::to_string(&pal).unwrap_or_default());
            t.cover = Some(file.to_string_lossy().to_string());
            t.palette = pal;
        }
    }
}

pub fn stream_for(app: &AppHandle, t: &Track) -> anyhow::Result<StreamSource> {
    let src = app.state::<Sources>();
    let p = src.provider(&t.source).ok_or_else(|| anyhow::anyhow!("источник «{}» недоступен", t.source))?;
    p.stream(&t.remote_id)
}

/// Lyrics from streaming services, for the lyrics cascade:
/// * the track's own service (same recording → no alignment needed);
/// * otherwise a Yandex search for the same song (title + artist + duration ±3 s).
///
/// Returns (text, duration_ms of the recording the text belongs to, label, same_recording).
pub fn lyrics_for(app: &AppHandle, t: &Track) -> Option<(String, Option<u64>, &'static str, bool)> {
    let src = app.state::<Sources>();
    if t.source == "yandex" {
        if let Ok(Some(text)) = src.yandex().lyrics(&t.remote_id) {
            return Some((text, Some(t.duration_ms), "Яндекс Музыка", true));
        }
    }
    let y = src.yandex();
    let q = format!("{} {}", crate::lyrics::lrclib::main_artist(&t.artist), crate::lyrics::lrclib::clean_title(&t.title));
    let hits = y.search(&q, 8).ok()?;
    let key = crate::db::match_key(&t.artist, &t.title);
    let hit = hits
        .iter()
        .filter(|h| crate::db::match_key(&h.artist, &h.title) == key)
        .min_by_key(|h| (h.duration_ms as i64 - t.duration_ms as i64).abs())
        .filter(|h| (h.duration_ms as i64 - t.duration_ms as i64).abs() <= 8000)?;
    let text = y.lyrics(&hit.remote_id).ok().flatten()?;
    Some((text, Some(hit.duration_ms), "Яндекс Музыка", false))
}

// ---------- commands ----------

#[tauri::command]
pub async fn sources_status(app: AppHandle) -> Vec<SourceStatus> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Sources>().status()).await.unwrap_or_default()
}

/// Opens the service's own login page in a webview and captures the token.
#[tauri::command]
pub async fn source_login(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("auth") {
        let _ = w.close();
    }
    match id.as_str() {
        "yandex" => {
            let app2 = app.clone();
            let url = crate::providers::yandex::OAUTH_URL.parse().map_err(|e| format!("{e}"))?;
            WebviewWindowBuilder::new(&app, "auth", WebviewUrl::External(url))
                .title("Вход в Яндекс Музыку")
                .inner_size(520.0, 720.0)
                .center()
                .on_navigation(move |u| {
                    if let Some(tok) = crate::providers::yandex::token_from_redirect(u.as_str()) {
                        let app3 = app2.clone();
                        std::thread::spawn(move || {
                            app3.state::<Sources>().set_yandex_token(Some(tok));
                            if let Some(w) = app3.get_webview_window("auth") {
                                let _ = w.close();
                            }
                            let _ = app3.emit("oto://sources", ());
                        });
                        return false;
                    }
                    true
                })
                .build()
                .map_err(|e| e.to_string())?;
        }
        "soundcloud" => {
            let url = "https://soundcloud.com/signin".parse().map_err(|e| format!("{e}"))?;
            let w = WebviewWindowBuilder::new(&app, "auth", WebviewUrl::External(url))
                .title("Вход в SoundCloud")
                .inner_size(520.0, 760.0)
                .center()
                .build()
                .map_err(|e| e.to_string())?;
            // SoundCloud keeps the session token in the `oauth_token` cookie; poll for it.
            let app2 = app.clone();
            std::thread::spawn(move || {
                let sc_url: tauri::Url = "https://soundcloud.com/".parse().unwrap();
                for _ in 0..600 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let Some(w) = app2.get_webview_window("auth") else { return };
                    if let Ok(cookies) = w.cookies_for_url(sc_url.clone()) {
                        if let Some(c) = cookies.iter().find(|c| c.name() == "oauth_token") {
                            app2.state::<Sources>().set_soundcloud_oauth(Some(c.value().to_string()));
                            let _ = w.close();
                            let _ = app2.emit("oto://sources", ());
                            return;
                        }
                    }
                }
            });
            let _ = w;
        }
        "spotify" => {
            // Browser-based OAuth with a loopback redirect; blocks until the user finishes.
            let app2 = app.clone();
            std::thread::spawn(move || match crate::providers::spotify::login() {
                Ok(refresh) => {
                    app2.state::<Sources>().set_spotify_refresh(Some(refresh));
                    let _ = app2.emit("oto://sources", ());
                }
                Err(e) => {
                    let _ = app2.emit("oto://error", format!("Вход в Spotify не удался: {e}"));
                }
            });
        }
        _ => return Err("неизвестный источник".into()),
    }
    Ok(())
}

#[tauri::command]
pub fn source_logout(app: AppHandle, id: String) {
    let s = app.state::<Sources>();
    match id.as_str() {
        "yandex" => s.set_yandex_token(None),
        "soundcloud" => s.set_soundcloud_oauth(None),
        "spotify" => s.set_spotify_refresh(None),
        _ => {}
    }
    let _ = app.emit("oto://sources", ());
}

/// Search results as Track rows (not stored until played or saved).
#[tauri::command]
pub async fn source_search(app: AppHandle, id: String, query: String) -> Result<Vec<Track>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = app.state::<Sources>().provider(&id).ok_or("неизвестный источник")?;
        let hits = p.search(&query, 30).map_err(|e| e.to_string())?;
        let db = app.state::<Db>();
        Ok(hits
            .iter()
            .map(|r| {
                let mut t = to_track(r, false);
                // Reflect library state for results already known.
                if let Some(known) = db.track(&t.id) {
                    t.saved = known.saved;
                    t.liked = known.liked;
                    if known.cover.as_deref().is_some_and(|c| !c.starts_with("http")) {
                        t.cover = known.cover;
                        t.palette = known.palette;
                    }
                }
                t
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stores tracks (e.g. search results about to be played), caching their covers.
/// `saved` puts them in the library.
#[tauri::command]
pub async fn source_store(app: AppHandle, tracks: Vec<Track>, saved: bool) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let mut tracks: Vec<Track> = tracks.into_iter().map(|mut t| { t.saved = saved; t }).collect();
        // Store immediately (playback can start), then cache covers and store again.
        app.state::<Db>().upsert_remote(&tracks);
        let _ = app.emit("oto://library", ());
        let dir = app.state::<crate::AppState>().cover_dir.clone();
        cache_covers(&dir, &mut tracks);
        app.state::<Db>().upsert_remote(&tracks);
        let _ = app.emit("oto://library", ());
        let audio = app.state::<crate::audio::Audio>();
        if let Some(cur) = audio.snapshot().track {
            audio.send(crate::audio::Cmd::RefreshTrack(cur.id));
        }
    })
    .await;
}

#[tauri::command]
pub fn track_set_saved(app: AppHandle, id: String, saved: bool) {
    app.state::<Db>().set_saved(&id, saved);
    let _ = app.emit("oto://library", ());
}

/// Imports the user's liked tracks from a service into the library, in the background.
#[tauri::command]
pub fn source_import_liked(app: AppHandle, id: String) {
    std::thread::spawn(move || {
        let emit = |done: usize, total: usize, finished: bool| {
            let _ = app.emit("oto://scan", crate::library::ScanProgress { done, total, finished });
        };
        let Some(p) = app.state::<Sources>().provider(&id) else { return };
        emit(0, 0, false);
        let liked = match p.liked() {
            Ok(v) => v,
            Err(e) => {
                let _ = app.emit("oto://error", format!("Импорт не удался: {e}"));
                emit(0, 0, true);
                return;
            }
        };
        let total = liked.len();
        let dir = app.state::<crate::AppState>().cover_dir.clone();
        // Newest like first → give them descending added_at so "new" sorting matches the service.
        let base = now_ms();
        for (n, chunk) in liked.chunks(25).enumerate() {
            let mut tracks: Vec<Track> = chunk
                .iter()
                .enumerate()
                .map(|(k, r)| {
                    let mut t = to_track(r, true);
                    t.liked = true;
                    t.added_at = base - (n * 25 + k) as i64;
                    t
                })
                .collect();
            cache_covers(&dir, &mut tracks);
            app.state::<Db>().upsert_remote(&tracks);
            emit((n * 25 + chunk.len()).min(total), total, false);
            if n % 4 == 0 {
                let _ = app.emit("oto://library", ());
            }
        }
        // Mirror the service's own order exactly in a playlist that is refreshed on every import.
        let ids: Vec<String> = liked.iter().map(|r| remote_id_of(&r.source, &r.remote_id)).collect();
        let db = app.state::<Db>();
        let key = format!("liked_playlist:{id}");
        let existing = db.meta(&key).and_then(|v| v.parse::<i64>().ok()).filter(|pid| db.playlists().iter().any(|p| p.id == *pid));
        let name = format!("Мне нравится · {}", source_name(&id));
        let pid = existing.unwrap_or_else(|| {
            let pid = db.playlist_create(&name);
            db.set_meta(&key, &pid.to_string());
            pid
        });
        db.playlist_set(pid, &ids);
        let _ = app.emit("oto://playlists", pid);
        emit(total, total, true);
        let _ = app.emit("oto://library", ());
        let _ = app.emit("oto://imported", serde_json::json!({ "source": id, "playlist": pid, "count": total }));
    });
}

pub fn source_name(id: &str) -> &'static str {
    match id {
        "yandex" => "Яндекс Музыка",
        "soundcloud" => "SoundCloud",
        "spotify" => "Spotify",
        _ => "Сервис",
    }
}

// ---------- global search, artists, albums ----------

/// One song found on one or more services.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GlobalHit {
    /// The copy to play: from the best service the user can actually stream in full.
    pub track: Track,
    /// The same song on other services.
    pub others: Vec<Track>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlobalResults {
    pub tracks: Vec<GlobalHit>,
    pub artists: Vec<RemoteArtist>,
    /// Services that failed, with the error (so the UI can say "SoundCloud недоступен").
    pub errors: Vec<(String, String)>,
}

/// Which services to query, best first. A service that would only give previews
/// (Yandex without login) ranks after ones that stream full tracks.
fn ranked_sources(s: &Sources) -> Vec<(&'static str, Arc<dyn Provider>)> {
    let c = s.creds.read().clone();
    let mut v: Vec<(u8, &'static str)> = vec![
        (if c.yandex_token.is_some() { 0 } else { 3 }, "yandex"),
        (1, "spotify"),
        (2, "soundcloud"),
    ];
    if c.spotify_refresh.is_none() {
        v.retain(|(_, id)| *id != "spotify");
    }
    v.sort();
    v.into_iter().filter_map(|(_, id)| s.provider(id).map(|p| (id, p))).collect()
}

#[tauri::command]
pub async fn search_all(app: AppHandle, query: String) -> GlobalResults {
    tauri::async_runtime::spawn_blocking(move || {
        let srcs = ranked_sources(&app.state::<Sources>());
        let q = query.trim().to_string();
        // All services in parallel; each returns tracks and artists.
        let results: Vec<(&'static str, anyhow::Result<(Vec<RemoteTrack>, Vec<RemoteArtist>)>)> = std::thread::scope(|sc| {
            let hs: Vec<_> = srcs
                .iter()
                .map(|(id, p)| {
                    let q = q.clone();
                    let p = p.clone();
                    let id = *id;
                    sc.spawn(move || (id, p.search(&q, 25).map(|t| (t, p.search_artists(&q, 8).unwrap_or_default()))))
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let mut out = GlobalResults::default();
        let db = app.state::<Db>();
        let order: Vec<&str> = srcs.iter().map(|(id, _)| *id).collect();
        let mut per_source: Vec<(usize, Vec<Track>)> = vec![];
        let mut artists: Vec<(usize, Vec<RemoteArtist>)> = vec![];
        for (id, r) in results {
            let rank = order.iter().position(|x| *x == id).unwrap_or(9);
            match r {
                Ok((tracks, arts)) => {
                    let ts = tracks
                        .iter()
                        .map(|r| {
                            let mut t = to_track(r, false);
                            if let Some(k) = db.track(&t.id) {
                                t.saved = k.saved;
                                t.liked = k.liked;
                                if k.cover.as_deref().is_some_and(|c| !c.starts_with("http")) {
                                    t.cover = k.cover;
                                    t.palette = k.palette;
                                }
                            }
                            t
                        })
                        .collect();
                    per_source.push((rank, ts));
                    artists.push((rank, arts));
                }
                Err(e) => out.errors.push((id.to_string(), e.to_string())),
            }
        }
        per_source.sort_by_key(|(r, _)| *r);
        // Merge: walk services best-first; a hit from a later service joins an existing
        // song when artist+title match and durations are within 3 s.
        for (_, list) in per_source {
            for t in list {
                let key = crate::db::match_key(&t.artist, &t.title);
                let found = out.tracks.iter_mut().find(|h| {
                    crate::db::match_key(&h.track.artist, &h.track.title) == key
                        && (h.track.duration_ms as i64 - t.duration_ms as i64).abs() <= 3000
                });
                match found {
                    Some(h) => {
                        if !h.others.iter().any(|o| o.source == t.source) && h.track.source != t.source {
                            h.others.push(t);
                        }
                    }
                    None => out.tracks.push(GlobalHit { track: t, others: vec![] }),
                }
            }
        }
        // Keep the services' relevance: interleave nothing, but float songs found on
        // several services (a strong signal that it is "the" song) slightly up.
        let n = out.tracks.len();
        let mut scored: Vec<(f32, GlobalHit)> = out.tracks.drain(..).enumerate().map(|(i, h)| (i as f32 / n.max(1) as f32 - h.others.len() as f32 * 0.08, h)).collect();
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        out.tracks = scored.into_iter().map(|(_, h)| h).take(40).collect();
        artists.sort_by_key(|(r, _)| *r);
        for (_, list) in artists {
            for a in list {
                let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
                if !out.artists.iter().any(|x| norm(&x.name) == norm(&a.name)) {
                    out.artists.push(a);
                }
            }
        }
        out.artists.truncate(12);
        out
    })
    .await
    .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistView {
    pub artist: RemoteArtist,
    pub description: Option<String>,
    pub top_tracks: Vec<Track>,
    pub albums: Vec<RemoteAlbum>,
    pub similar: Vec<RemoteArtist>,
}

fn artist_view(p: ArtistPage, db: &Db) -> ArtistView {
    ArtistView {
        top_tracks: p
            .top_tracks
            .iter()
            .map(|r| {
                let mut t = to_track(r, false);
                if let Some(k) = db.track(&t.id) {
                    t.saved = k.saved;
                    t.liked = k.liked;
                }
                t
            })
            .collect(),
        artist: p.artist,
        description: p.description,
        albums: p.albums,
        similar: p.similar,
    }
}

/// Artist page from a service: `reference` is "<source>:<id>".
#[tauri::command]
pub async fn artist_page(app: AppHandle, reference: String) -> Result<ArtistView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (src, id) = reference.split_once(':').ok_or("bad reference")?;
        let p = app.state::<Sources>().provider(src).ok_or("unknown source")?;
        let page = p.artist(id).map_err(|e| e.to_string())?;
        Ok(artist_view(page, &app.state::<Db>()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumView {
    pub album: RemoteAlbum,
    pub tracks: Vec<Track>,
}

#[tauri::command]
pub async fn album_page(app: AppHandle, reference: String) -> Result<AlbumView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (src, id) = reference.split_once(':').ok_or("bad reference")?;
        let p = app.state::<Sources>().provider(src).ok_or("unknown source")?;
        let AlbumPage { album, tracks } = p.album(id).map_err(|e| e.to_string())?;
        let db = app.state::<Db>();
        let tracks = tracks
            .iter()
            .map(|r| {
                let mut t = to_track(r, false);
                if t.cover.is_none() {
                    t.cover = album.cover_url.clone();
                }
                if let Some(k) = db.track(&t.id) {
                    t.saved = k.saved;
                    t.liked = k.liked;
                }
                t
            })
            .collect();
        Ok(AlbumView { album, tracks })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn norm_name(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Finds an artist by name on the services (Yandex first: richest pages, works without login).
fn lookup_artist(app: &AppHandle, name: &str) -> Option<RemoteArtist> {
    let want = norm_name(name);
    for (_, p) in ranked_sources(&app.state::<Sources>()) {
        if let Ok(list) = p.search_artists(name, 5) {
            if let Some(a) = list.into_iter().find(|a| norm_name(&a.name) == want) {
                return Some(a);
            }
        }
    }
    None
}

/// "<source>:<id>" for a library artist name, looked up once and cached.
#[tauri::command]
pub async fn artist_ref(app: AppHandle, name: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Db>();
        if let Some((_, r, _)) = db.artist_cached(&name) {
            if !r.is_empty() {
                return Some(r);
            }
        }
        let a = lookup_artist(&app, &name)?;
        let r = format!("{}:{}", a.source, a.remote_id);
        let dir = app.state::<crate::AppState>().cover_dir.clone();
        let img = a.image_url.as_deref().and_then(|u| cache_image(&dir, u));
        db.cache_artist(&name, img.as_deref(), &r);
        Some(r)
    })
    .await
    .ok()
    .flatten()
}

fn cache_image(dir: &std::path::Path, url: &str) -> Option<String> {
    let mut t = Track { cover: Some(url.to_string()), ..Default::default() };
    cache_covers(dir, std::slice::from_mut(&mut t));
    t.cover.filter(|c| !c.starts_with("http"))
}

/// Photos for library artists: returns what is cached now and fetches the rest in the
/// background (emitting `oto://artist-images` as they arrive). Misses are retried weekly.
#[tauri::command]
pub fn artist_images(app: AppHandle, names: Vec<String>) -> std::collections::HashMap<String, String> {
    let db = app.state::<Db>();
    let mut have = std::collections::HashMap::new();
    let mut missing = vec![];
    let week = 7 * 24 * 3600 * 1000;
    for n in names {
        match db.artist_cached(&n) {
            Some((Some(img), _, _)) => {
                have.insert(n, img);
            }
            Some((None, _, at)) if now_ms() - at < week => {}
            _ => missing.push(n),
        }
    }
    if !missing.is_empty() {
        let app2 = app.clone();
        std::thread::spawn(move || {
            let dir = app2.state::<crate::AppState>().cover_dir.clone();
            for chunk in missing.chunks(6) {
                let found: Vec<(String, Option<String>, String)> = std::thread::scope(|sc| {
                    let hs: Vec<_> = chunk
                        .iter()
                        .map(|n| {
                            let app3 = app2.clone();
                            let dir = dir.clone();
                            sc.spawn(move || {
                                let a = lookup_artist(&app3, n);
                                let img = a.as_ref().and_then(|a| a.image_url.as_deref()).and_then(|u| cache_image(&dir, u));
                                let r = a.map(|a| format!("{}:{}", a.source, a.remote_id)).unwrap_or_default();
                                (n.clone(), img, r)
                            })
                        })
                        .collect();
                    hs.into_iter().filter_map(|h| h.join().ok()).collect()
                });
                let db = app2.state::<Db>();
                let mut update = serde_json::Map::new();
                for (n, img, r) in found {
                    db.cache_artist(&n, img.as_deref(), &r);
                    if let Some(i) = img {
                        update.insert(n, serde_json::Value::String(i));
                    }
                }
                if !update.is_empty() {
                    let _ = app2.emit("oto://artist-images", update);
                }
            }
        });
    }
    have
}

#[cfg(test)]
mod tests {
    use crate::providers::{soundcloud::SoundCloud, yandex::Yandex, Provider};

    #[test]
    #[ignore = "network"]
    fn net_artist_and_album_pages() {
        let y = Yandex::new(None);
        let a = y.search_artists("Кино", 3).unwrap();
        println!("yandex artists: {:?}", a.iter().map(|x| (&x.name, &x.remote_id, x.image_url.is_some(), x.followers)).collect::<Vec<_>>());
        let page = y.artist(&a[0].remote_id).unwrap();
        println!("top {} / albums {} / similar {} / desc {}", page.top_tracks.len(), page.albums.len(), page.similar.len(), page.description.is_some());
        assert!(!page.top_tracks.is_empty() && !page.albums.is_empty());
        let al = y.album(&page.albums[0].remote_id).unwrap();
        println!("album '{}' {} tracks", al.album.title, al.tracks.len());
        assert!(!al.tracks.is_empty());

        let sc = SoundCloud::new(None);
        let u = sc.search_artists("odesza", 3).unwrap();
        println!("sc users: {:?}", u.iter().map(|x| (&x.name, x.followers)).collect::<Vec<_>>());
        let p = sc.artist(&u[0].remote_id).unwrap();
        println!("sc top {} albums {}", p.top_tracks.len(), p.albums.len());
        assert!(!p.top_tracks.is_empty());
    }
}
