//! Local library: folder scanning, tag reading, cover extraction and palette analysis.

use crate::db::{now_ms, Db, Track};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use lofty::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const AUDIO_EXT: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aif", "aiff", "caf", "mka"];
const FOLDER_COVERS: &[&str] = &["cover", "folder", "front", "album", "artwork"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
    pub finished: bool,
}

pub fn track_id(path: &str) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(path.to_lowercase().as_bytes());
    h.digest().to_string()[..16].to_string()
}

pub fn scan(db: &Db, roots: &[String], cover_dir: &Path, mut progress: impl FnMut(ScanProgress)) {
    let _ = std::fs::create_dir_all(cover_dir);
    let files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|r| WalkDir::new(r).follow_links(true).into_iter().filter_map(Result::ok))
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();

    let total = files.len();
    let mut batch = Vec::with_capacity(64);
    for (i, path) in files.iter().enumerate() {
        let path_str = path.to_string_lossy().to_string();
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if db.mtime_of(&path_str) != Some(mtime) {
            if let Some(t) = read_track(path, cover_dir) {
                batch.push((t, mtime));
            }
        }
        if batch.len() >= 64 {
            db.upsert_tracks(&batch);
            batch.clear();
        }
        if i % 16 == 0 {
            progress(ScanProgress { done: i, total, finished: false });
        }
    }
    db.upsert_tracks(&batch);
    for r in roots {
        db.prune_missing(r);
    }
    progress(ScanProgress { done: total, total, finished: true });
}

fn read_track(path: &Path, cover_dir: &Path) -> Option<Track> {
    let tagged = lofty::read_from_path(path).ok()?;
    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    let get = |f: &dyn Fn(&lofty::tag::Tag) -> Option<String>| tag.and_then(|t| f(t)).filter(|s| !s.trim().is_empty());
    let (fallback_artist, fallback_title) = split_filename(&stem);
    let title = get(&|t| t.title().map(|s| s.to_string())).unwrap_or(fallback_title);
    let artist = get(&|t| t.artist().map(|s| s.to_string())).unwrap_or(fallback_artist);
    let album = get(&|t| t.album().map(|s| s.to_string())).unwrap_or_default();
    let album_artist = get(&|t| t.get_string(ItemKey::AlbumArtist).map(|s| s.to_string())).unwrap_or_else(|| artist.clone());

    let path_str = path.to_string_lossy().to_string();
    let id = track_id(&path_str);

    // Covers are shared per album so a 14-track album produces one image on disk.
    let cover_key = if album.is_empty() { id.clone() } else { track_id(&format!("{album_artist}\u{1f}{album}")) };
    let cover_path = cover_dir.join(format!("{cover_key}.jpg"));
    let palette_path = cover_dir.join(format!("{cover_key}.json"));
    let (cover, palette) = if cover_path.exists() {
        let pal = std::fs::read_to_string(&palette_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        (Some(cover_path.to_string_lossy().to_string()), pal)
    } else {
        let bytes = tag
            .and_then(|t| {
                t.pictures()
                    .iter()
                    .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
                    .or_else(|| t.pictures().first())
                    .map(|p| p.data().to_vec())
            })
            .or_else(|| folder_cover(path));
        match bytes.and_then(|b| image::load_from_memory(&b).ok()) {
            Some(img) => {
                let pal = palette_of(&img);
                let img = if img.width() > 640 { img.resize(640, 640, FilterType::Lanczos3) } else { img };
                if img.to_rgb8().save_with_format(&cover_path, image::ImageFormat::Jpeg).is_ok() {
                    let _ = std::fs::write(&palette_path, serde_json::to_string(&pal).unwrap_or_default());
                    (Some(cover_path.to_string_lossy().to_string()), pal)
                } else {
                    (None, pal)
                }
            }
            None => (None, vec![]),
        }
    };

    Some(Track {
        id,
        path: path_str,
        title,
        artist,
        album,
        album_artist,
        duration_ms: props.duration().as_millis() as u64,
        track_no: tag.and_then(|t| t.track()),
        year: tag.and_then(|t| t.date()).map(|d| d.year as u32),
        genre: get(&|t| t.genre().map(|s| s.to_string())),
        cover,
        palette,
        liked: false,
        added_at: now_ms(),
        source: "local".into(),
    })
}

/// "Artist - Title" is the most common naming for untagged files.
fn split_filename(stem: &str) -> (String, String) {
    let cleaned = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ' || c == '-' || c == '_');
    match cleaned.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => ("Unknown artist".into(), if cleaned.is_empty() { stem.to_string() } else { cleaned.to_string() }),
    }
}

fn folder_cover(track: &Path) -> Option<Vec<u8>> {
    let dir = track.parent()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        let stem = p.file_stem()?.to_string_lossy().to_lowercase();
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if FOLDER_COVERS.contains(&stem.as_str()) && matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            return std::fs::read(p).ok();
        }
    }
    None
}

/// K-means on a 48×48 thumbnail. Returns colours sorted by how much area they cover,
/// with near-duplicates merged, so the UI can pick a background (0) and an accent (most vivid).
pub fn palette_of(img: &DynamicImage) -> Vec<[u8; 3]> {
    let small = img.resize_exact(48, 48, FilterType::Triangle).to_rgb8();
    let px: Vec<[f32; 3]> = small.pixels().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let k = 6;
    // Deterministic seeding spread across the image.
    let mut centers: Vec<[f32; 3]> = (0..k).map(|i| px[(i * px.len()) / k + px.len() / (2 * k)]).collect();
    let mut assign = vec![0usize; px.len()];
    for _ in 0..12 {
        for (i, p) in px.iter().enumerate() {
            assign[i] = (0..k)
                .min_by(|&a, &b| dist(p, &centers[a]).partial_cmp(&dist(p, &centers[b])).unwrap())
                .unwrap();
        }
        let mut sums = vec![[0f32; 4]; k];
        for (i, p) in px.iter().enumerate() {
            let s = &mut sums[assign[i]];
            s[0] += p[0];
            s[1] += p[1];
            s[2] += p[2];
            s[3] += 1.0;
        }
        for (c, s) in centers.iter_mut().zip(&sums) {
            if s[3] > 0.0 {
                *c = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
            }
        }
    }
    let mut counts = vec![0usize; k];
    for a in &assign {
        counts[*a] += 1;
    }
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(counts[i]));
    let mut out: Vec<[u8; 3]> = vec![];
    for i in order {
        if counts[i] == 0 {
            continue;
        }
        let c = centers[i];
        if out.iter().all(|o| dist(&[o[0] as f32, o[1] as f32, o[2] as f32], &c) > 900.0) {
            out.push([c[0] as u8, c[1] as u8, c[2] as u8]);
        }
        if out.len() == 5 {
            break;
        }
    }
    let _ = img.dimensions();
    out
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    // Weighted RGB distance, a cheap approximation of perceptual difference.
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    2.0 * dr * dr + 4.0 * dg * dg + 3.0 * db * db
}
