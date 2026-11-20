//! Everything that lives outside the main window: tray + flyout, the desktop/overlay widget,
//! global hotkeys and Windows media controls (SMTC).

use crate::audio::{Audio, Cmd, PlayerState};
use crate::db::Db;
use crate::settings::{self, Settings};
use crate::AppState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    TogglePlay,
    Next,
    Prev,
    VolumeUp,
    VolumeDown,
    Like,
    ToggleWidget,
    ToggleClickThrough,
    SeekForward,
    SeekBack,
    ShowMain,
    Quit,
}

pub struct Shell {
    pub hotkeys: Mutex<HashMap<u32, Action>>,
    tray_now: Mutex<Option<MenuItem<tauri::Wry>>>,
    tray_toggle: Mutex<Option<MenuItem<tauri::Wry>>>,
    tray_like: Mutex<Option<MenuItem<tauri::Wry>>>,
    last: Mutex<(Option<String>, bool, bool)>,
    flyout_hidden_at: Mutex<Option<Instant>>,
    smtc: Mutex<Option<Smtc>>,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            hotkeys: Mutex::new(HashMap::new()),
            tray_now: Mutex::new(None),
            tray_toggle: Mutex::new(None),
            tray_like: Mutex::new(None),
            last: Mutex::new((None, false, false)),
            flyout_hidden_at: Mutex::new(None),
            smtc: Mutex::new(None),
        }
    }
}

// SMTC objects are WinRT agile objects; they are safe to use from the audio thread.
pub struct Smtc(souvlaki::MediaControls);
unsafe impl Send for Smtc {}

// ---------- actions ----------

pub fn dispatch(app: &AppHandle, action: Action) {
    let audio = app.state::<Audio>();
    match action {
        Action::TogglePlay => audio.send(Cmd::Toggle),
        Action::Next => audio.send(Cmd::Next),
        Action::Prev => audio.send(Cmd::Prev),
        Action::VolumeUp => audio.send(Cmd::VolumeDelta(0.05)),
        Action::VolumeDown => audio.send(Cmd::VolumeDelta(-0.05)),
        Action::Like => {
            if let Some(t) = audio.snapshot().track {
                toggle_like(app, &t.id);
            }
        }
        Action::ToggleWidget => update_settings(app, |s| s.widget.enabled = !s.widget.enabled),
        Action::ToggleClickThrough => update_settings(app, |s| s.widget.click_through = !s.widget.click_through),
        Action::SeekForward => audio.send(Cmd::SeekBy(10_000)),
        Action::SeekBack => audio.send(Cmd::SeekBy(-10_000)),
        Action::ShowMain => show_main(app),
        Action::Quit => {
            persist(app);
            app.exit(0);
        }
    }
}

pub fn toggle_like(app: &AppHandle, id: &str) -> bool {
    let db = app.state::<Db>();
    let liked = !db.track(id).map(|t| t.liked).unwrap_or(false);
    db.set_liked(id, liked);
    app.state::<Audio>().send(Cmd::RefreshTrack(id.to_string()));
    let _ = app.emit("oto://liked", serde_json::json!({ "id": id, "liked": liked }));
    liked
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Applies a change to settings, persists it, and pushes it to every window.
pub fn update_settings(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let st = app.state::<AppState>();
    let (old, new) = {
        let mut s = st.settings.write();
        let old = s.clone();
        f(&mut s);
        (old, s.clone())
    };
    apply_settings(app, &old, &new);
}

pub fn apply_settings(app: &AppHandle, old: &Settings, new: &Settings) -> Vec<String> {
    let st = app.state::<AppState>();
    settings::save(&st.settings_path, new);
    let mut errors = vec![];
    if old.hotkeys != new.hotkeys || app.state::<Shell>().hotkeys.lock().is_empty() {
        errors = register_hotkeys(app, new);
    }
    apply_widget(app, new);
    let _ = app.emit("oto://settings", new);
    errors
}

pub fn persist(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut s = st.settings.write();
    s.volume = app.state::<Audio>().snapshot().volume;
    settings::save(&st.settings_path, &s);
}

// ---------- hotkeys ----------

pub fn register_hotkeys(app: &AppHandle, s: &Settings) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let shell = app.state::<Shell>();
    let mut map = shell.hotkeys.lock();
    map.clear();
    let mut errors = vec![];
    for (action, combo) in s.hotkeys.entries() {
        if combo.trim().is_empty() {
            continue;
        }
        match combo.parse::<Shortcut>() {
            Ok(sc) => match gs.register(sc) {
                Ok(_) => {
                    map.insert(sc.id(), action);
                }
                Err(e) => errors.push(format!("{combo}: {e}")),
            },
            Err(e) => errors.push(format!("{combo}: {e}")),
        }
    }
    errors
}

