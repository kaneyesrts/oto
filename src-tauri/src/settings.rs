use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Missing in old files → 0, so migrations run.
    #[serde(default)]
    pub version: u32,
    pub theme: String,
    pub accent_from_cover: bool,
    pub reduced_motion: bool,
    pub close_to_tray: bool,
    pub folders: Vec<String>,
    pub volume: f32,
    pub lyrics: LyricsSettings,
    pub visual: VisualSettings,
    pub widget: WidgetSettings,
    pub hotkeys: Hotkeys,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            theme: "aura".into(),
            accent_from_cover: true,
            reduced_motion: false,
            close_to_tray: true,
            folders: vec![],
            volume: 0.8,
            lyrics: LyricsSettings::default(),
            visual: VisualSettings::default(),
            widget: WidgetSettings::default(),
            hotkeys: Hotkeys::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LyricsSettings {
    pub font_scale: f32,
    pub depth_blur: bool,
    pub auto_align: bool,
    pub online: bool,
}
impl Default for LyricsSettings {
    fn default() -> Self {
        Self { font_scale: 1.0, depth_blur: true, auto_align: true, online: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VisualSettings {
    pub particles: bool,
    pub intensity: f32,
    /// "low" | "high"
    pub quality: String,
}
impl Default for VisualSettings {
    fn default() -> Self {
        Self { particles: true, intensity: 1.0, quality: "high".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WidgetSettings {
    pub enabled: bool,
    /// "desktop" (below all windows) | "overlay" (above all windows)
    pub layer: String,
    /// "compact" | "lyrics" | "spectrum"
    pub style: String,
    pub opacity: f32,
    pub blur: f32,
    pub scale: f32,
    pub click_through: bool,
    pub glow: bool,
    pub particles: bool,
    pub spectrum: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
}
impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            layer: "overlay".into(),
            style: "compact".into(),
            opacity: 0.92,
            blur: 24.0,
            scale: 1.0,
            click_through: false,
            glow: true,
            particles: false,
            spectrum: true,
            x: None,
            y: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    pub play_pause: String,
    pub next: String,
    pub prev: String,
    pub volume_up: String,
    pub volume_down: String,
    pub like: String,
    pub toggle_widget: String,
    pub toggle_click_through: String,
    pub seek_forward: String,
    pub seek_back: String,
}
impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            play_pause: "Ctrl+Alt+Space".into(),
            next: "Ctrl+Alt+Right".into(),
            prev: "Ctrl+Alt+Left".into(),
            volume_up: "Ctrl+Alt+Up".into(),
            volume_down: "Ctrl+Alt+Down".into(),
            like: "Ctrl+Alt+L".into(),
            toggle_widget: "Ctrl+Alt+W".into(),
            toggle_click_through: "Ctrl+Alt+O".into(),
            seek_forward: "Ctrl+Alt+Shift+Right".into(),
            seek_back: "Ctrl+Alt+Shift+Left".into(),
        }
    }
}

impl Hotkeys {
    pub fn entries(&self) -> Vec<(crate::shell::Action, &str)> {
        use crate::shell::Action::*;
        vec![
            (TogglePlay, self.play_pause.as_str()),
            (Next, self.next.as_str()),
            (Prev, self.prev.as_str()),
            (VolumeUp, self.volume_up.as_str()),
            (VolumeDown, self.volume_down.as_str()),
            (Like, self.like.as_str()),
            (ToggleWidget, self.toggle_widget.as_str()),
            (ToggleClickThrough, self.toggle_click_through.as_str()),
            (SeekForward, self.seek_forward.as_str()),
            (SeekBack, self.seek_back.as_str()),
        ]
    }
}

pub const CURRENT_VERSION: u32 = 3;

pub fn load(path: &PathBuf) -> Settings {
    let mut s: Settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if s.version < 3 {
        // v3: the adaptive "aura" theme became the default, and the widget defaults to the
        // overlay layer: "desktop" sits under every window and looked like it never opened.
        if s.theme == "noir" {
            s.theme = "aura".into();
        }
        s.widget.layer = "overlay".into();
        s.widget.x = None;
        s.widget.y = None;
    }
    s.version = CURRENT_VERSION;
    s
}

pub fn save(path: &PathBuf, s: &Settings) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(s) {
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
