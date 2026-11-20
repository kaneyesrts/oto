//! «Поток»: an endless station built from what you actually listen to (not your likes).
//!
//! Seeds come from play history: listening time weighted by recency, minus skips.
//! Each batch mixes "new to you" tracks (services' similar-track engines and similar
//! artists) with an occasional familiar one from your library, in a ratio set by `novelty`.
//! Skips during the session push an artist out of the station.

use crate::db::{match_key, now_ms, Db, Track};
use crate::providers::Provider;
use crate::sources::{to_track, Sources};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

const DAY: i64 = 86_400_000;
const BATCH: usize = 12;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationReq {
    pub mode: String,
    pub novelty: f32,
    pub exclude_recent: bool,
    /// Ids already queued in this session (never repeat them).
    pub exclude: Vec<String>,
    /// When the session started: skips after this steer the station.
    pub session_start: i64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StationBatch {
    pub tracks: Vec<Track>,
    /// Artists the station is built on, for "на основе …".
    pub basis: Vec<String>,
}

// ---------- tiny deterministic RNG (no extra crate) ----------
struct Rng(u64);
impl Rng {
    fn new() -> Self {
        Rng((now_ms() as u64).wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
    /// Weighted pick of `k` distinct indices.
    fn pick_weighted(&mut self, weights: &[f64], k: usize) -> Vec<usize> {
        let mut w: Vec<f64> = weights.iter().map(|x| x.max(0.0)).collect();
        let mut out = vec![];
        for _ in 0..k.min(w.len()) {
            let total: f64 = w.iter().sum();
            if total <= 0.0 {
                break;
            }
            let mut r = self.f() * total;
            for (i, x) in w.iter_mut().enumerate() {
                if r < *x {
                    out.push(i);
                    *x = 0.0;
                    break;
                }
                r -= *x;
            }
        }
        out
    }
}

pub fn main_artist(a: &str) -> String {
    crate::lyrics::lrclib::main_artist(a)
}

struct History {
    /// (track, score) best first.
    tracks: Vec<(Track, f64)>,
    /// (artist, score) best first.
    artists: Vec<(String, f64)>,
    recent_keys: HashSet<String>,
    session_skipped_artists: HashSet<String>,
    known_artists: HashSet<String>,
    last_played: HashMap<String, i64>,
}

fn history(db: &Db, session_start: i64) -> History {
    let now = now_ms();
    let rows: Vec<(String, i64, i64, i64, bool)> = {
        let c = db.0.lock();
        let mut st = c
            .prepare("SELECT track_id, started_at, listened_ms, duration_ms, skipped FROM plays WHERE started_at > ? ORDER BY started_at")
            .unwrap();
        st.query_map([now - 120 * DAY], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0)))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    };
    let mut score: HashMap<String, f64> = HashMap::new();
    let mut last_played: HashMap<String, i64> = HashMap::new();
    let mut recent_ids: HashSet<String> = HashSet::new();
    let mut session_skipped: HashSet<String> = HashSet::new();
    for (id, at, listened, dur, skipped) in &rows {
        let age_days = (now - at) as f64 / DAY as f64;
        let w = (-age_days / 21.0).exp();
        let counted = *listened >= 30_000.min(dur / 2);
        let s = if counted { 1.0 } else if *skipped { -0.6 } else { 0.15 };
        *score.entry(id.clone()).or_default() += s * w;
        last_played.insert(id.clone(), *at);
        if now - at < 3 * DAY {
            recent_ids.insert(id.clone());
        }
        if *at >= session_start && (*skipped || *listened < 20_000) {
            session_skipped.insert(id.clone());
        }
    }
    let mut tracks: Vec<(Track, f64)> = score.into_iter().filter_map(|(id, s)| db.track(&id).map(|t| (t, s))).filter(|(_, s)| *s > 0.0).collect();
    tracks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut by_artist: HashMap<String, f64> = HashMap::new();
    for (t, s) in &tracks {
        *by_artist.entry(main_artist(&t.artist)).or_default() += s;
    }
    let mut artists: Vec<(String, f64)> = by_artist.into_iter().collect();
    artists.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let recent_keys = recent_ids.iter().filter_map(|id| db.track(id)).map(|t| match_key(&t.artist, &t.title)).collect();
    let session_skipped_artists = session_skipped.iter().filter_map(|id| db.track(id)).map(|t| norm(&main_artist(&t.artist))).collect();
    let mut known_artists: HashSet<String> = db.all_tracks().iter().filter(|t| t.saved).map(|t| norm(&main_artist(&t.artist))).collect();
    known_artists.extend(tracks.iter().map(|(t, _)| norm(&main_artist(&t.artist))));
    History { tracks, artists, recent_keys, session_skipped_artists, known_artists, last_played }
}

/// Song identity that survives re-upload noise ("Game - Song By X (Cover)", "05 - Artist -
/// Song"): bracketed parts and "by …" dropped, last " - " segment kept. None when too
/// short to be distinctive ("Intro").
fn loose_title(title: &str) -> Option<String> {
    let mut t = String::new();
    let mut depth = 0;
    for c in title.to_lowercase().chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => t.push(c),
            _ => {}
        }
    }
    let mut t = t.as_str();
    for cut in [" by ", " feat", " ft.", " ft ", " prod", " slowed", " sped up"] {
        t = t.split(cut).next().unwrap_or(t);
    }
    let t = t.rsplit(" - ").next().unwrap_or(t);
    let k: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
    (k.chars().count() >= 8).then_some(k)
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The seed as a track on a service that can recommend (its own, or a Yandex match).
fn resolve_seed(srcs: &Sources, t: &Track) -> Option<(Arc<dyn Provider>, String)> {
    if matches!(t.source.as_str(), "yandex" | "soundcloud") {
        return srcs.provider(&t.source).map(|p| (p, t.remote_id.clone()));
    }
    let y = srcs.provider("yandex")?;
    let key = match_key(&t.artist, &t.title);
    let hits = y.search(&format!("{} {}", main_artist(&t.artist), crate::lyrics::lrclib::clean_title(&t.title)), 5).ok()?;
    let h = hits.into_iter().find(|h| match_key(&h.artist, &h.title) == key)?;
    Some((y, h.remote_id))
}