pub fn hotkey_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, sc, ev| {
            if ev.state() != ShortcutState::Pressed {
                return;
            }
            let action = app.state::<Shell>().hotkeys.lock().get(&sc.id()).copied();
            if let Some(a) = action {
                dispatch(app, a);
            }
        })
        .build()
}

// ---------- tray ----------

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let now = MenuItem::with_id(app, "now", "Ничего не играет", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Играть", true, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", "Предыдущий", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Следующий", true, None::<&str>)?;
    let like = MenuItem::with_id(app, "like", "В избранное", true, None::<&str>)?;
    let widget = MenuItem::with_id(app, "widget", "Виджет вкл/выкл", true, None::<&str>)?;
    let ct = MenuItem::with_id(app, "clickthrough", "Виджет: сквозь клики", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Открыть oto", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(app, &[&now, &sep()?, &toggle, &prev, &next, &like, &sep()?, &widget, &ct, &sep()?, &open, &quit])?;

    let shell = app.state::<Shell>();
    *shell.tray_now.lock() = Some(now);
    *shell.tray_toggle.lock() = Some(toggle);
    *shell.tray_like.lock() = Some(like);

    TrayIconBuilder::with_id("oto")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("oto")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let a = match ev.id().as_ref() {
                "toggle" => Action::TogglePlay,
                "prev" => Action::Prev,
                "next" => Action::Next,
                "like" => Action::Like,
                "widget" => Action::ToggleWidget,
                "clickthrough" => Action::ToggleClickThrough,
                "open" => Action::ShowMain,
                "quit" => Action::Quit,
                _ => return,
            };
            dispatch(app, a);
        })
        .on_tray_icon_event(|tray, ev| match ev {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } => {
                toggle_flyout(tray.app_handle(), rect);
            }
            TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => show_main(tray.app_handle()),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn create_flyout(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "flyout", WebviewUrl::App("index.html".into()))
        .title("oto")
        .inner_size(340.0, 432.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .focused(false)
        .build()?;
    let app2 = app.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::Focused(false) = e {
            if let Some(f) = app2.get_webview_window("flyout") {
                let _ = f.hide();
                *app2.state::<Shell>().flyout_hidden_at.lock() = Some(Instant::now());
            }
        }
    });
    Ok(())
}

