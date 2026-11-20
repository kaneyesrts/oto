//! Lyrics resolution: a cascade of sources, preferring time-synced text from any of them,
//! then fitting the timing to the actual audio file.

pub mod align;
pub mod lrc;
pub mod lrclib;

use crate::db::{now_ms, Db, Track};
use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub t: u32,
    pub d: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    /// Start, ms.
    pub t: u32,
    /// Duration, ms.
    pub d: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Word>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LyricsDoc {
    /// "synced" | "estimated" | "plain" | "instrumental" | "none"
    pub kind: String,
    pub source: String,
    pub lines: Vec<LyricLine>,
    /// Automatic correction already applied to `lines` (for display only).
    pub auto_shift_ms: i32,
    pub auto_scale: f32,
    /// User's manual nudge, applied by the UI on top.
    pub user_offset_ms: i32,
}

struct Candidate {
    source: &'static str,
    synced: Option<Vec<LyricLine>>,
    plain: Option<String>,
    /// Duration of the recording the lyrics were timed against, if the source says.
    duration_ms: Option<u64>,
    /// True when the text is known to belong to this exact file.
    same_file: bool,
    instrumental: bool,
}

/// Lyrics from streaming services: (text, duration of that recording, label, same recording).
pub type ServiceLyrics<'a> = &'a dyn Fn(&Track) -> Option<(String, Option<u64>, &'static str, bool)>;

pub fn resolve(db: &Db, track: &Track, online: bool, auto_align: bool, force: bool, services: ServiceLyrics) -> LyricsDoc {
    if !force {
        if let Some((json, at)) = db.lyrics_cached(&track.id) {
            if let Ok(mut doc) = serde_json::from_str::<LyricsDoc>(&json) {
                let stale_miss = doc.kind == "none" && now_ms() - at > 3 * 24 * 3600 * 1000;
                if !stale_miss {
                    doc.user_offset_ms = db.lyrics_offset(&track.id).unwrap_or(0);
                    return doc;
                }
            }
        }
    }

    let mut cands: Vec<Candidate> = vec![];
    let path = Path::new(&track.path);
    let local = track.source == "local";

    // 0. A streaming track's own service: same recording, timing needs no fitting.
    if !local && online {
        if let Some((text, dur, label, same)) = services(track) {
            push_text(&mut cands, label, &text, dur, same);
        }
    }

    // 1. Sidecar .lrc / .txt
    for ext in ["lrc", "LRC", "txt"] {
        if !local {
            break;
        }
        let p = path.with_extension(ext);
        if let Ok(bytes) = std::fs::read(&p) {
            let text = String::from_utf8_lossy(&bytes).to_string();
            push_text(&mut cands, "Файл .lrc", &text, None, true);
            break;
        }
    }
    // 2. Embedded in tags
    if local && !has_synced(&cands) {
        if let Ok(tagged) = lofty::read_from_path(path) {
            for tag in tagged.tags() {
                for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
                    if let Some(text) = tag.get_string(key) {
                        push_text(&mut cands, "Теги файла", text, None, true);
                    }
                }
            }
        }
    }
    // 3. LRCLIB
    if online && !has_synced(&cands) {
        if let Some(hit) = lrclib::find(track) {
            let dur = Some((hit.duration.unwrap_or(0.0) * 1000.0) as u64).filter(|d| *d > 0);
            if hit.instrumental {
                cands.push(Candidate { source: "LRCLIB", synced: None, plain: None, duration_ms: dur, same_file: false, instrumental: true });
            } else {
                if let Some(s) = hit.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
                    push_text(&mut cands, "LRCLIB", s, dur, false);
                }
                if let Some(p) = hit.plain_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
                    push_text(&mut cands, "LRCLIB", p, dur, false);
                }
            }
        }
    }

    // 4. Another service that has the same song (local files, SoundCloud uploads).
    if online && local && !has_synced(&cands) {
        if let Some((text, dur, label, same)) = services(track) {
            push_text(&mut cands, label, &text, dur, same);
        }
    }

    let mut doc = LyricsDoc { kind: "none".into(), source: String::new(), auto_scale: 1.0, ..Default::default() };

    if let Some(c) = cands.iter().find(|c| c.synced.is_some()) {
        let mut lines = c.synced.clone().unwrap();
        doc.kind = "synced".into();
        doc.source = c.source.into();
        if auto_align {
            // A text timed against a different cut of the song (longer intro, radio edit)
            // is the most common reason lyrics drift. Fit it to this file's vocal onsets.
            let mismatch = match c.duration_ms {
                Some(d) => (d as i64 - track.duration_ms as i64).abs() > 1500,
                None => !c.same_file,
            };
            if mismatch || !c.same_file {
                if let Some(feat) = align::analyse(&track.path) {
                    let starts: Vec<u32> = lines.iter().filter(|l| !l.text.trim().is_empty()).map(|l| l.t).collect();
                    if let Some((shift, scale)) = align::fit(&starts, &feat, mismatch) {
                        for l in lines.iter_mut() {
                            l.t = ((l.t as f32 * scale) as i64 + shift as i64).max(0) as u32;
                            if let Some(ws) = &mut l.words {
                                for w in ws {
                                    w.t = ((w.t as f32 * scale) as i64 + shift as i64).max(0) as u32;
                                }
                            }
                        }
                        lrc::fill_durations(&mut lines);
                        doc.auto_shift_ms = shift;
                        doc.auto_scale = scale;
                    }
                }
            }
        }
        doc.lines = lines;
    } else if let Some(c) = cands.iter().find(|c| c.plain.is_some()) {
        let text = c.plain.clone().unwrap();
        doc.source = c.source.into();
        let raw = lrc::plain_lines(&text);
        match align::analyse(&track.path).map(|f| align::estimate(&raw, &f)) {
            Some(lines) if !lines.is_empty() => {
                doc.kind = "estimated".into();
                doc.lines = lines;
            }
            _ => {
                doc.kind = "plain".into();
                doc.lines = raw.into_iter().map(|text| LyricLine { t: 0, d: None, text, words: None }).collect();
            }
        }
    } else if cands.iter().any(|c| c.instrumental) {
        doc.kind = "instrumental".into();
        doc.source = "LRCLIB".into();
    }

    if let Ok(json) = serde_json::to_string(&doc) {
        db.cache_lyrics(&track.id, &json);
    }
    doc.user_offset_ms = db.lyrics_offset(&track.id).unwrap_or(0);
    doc
}

fn has_synced(c: &[Candidate]) -> bool {
    c.iter().any(|c| c.synced.is_some())
}

fn push_text(cands: &mut Vec<Candidate>, source: &'static str, text: &str, duration_ms: Option<u64>, same_file: bool) {
    if text.trim().is_empty() {
        return;
    }
    if lrc::looks_synced(text) {
        let lines = lrc::parse(text);
        if !lines.is_empty() {
            cands.push(Candidate { source, synced: Some(lines), plain: None, duration_ms, same_file, instrumental: false });
            return;
        }
    }
    cands.push(Candidate { source, synced: None, plain: Some(text.to_string()), duration_ms, same_file, instrumental: false });
}
