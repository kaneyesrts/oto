//! Discord Rich Presence: "Listening to …" in the profile, with the cover, a live progress
//! bar, clickable title, and buttons to the track and to oto on GitHub.
//!
//! A background thread watches the player and only talks to Discord when something the
//! card shows changes (track, play/pause, a seek): Discord rate-limits activity updates.
//! If Discord isn't running it quietly retries every few seconds.

use crate::audio::{Audio, PlayerState};
use crate::db::{now_ms, Track};
use crate::sources::Sources;
use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// The "oto" application in the Discord Developer Portal.
pub const APP_ID: &str = "1573708833635958935";
const REPO: &str = "https://github.com/kaneyesrts/oto";
const LOGO: &str = "https://raw.githubusercontent.com/kaneyesrts/oto/main/src-tauri/icons/icon.png";

#[derive(PartialEq, Clone)]
struct Shown {
    track: String,
    playing: bool,
    /// When the track (virtually) started, for the progress bar; a seek moves it.
    start_ms: i64,
}

pub fn start(app: AppHandle) {
    if APP_ID.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new().name("oto-discord".into()).spawn(move || run(app));
}

fn run(app: AppHandle) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut last_try = Instant::now() - Duration::from_secs(60);
    let mut shown: Option<Shown> = None;
    let mut share: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    loop {
        std::thread::sleep(Duration::from_millis(1000));
        let cfg = app.state::<crate::AppState>().settings.read().discord.clone();
        let s = app.state::<Audio>().snapshot();
        let want = cfg.enabled && s.track.is_some() && (s.playing || !cfg.hide_paused);

        if !want {
            if let (Some(c), Some(_)) = (client.as_mut(), shown.as_ref()) {
                let _ = c.clear_activity();
            }
            shown = None;
            if !cfg.enabled {
                if let Some(mut c) = client.take() {
                    let _ = c.close();
                }
            }
            continue;
        }

        if client.is_none() {
            if last_try.elapsed() < Duration::from_secs(10) {
                continue;
            }
            last_try = Instant::now();
            let mut c = DiscordIpcClient::new(APP_ID);
            if c.connect().is_err() {
                continue; // Discord isn't running
            }
            client = Some(c);
            shown = None;
        }

        let t = s.track.clone().unwrap();
        let start_ms = now_ms() - s.pos_ms as i64;
        let next = Shown { track: t.id.clone(), playing: s.playing, start_ms };
        let same = shown.as_ref().is_some_and(|o| o.track == next.track && o.playing == next.playing && (!next.playing || (o.start_ms - next.start_ms).abs() < 2500));
        if same {
            continue;
        }

        let (url, cover) = share.entry(t.id.clone()).or_insert_with(|| share_info(&app, &t)).clone();
        let act = activity(&t, &s, start_ms, url.as_deref(), cover.as_deref(), cfg.buttons);
        match client.as_mut().unwrap().set_activity(act) {
            Ok(()) => shown = Some(next),
            Err(_) => {
                // Discord closed: reconnect later.
                client = None;
                shown = None;
            }
        }
    }
}

/// Public link and cover URL for a streaming track (local files have neither).
fn share_info(app: &AppHandle, t: &Track) -> (Option<String>, Option<String>) {
    if t.source == "local" || t.remote_id.is_empty() {
        return (None, None);
    }
    let cover = t.cover.clone().filter(|c| c.starts_with("https://"));
    let Some(p) = app.state::<Sources>().provider(&t.source) else { return (None, cover) };
    let album = t.album_ref.split_once(':').map(|(_, id)| id).filter(|id| !id.is_empty());
    match p.share_info(&t.remote_id, album) {
        Ok(i) => (i.url, i.cover.or(cover)),
        Err(_) => (None, cover),
    }
}

/// Discord wants 2–128 characters in text fields.
fn fit(s: &str) -> String {
    let s: String = s.trim().chars().take(128).collect();
    if s.chars().count() < 2 { format!("{s}  ") } else { s }
}

fn activity<'a>(t: &'a Track, s: &PlayerState, start_ms: i64, url: Option<&'a str>, cover: Option<&'a str>, buttons: bool) -> Activity<'a> {
    let service = match t.source.as_str() {
        "yandex" => Some("Яндекс Музыке"),
        "soundcloud" => Some("SoundCloud"),
        "spotify" => Some("Spotify"),
        _ => None,
    };
    let mut assets = Assets::new();
    match cover {
        Some(c) => {
            assets = assets
                .large_image(c)
                .large_text(fit(if t.album.trim().is_empty() { &t.title } else { &t.album }))
                .small_image(LOGO)
                .small_text(fit(&format!("oto · {}", if s.format.is_empty() { "плеер" } else { &s.format })))
                .small_url(REPO);
        }
        None => {
            assets = assets.large_image(LOGO).large_text(fit(&format!("oto · {}", if s.format.is_empty() { "плеер" } else { &s.format }))).large_url(REPO);
        }
    }
    let state = if s.playing { fit(&t.artist) } else { fit(&format!("{} · на паузе", t.artist)) };
    let mut a = Activity::new()
        .activity_type(ActivityType::Listening)
        .status_display_type(StatusDisplayType::Details)
        .details(fit(&t.title))
        .state(state)
        .assets(assets);
    if let Some(u) = url {
        a = a.details_url(u);
    }
    if s.playing && t.duration_ms > 0 {
        a = a.timestamps(Timestamps::new().start(start_ms / 1000).end((start_ms + t.duration_ms as i64) / 1000));
    }
    if buttons {
        let mut b = vec![];
        if let (Some(u), Some(svc)) = (url, service) {
            b.push(Button::new(format!("Слушать в {svc}"), u));
        }
        b.push(Button::new("oto на GitHub", REPO));
        a = a.buttons(b);
    }
    a
}