fn toggle_flyout(app: &AppHandle, rect: tauri::Rect) {
    let Some(w) = app.get_webview_window("flyout") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    // Clicking the icon while the flyout is open first blurs it (hiding it); don't reopen.
    if let Some(t) = *app.state::<Shell>().flyout_hidden_at.lock() {
        if t.elapsed() < Duration::from_millis(250) {
            return;
        }
    }
    let scale = w.scale_factor().unwrap_or(1.0);
    let pos: PhysicalPosition<f64> = rect.position.to_physical(scale);
    let size: tauri::PhysicalSize<f64> = rect.size.to_physical(scale);
    let ws = w.outer_size().unwrap_or_default();
    let (fw, fh) = (ws.width as f64, ws.height as f64);
    let margin = 12.0 * scale;
    let mut x = pos.x + size.width / 2.0 - fw / 2.0;
    let mut y = pos.y - fh - margin;
    if let Ok(Some(m)) = w.current_monitor().or_else(|_| app.primary_monitor()) {
        let wa = m.work_area();
        let (l, t) = (wa.position.x as f64, wa.position.y as f64);
        let (r, b) = (l + wa.size.width as f64, t + wa.size.height as f64);
        if y < t {
            y = pos.y + size.height + margin; // taskbar at the top
        }
        x = x.clamp(l + margin, r - fw - margin);
        y = y.clamp(t + margin, b - fh - margin);
    }
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to("flyout", "oto://flyout-open", ());
}

// ---------- widget ----------

fn widget_size(s: &Settings) -> (f64, f64) {
    // Card size, plus a transparent margin wide enough for the beat glow: if the glow is
    // larger than the margin, the window edge cuts it into a visible rectangle.
    const MARGIN: f64 = 2.0 * 26.0;
    let (w, h) = match s.widget.style.as_str() {
        "lyrics" => (536.0, 126.0),
        "spectrum" => (356.0, 146.0),
        _ => (356.0, 100.0),
    };
    let k = s.widget.scale as f64;
    (w * k + MARGIN, h * k + MARGIN)
}

pub fn apply_widget(app: &AppHandle, s: &Settings) {
    let existing = app.get_webview_window("widget");
    if !s.widget.enabled {
        if let Some(w) = existing {
            let _ = w.close();
        }
        return;
    }
    let (w, h) = widget_size(s);
    let overlay = s.widget.layer == "overlay";
    let win = match existing {
        Some(win) => {
            let _ = win.set_size(tauri::LogicalSize::new(w, h));
            win
        }
        None => {
            // Default spot: bottom-right of the primary work area, computed before build;
            // moving a transparent window right after creation is unreliable on Windows.
            let (x, y) = match (s.widget.x, s.widget.y) {
                (Some(x), Some(y)) => (x as f64, y as f64),
                _ => match app.primary_monitor() {
                    Ok(Some(m)) => {
                        let wa = m.work_area();
                        let sc = m.scale_factor();
                        (
                            (wa.position.x as f64 + wa.size.width as f64) / sc - w - 24.0,
                            (wa.position.y as f64 + wa.size.height as f64) / sc - h - 24.0,
                        )
                    }
                    _ => (80.0, 80.0),
                },
            };
            let b = WebviewWindowBuilder::new(app, "widget", WebviewUrl::App("index.html".into()))
                .title("oto widget")
                .inner_size(w, h)
                .position(x, y)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .skip_taskbar(true)
                .always_on_top(overlay)
                .focused(false);
            let win = match b.build() {
                Ok(w) => w,
                Err(e) => {
                    let _ = app.emit("oto://error", format!("Виджет не открылся: {e}"));
                    return;
                }
            };
            let app2 = app.clone();
            win.on_window_event(move |e| {
                if let WindowEvent::Moved(p) = e {
                    if let Some(w) = app2.get_webview_window("widget") {
                        let sc = w.scale_factor().unwrap_or(1.0);
                        let state = app2.state::<AppState>();
                        let mut st = state.settings.write();
                        st.widget.x = Some((p.x as f64 / sc) as i32);
                        st.widget.y = Some((p.y as f64 / sc) as i32);
                    }
                }
            });
            win
        }
    };
    let _ = win.set_always_on_top(overlay);
    let _ = win.set_always_on_bottom(!overlay);
    let _ = win.set_ignore_cursor_events(s.widget.click_through);
    let _ = win.show();
}

// ---------- state reflection: tray, SMTC ----------

