//! Streaming sources at the app level: credentials, login windows, search, library import,
//! resolving a track to a playable stream, and lyrics lookups across services.

use crate::db::{now_ms, Db, Track};
use crate::providers::{soundcloud::SoundCloud, spotify::Spotify, yandex::Yandex, Provider, RemoteTrack, StreamSource};
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
                works_anonymously: false,
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
        emit(total, total, true);
        let _ = app.emit("oto://library", ());
    });
}
