mod audio;
mod db;
mod library;
mod lyrics;
mod settings;
mod shell;
mod stats;

use audio::{Audio, Cmd, PlayerState};
use db::{Db, Track};
use parking_lot::RwLock;
use serde::Deserialize;
use settings::Settings;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    pub cover_dir: PathBuf,
}

// ---------- settings ----------

#[tauri::command]
fn get_settings(st: State<AppState>) -> Settings {
    st.settings.read().clone()
}

/// Returns hotkey registration errors, if any, so the UI can flag the offending field.
// Async on purpose: these may create windows (the widget), and creating a window from a
// synchronous command deadlocks on Windows.
#[tauri::command]
async fn set_settings(app: AppHandle, settings: Settings) -> Vec<String> {
    let st = app.state::<AppState>();
    let old = {
        let mut s = st.settings.write();
        let old = s.clone();
        let (x, y) = (s.widget.x, s.widget.y);
        *s = settings.clone();
        // The widget's position is owned by the window, not the settings form.
        s.widget.x = x;
        s.widget.y = y;
        old
    };
    let new = st.settings.read().clone();
    shell::apply_settings(&app, &old, &new)
}

// ---------- library ----------

#[tauri::command]
fn get_tracks(db: State<Db>) -> Vec<Track> {
    db.all_tracks()
}

fn spawn_scan(app: AppHandle, roots: Vec<String>) {
    std::thread::spawn(move || {
        let cover_dir = app.state::<AppState>().cover_dir.clone();
        let db = app.state::<Db>();
        let app2 = app.clone();
        library::scan(&db, &roots, &cover_dir, |p| {
            let _ = app2.emit("oto://scan", p);
        });
        let _ = app.emit("oto://library", ());
    });
}

#[tauri::command]
async fn add_folder(app: AppHandle, path: String) {
    shell::update_settings(&app, |s| {
        if !s.folders.contains(&path) {
            s.folders.push(path.clone());
        }
    });
    spawn_scan(app, vec![path]);
}

#[tauri::command]
async fn remove_folder(app: AppHandle, path: String) {
    shell::update_settings(&app, |s| s.folders.retain(|f| f != &path));
    app.state::<Db>().remove_under(&path);
    let _ = app.emit("oto://library", ());
}

#[tauri::command]
fn rescan(app: AppHandle, st: State<AppState>) {
    let roots = st.settings.read().folders.clone();
    spawn_scan(app, roots);
}

#[tauri::command]
fn toggle_like(app: AppHandle, id: String) -> bool {
    shell::toggle_like(&app, &id)
}

// ---------- player ----------

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum PlayerReq {
    PlayList { ids: Vec<String>, start: usize },
    Enqueue { ids: Vec<String> },
    PlayNext { ids: Vec<String> },
    JumpTo { index: usize },
    Toggle,
    Play,
    Pause,
    Next,
    Prev,
    Seek { ms: u64 },
    SeekBy { ms: i64 },
    Volume { value: f32 },
    Shuffle { on: bool },
    Repeat { mode: String },
}

#[tauri::command]
fn player(audio: State<Audio>, req: PlayerReq) {
    audio.send(match req {
        PlayerReq::PlayList { ids, start } => Cmd::PlayList { ids, start },
        PlayerReq::Enqueue { ids } => Cmd::Enqueue(ids),
        PlayerReq::PlayNext { ids } => Cmd::PlayNext(ids),
        PlayerReq::JumpTo { index } => Cmd::JumpTo(index),
        PlayerReq::Toggle => Cmd::Toggle,
        PlayerReq::Play => Cmd::Play,
        PlayerReq::Pause => Cmd::Pause,
        PlayerReq::Next => Cmd::Next,
        PlayerReq::Prev => Cmd::Prev,
        PlayerReq::Seek { ms } => Cmd::Seek(ms),
        PlayerReq::SeekBy { ms } => Cmd::SeekBy(ms),
        PlayerReq::Volume { value } => Cmd::Volume(value),
        PlayerReq::Shuffle { on } => Cmd::Shuffle(on),
        PlayerReq::Repeat { mode } => Cmd::Repeat(mode),
    });
}

#[tauri::command]
fn player_state(audio: State<Audio>) -> PlayerState {
    audio.snapshot()
}

// ---------- lyrics & stats ----------

#[tauri::command]
async fn get_lyrics(app: AppHandle, id: String, force: Option<bool>) -> Option<lyrics::LyricsDoc> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Db>();
        let track = db.track(&id)?;
        let (online, align) = {
            let s = app.state::<AppState>();
            let s = s.settings.read();
            (s.lyrics.online, s.lyrics.auto_align)
        };
        if force.unwrap_or(false) {
            db.drop_lyrics_cache(&id);
        }
        Some(lyrics::resolve(&db, &track, online, align, force.unwrap_or(false)))
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
fn set_lyrics_offset(db: State<Db>, id: String, ms: i32) {
    db.set_lyrics_offset(&id, ms);
}

#[tauri::command]
async fn get_stats(app: AppHandle, range: String) -> stats::Stats {
    tauri::async_runtime::spawn_blocking(move || stats::compute(&app.state::<Db>(), &range)).await.unwrap_or_default()
}

// ---------- windows ----------

#[tauri::command]
fn show_main(app: AppHandle) {
    shell::show_main(&app);
}

#[tauri::command]
async fn toggle_widget(app: AppHandle) {
    shell::dispatch(&app, shell::Action::ToggleWidget);
}

#[tauri::command]
fn hide_flyout(app: AppHandle) {
    if let Some(w) = app.get_webview_window("flyout") {
        let _ = w.hide();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| shell::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(shell::hotkey_plugin())
        .setup(|app| {
            let h = app.handle().clone();
            let cfg = app.path().app_config_dir()?;
            let data = app.path().app_data_dir()?;
            let cache = app.path().app_cache_dir()?;
            let settings_path = cfg.join("settings.json");
            let s = settings::load(&settings_path);
            let volume = s.volume;
            let folders = s.folders.clone();

            app.manage(Db::open(&data.join("library.db"))?);
            app.manage(AppState { settings: RwLock::new(s.clone()), settings_path, cover_dir: cache.join("covers") });
            app.manage(shell::Shell::default());
            app.manage(audio::start(h.clone(), volume));

            shell::setup_tray(&h)?;
            shell::create_flyout(&h)?;
            shell::setup_smtc(&h);
            let _ = shell::register_hotkeys(&h, &s);
            shell::apply_widget(&h, &s);

            if let Some(main) = app.get_webview_window("main") {
                let h2 = h.clone();
                main.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        if h2.state::<AppState>().settings.read().close_to_tray {
                            api.prevent_close();
                            if let Some(w) = h2.get_webview_window("main") {
                                let _ = w.hide();
                            }
                        } else {
                            shell::persist(&h2);
                            h2.exit(0);
                        }
                    }
                });
            }
            // Pick up files changed while the app was closed.
            if !folders.is_empty() {
                spawn_scan(h, folders);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            get_tracks,
            add_folder,
            remove_folder,
            rescan,
            toggle_like,
            player,
            player_state,
            get_lyrics,
            set_lyrics_offset,
            get_stats,
            show_main,
            hide_flyout,
            toggle_widget,
        ])
        .build(tauri::generate_context!())
        .expect("error while building oto")
        .run(|app, e| {
            if let tauri::RunEvent::Exit = e {
                shell::persist(app);
            }
        });
}