const GENRES: &[&str] = &[
    "rock", "pop", "indie", "electronics", "hiphop", "rusrap", "alternative", "metal", "jazz", "soul",
    "rnb", "dance", "folk", "punk", "ambient", "classical", "reggae", "blues", "lofi", "house", "techno",
];
const RANDOM_WORDS: &[&str] = &[
    "ночь", "город", "summer", "love", "dream", "rain", "fire", "neon", "ocean", "дорога", "звёзды",
    "midnight", "gold", "ghost", "wild", "мечта", "весна", "heart", "lost", "sky", "drive", "moon",
];

#[tauri::command]
pub async fn station_next(app: AppHandle, req: StationReq) -> StationBatch {
    tauri::async_runtime::spawn_blocking(move || build(&app, req)).await.unwrap_or_default()
}

fn build(app: &AppHandle, req: StationReq) -> StationBatch {
    let db = app.state::<Db>();
    let srcs = app.state::<Sources>();
    let h = history(&db, req.session_start);
    let mut rng = Rng::new();
    let now = now_ms();

    let exclude_keys: HashSet<String> = req
        .exclude
        .iter()
        .filter_map(|id| db.track(id))
        .map(|t| match_key(&t.artist, &t.title))
        .chain(if req.exclude_recent { h.recent_keys.iter().cloned().collect::<Vec<_>>() } else { vec![] })
        .collect();
    let ok = |artist: &str, title: &str, seen: &HashSet<String>| -> bool {
        let k = match_key(artist, title);
        let loose = loose_title(title).is_some_and(|l| seen.contains(&l));
        !exclude_keys.contains(&k) && !seen.contains(&k) && !loose && !h.session_skipped_artists.contains(&norm(&main_artist(artist)))
    };
    let mark = |seen: &mut HashSet<String>, t: &Track| {
        seen.insert(match_key(&t.artist, &t.title));
        seen.extend(loose_title(&t.title));
    };

    let mode = req.mode.as_str();
    let novelty = match mode {
        "discover" | "random" => 1.0,
        "favorites" => 0.15,
        _ => req.novelty.clamp(0.0, 1.0),
    };
    let basis: Vec<String> = h.artists.iter().take(5).map(|(a, _)| a.clone()).collect();

    // ---------- new-to-you candidates ----------
    let mut fresh: Vec<Track> = vec![];
    if mode == "random" || h.tracks.is_empty() {
        // Random: genre radio when logged in to Yandex, else random searches.
        let y = srcs.yandex();
        if srcs.status_connected("yandex") {
            // Each station call returns ~5 tracks: four different genres per batch.
            let mut genres: Vec<&str> = GENRES.to_vec();
            rng.shuffle(&mut genres);
            let got: Vec<Vec<Track>> = std::thread::scope(|sc| {
                let hs: Vec<_> = genres
                    .iter()
                    .take(4)
                    .map(|g| {
                        let y = y.clone();
                        sc.spawn(move || y.rotor(&format!("genre:{g}")).map(|v| v.iter().map(|r| to_track(r, false)).collect()).unwrap_or_default())
                    })
                    .collect();
                hs.into_iter().filter_map(|h| h.join().ok()).collect()
            });
            for v in got {
                fresh.extend(v);
            }
        }
        if fresh.len() < BATCH * 2 {
            // Top-up: a few random words, only a handful of hits from each, so no single
            // word ("ocean") takes over the batch.
            let mut words: Vec<&str> = RANDOM_WORDS.to_vec();
            rng.shuffle(&mut words);
            for (k, w) in words.iter().take(4).enumerate() {
                let p = if k % 2 == 0 { srcs.provider("yandex") } else { srcs.provider("soundcloud") };
                let Some(p) = p else { continue };
                if let Ok(mut v) = p.search(w, 30) {
                    rng.shuffle(&mut v);
                    fresh.extend(v.iter().take(4).map(|r| to_track(r, false)));
                }
            }
        }
    } else {
        // Similar tracks for a few weighted-random seeds, in parallel.
        let seed_w: Vec<f64> = h.tracks.iter().take(25).map(|(_, s)| *s).collect();
        let seeds: Vec<Track> = rng.pick_weighted(&seed_w, 4).into_iter().map(|i| h.tracks[i].0.clone()).collect();
        let srcs_ref: &Sources = &srcs;
        let similar: Vec<Vec<Track>> = std::thread::scope(|sc| {
            let hs: Vec<_> = seeds
                .iter()
                .map(|t| {
                    sc.spawn(move || {
                        let Some((p, id)) = resolve_seed(srcs_ref, t) else { return vec![] };
                        p.similar(&id).map(|v| v.iter().map(|r| to_track(r, false)).collect()).unwrap_or_default()
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for v in similar {
            fresh.extend(v);
        }
        // Similar artists' popular tracks: wider net, and the only source in "discover".
        let art_w: Vec<f64> = h.artists.iter().take(12).map(|(_, s)| *s).collect();
        for i in rng.pick_weighted(&art_w, if mode == "discover" { 3 } else { 2 }) {
            let name = &h.artists[i].0;
            let r = db
                .artist_cached(name)
                .map(|(_, r, _)| r)
                .filter(|r| !r.is_empty())
                .or_else(|| {
                    let y = srcs.provider("yandex")?;
                    let a = y.search_artists(name, 5).ok()?.into_iter().find(|a| norm(&a.name) == norm(name))?;
                    Some(format!("yandex:{}", a.remote_id))
                });
            let Some(r) = r else { continue };
            let Some((sid, aid)) = r.split_once(':') else { continue };
            let Some(p) = srcs.provider(sid) else { continue };
            let Ok(page) = p.artist(aid) else { continue };
            let mut sims = page.similar.clone();
            rng.shuffle(&mut sims);
            for s in sims.iter().take(if mode == "discover" { 3 } else { 2 }) {
                if let Ok(sp) = p.artist(&s.remote_id) {
                    let mut top: Vec<Track> = sp.top_tracks.iter().map(|r| to_track(r, false)).collect();
                    rng.shuffle(&mut top);
                    fresh.extend(top.into_iter().take(3));
                }
            }
        }
    }
    if mode == "discover" {
        fresh.retain(|t| !h.known_artists.contains(&norm(&main_artist(&t.artist))));
    }
    // "Similar" answers often contain songs you already have; those are not discoveries,
    // they only enter through the (small) familiar quota below.
    let lib = db.all_tracks();
    let lib_keys: HashSet<String> = lib.iter().filter(|t| t.saved).map(|t| match_key(&t.artist, &t.title)).collect();
    let lib_loose: HashSet<String> = lib.iter().filter(|t| t.saved).filter_map(|t| loose_title(&t.title)).collect();
    drop(lib);
    fresh.retain(|t| {
        !lib_keys.contains(&match_key(&t.artist, &t.title))
            && !loose_title(&t.title).is_some_and(|l| lib_loose.contains(&l))
            && !db.track(&t.id).is_some_and(|k| k.saved)
    });
    rng.shuffle(&mut fresh);

    // ---------- familiar candidates (your library) ----------
    let mut familiar: Vec<Track> = vec![];
    if novelty < 1.0 {
        let liked_or_top: Vec<Track> = {
            let top_artists: HashSet<String> = h.artists.iter().take(15).map(|(a, _)| norm(a)).collect();
            db.all_tracks()
                .into_iter()
                .filter(|t| t.saved && !t.hidden)
                .filter(|t| t.liked || top_artists.contains(&norm(&main_artist(&t.artist))))
                .collect()
        };
        // Prefer what you haven't heard in a while ("forgotten" favourites).
        let w: Vec<f64> = liked_or_top
            .iter()
            .map(|t| {
                let since = h.last_played.get(&t.id).map(|at| (now - at) as f64 / DAY as f64).unwrap_or(60.0);
                (since / 14.0).min(3.0) + if t.liked { 0.5 } else { 0.0 }
            })
            .collect();
        familiar = rng.pick_weighted(&w, BATCH * 2).into_iter().map(|i| liked_or_top[i].clone()).collect();
        if mode == "favorites" {
            // Plus the top tracks themselves.
            familiar.extend(h.tracks.iter().take(30).map(|(t, _)| t.clone()));
            rng.shuffle(&mut familiar);
        }
    }

    // ---------- compose ----------
    // The station is for finding music: library tracks are an occasional familiar beat
    // (about 1 in 12 at the default setting, at most 5 with the slider all the way to
    // "familiar"); only "Любимое" is built mostly from the library.
    let fam_share = if mode == "favorites" { 0.85 } else { (1.0 - novelty) * 0.4 };
    let n_fam = ((BATCH as f32) * fam_share).round() as usize;
    let n_new = BATCH - n_fam;
    let mut seen: HashSet<String> = HashSet::new();
    let mut new_pick = vec![];
    for t in fresh {
        if new_pick.len() >= n_new {
            break;
        }
        if ok(&t.artist, &t.title, &seen) && t.duration_ms > 45_000 {
            mark(&mut seen, &t);
            new_pick.push(t);
        }
    }
    let mut fam_pick = vec![];
    // Short on new tracks: a smaller batch (the queue asks again sooner) rather than
    // padding with the library — unless there is nothing new at all.
    let fam_cap = if new_pick.is_empty() { BATCH } else { n_fam };
    for t in familiar {
        if fam_pick.len() >= fam_cap {
            break;
        }
        if ok(&t.artist, &t.title, &seen) {
            mark(&mut seen, &t);
            fam_pick.push(t);
        }
    }
    // Spread familiar tracks evenly; a batch never opens with one (unless it's all familiar).
    let total = new_pick.len() + fam_pick.len();
    let mut out = Vec::with_capacity(total);
    if mode == "favorites" || new_pick.is_empty() {
        let (mut a, mut b) = (fam_pick.into_iter(), new_pick.into_iter());
        let ratio = (1.0 - fam_share).max(0.01);
        let mut acc = 0f32;
        loop {
            acc += ratio;
            let take_new = acc >= 1.0;
            if take_new {
                acc -= 1.0;
            }
            let next = if take_new { b.next().or_else(|| a.next()) } else { a.next().or_else(|| b.next()) };
            match next {
                Some(t) => out.push(t),
                None => break,
            }
        }
    } else {
        let nf = fam_pick.len();
        let slots: Vec<usize> = (0..nf).map(|i| (i + 1) * total / (nf + 1)).collect();
        let (mut a, mut b) = (new_pick.into_iter(), fam_pick.into_iter());
        for i in 0..total {
            let next = if slots.contains(&i) { b.next().or_else(|| a.next()) } else { a.next().or_else(|| b.next()) };
            out.extend(next);
        }
    }
    // Reflect what the library knows about these tracks (liked, cached cover).
    for t in out.iter_mut() {
        if let Some(k) = db.track(&t.id) {
            t.saved = k.saved;
            t.liked = k.liked;
            if k.cover.as_deref().is_some_and(|c| !c.starts_with("http")) {
                t.cover = k.cover;
                t.palette = k.palette;
            }
        }
    }
    StationBatch { tracks: out, basis }
}

/// Top artists for the station card ("на основе …"), without building a batch.
#[tauri::command]
pub fn station_basis(app: AppHandle) -> Vec<String> {
    history(&app.state::<Db>(), now_ms()).artists.into_iter().take(5).map(|(a, _)| a).collect()
}


#[cfg(test)]
mod loose_tests {
    use super::loose_title;
    #[test]
    fn reuploads_collapse() {
        let a = loose_title("Never Fade Away By SAMURAI");
        assert!(a.is_some());
        assert_eq!(a, loose_title("Never Fade Away by P. T. Adamczyk & Olga Jankowska (SAMURAI Cover)"));
        assert_eq!(a, loose_title("Never Fade Away"));
        assert_eq!(a, loose_title("Never Fade Away (SAMURAI Cover) feat. Olga Jankowska"));
        assert_eq!(loose_title("05 - P.T. Adamczyk - Outsider No More (Cyberpunk 2077)"), loose_title("Outsider No More"));
        assert_eq!(loose_title("Intro"), None);
        assert_ne!(loose_title("Chippin' In"), a);
    }
}