pub fn setup_smtc(app: &AppHandle) {
    #[cfg(windows)]
    {
        let Some(main) = app.get_webview_window("main") else { return };
        let Ok(hwnd) = main.hwnd() else { return };
        let cfg = souvlaki::PlatformConfig { display_name: "oto", dbus_name: "oto", hwnd: Some(hwnd.0 as *mut std::ffi::c_void) };
        let Ok(mut mc) = souvlaki::MediaControls::new(cfg) else { return };
        let app2 = app.clone();
        let _ = mc.attach(move |e| {
            use souvlaki::MediaControlEvent as E;
            let audio = app2.state::<Audio>();
            match e {
                E::Play => audio.send(Cmd::Play),
                E::Pause => audio.send(Cmd::Pause),
                E::Toggle => audio.send(Cmd::Toggle),
                E::Next => audio.send(Cmd::Next),
                E::Previous => audio.send(Cmd::Prev),
                E::Stop => audio.send(Cmd::Pause),
                E::SetPosition(p) => audio.send(Cmd::Seek(p.0.as_millis() as u64)),
                E::SetVolume(v) => audio.send(Cmd::Volume(v as f32)),
                E::Raise => show_main(&app2),
                _ => {}
            }
        });
        *app.state::<Shell>().smtc.lock() = Some(Smtc(mc));
    }
}

pub fn on_player_state(app: &AppHandle, s: &PlayerState) {
    let shell = app.state::<Shell>();
    let id = s.track.as_ref().map(|t| t.id.clone());
    let liked = s.track.as_ref().map(|t| t.liked).unwrap_or(false);
    {
        let mut last = shell.last.lock();
        let changed_track = last.0 != id;
        if !changed_track && last.1 == s.playing && last.2 == liked {
            // Only progress changed: keep SMTC's timeline in sync, skip the rest.
            if let Some(m) = shell.smtc.lock().as_mut() {
                let p = Some(souvlaki::MediaPosition(Duration::from_millis(s.pos_ms)));
                let _ = m.0.set_playback(if s.playing { souvlaki::MediaPlayback::Playing { progress: p } } else { souvlaki::MediaPlayback::Paused { progress: p } });
            }
            return;
        }
        *last = (id, s.playing, liked);
        if changed_track {
            if let (Some(t), Some(m)) = (&s.track, shell.smtc.lock().as_mut()) {
                let cover = t.cover.as_ref().map(|c| format!("file:///{}", c.replace('\\', "/")));
                let _ = m.0.set_metadata(souvlaki::MediaMetadata {
                    title: Some(&t.title),
                    artist: Some(&t.artist),
                    album: Some(&t.album),
                    cover_url: cover.as_deref(),
                    duration: Some(Duration::from_millis(t.duration_ms)),
                });
            }
        }
    }
    if let Some(m) = shell.smtc.lock().as_mut() {
        let p = Some(souvlaki::MediaPosition(Duration::from_millis(s.pos_ms)));
        let _ = m.0.set_playback(if s.playing { souvlaki::MediaPlayback::Playing { progress: p } } else { souvlaki::MediaPlayback::Paused { progress: p } });
    }

    let label = s.track.as_ref().map(|t| format!("{} — {}", t.artist, t.title));
    if let Some(tray) = app.tray_by_id("oto") {
        let tip = match &label {
            Some(l) => format!("{}{}", if s.playing { "▶ " } else { "⏸ " }, truncate(l, 110)),
            None => "oto".into(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
    if let Some(i) = shell.tray_now.lock().as_ref() {
        let _ = i.set_text(label.as_deref().map(|l| truncate(l, 48)).unwrap_or_else(|| "Ничего не играет".into()));
    }
    if let Some(i) = shell.tray_toggle.lock().as_ref() {
        let _ = i.set_text(if s.playing { "Пауза" } else { "Играть" });
    }
    if let Some(i) = shell.tray_like.lock().as_ref() {
        let _ = i.set_text(if liked { "Убрать из избранного" } else { "В избранное" });
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
