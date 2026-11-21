//! Word-exact lyrics sync ("по голосу"): listen to the song with a local speech recogniser
//! (whisper.cpp) and re-time the lyrics text word by word.
//!
//! Runs in the background after `get_lyrics`, one track at a time; a newer request cancels
//! the one in progress. The refined doc replaces the cached one, so it is instant next time.
//!
//! Events:
//! * `oto://wordsync-progress` `{ id, stage, progress }`, stage = "decode" | "recognize" |
//!   "align" | "done" | "failed" | "cancelled", progress 0..1 within the stage;
//! * `oto://lyrics-refined` `{ id, doc }` — the new doc for track `id`;
//! * `oto://wordsync-model` — model download progress (see `model.rs`).
//!
//! Benchmark (`bench::bench_fixture`, 6 songs RU/EN, plain text in, line starts compared with
//! human-timed LRC of the same recording, Ryzen 5 7500F, 6 threads):
//! * ggml-base-q5_1: ~16 s per 3.5–4 min song; 21–60 % of lyric words matched, median line
//!   start error 1.9 s — too many misheard words on sung vocals;
//! * ggml-small-q5_1: ~45–60 s per song; 30–93 % matched (EN rock 84–93 %), median error
//!   0.68 s overall and 0.34 s on the clean songs (≤1 s for 98 % of lines on the best one).
//! Small it is. DTW timestamps beat whisper's token timestamps (≤1 s: 98 % vs 68 %).

// Without the `wordsync` feature the pipeline is compiled out; keep the rest warning-free.
#![cfg_attr(not(feature = "wordsync"), allow(dead_code))]

pub mod align;
#[cfg(feature = "wordsync")]
pub mod asr;
pub mod audio;
pub mod model;

use super::{LyricLine, LyricsDoc};
use align::{Prior, RecWord};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(feature = "wordsync")]
use std::time::Instant;
use tauri::AppHandle;
#[cfg(feature = "wordsync")]
use tauri::{Emitter, Manager};

/// Below this share of lyric words matched to what was heard, the result is not trusted.
pub const MIN_MATCH_RATE: f32 = 0.35;

/// Decoder language for the lyrics: Cyrillic → Russian, Latin → English, otherwise detect.
pub fn lang_for(lines: &[LyricLine]) -> &'static str {
    let (mut cyr, mut lat, mut other) = (0usize, 0usize, 0usize);
    for c in lines.iter().flat_map(|l| l.text.chars()).filter(|c| c.is_alphabetic()) {
        match c {
            'а'..='я' | 'А'..='Я' | 'ё' | 'Ё' => cyr += 1,
            'a'..='z' | 'A'..='Z' => lat += 1,
            _ => other += 1,
        }
    }
    let all = (cyr + lat + other).max(1);
    if cyr * 10 >= all * 3 {
        "ru"
    } else if lat * 10 >= all * 7 {
        "en"
    } else {
        "auto"
    }
}

/// Decoder prompt for the next chunk: find where in the lyrics the words heard so far end,
/// and take a line back plus the next few lines from there. (Prompting with only the lines
/// sung so far, like whisper's own context, matched far fewer words in the benchmark.)
pub fn chunk_prompt(lines: &[String], heard: &[RecWord]) -> String {
    let texts: Vec<&str> = lines.iter().map(|s| s.trim()).collect();
    let cursor = if heard.is_empty() { None } else { align::align(lines, &vec![None; lines.len()], heard).last_anchor_line };
    let pick: &[&str] = match cursor {
        None => &texts[..texts.len().min(8)],
        Some(c) => &texts[c.saturating_sub(1)..(c + 7).min(texts.len())],
    };
    pick.iter().filter(|t| !t.is_empty()).copied().collect::<Vec<_>>().join("
")
}

/// Prior line timing from the doc, for tie-breaking (which chorus is which).
fn priors_for(doc: &LyricsDoc) -> Vec<Option<Prior>> {
    let slack = match doc.kind.as_str() {
        "synced" => 4000,
        "estimated" => 15_000,
        _ => return vec![None; doc.lines.len()],
    };
    doc.lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let next = doc.lines.get(i + 1).map(|n| n.t);
            let d = l.d.or(next.map(|n| n.saturating_sub(l.t))).unwrap_or(3000).min(15_000);
            Some(Prior { t: l.t, d, slack })
        })
        .collect()
}

/// Whether a doc is worth refining.
pub fn wants(doc: &LyricsDoc) -> bool {
    if doc.word_synced || doc.word_sync_failed || !matches!(doc.kind.as_str(), "synced" | "estimated" | "plain") {
        return false;
    }
    let lines: Vec<&LyricLine> = doc.lines.iter().filter(|l| !l.text.trim().is_empty()).collect();
    // Already karaoke-timed (enhanced LRC from the same recording).
    !lines.is_empty() && !lines.iter().all(|l| l.words.is_some())
}

/// Aligns `rec` to the doc's text. None when the match is too poor to trust.
pub fn apply(doc: &LyricsDoc, rec: &[RecWord]) -> Option<(LyricsDoc, f32)> {
    let texts: Vec<String> = doc.lines.iter().map(|l| l.text.clone()).collect();
    let res = align::align(&texts, &priors_for(doc), rec);
    let rate = res.match_rate();
    if res.total == 0 || rate < MIN_MATCH_RATE {
        return None;
    }
    let source = if doc.source.is_empty() { "по голосу".to_string() } else { format!("{} · по голосу", doc.source) };
    Some((
        LyricsDoc {
            kind: "synced".into(),
            source,
            lines: res.lines,
            auto_shift_ms: 0,
            auto_scale: 1.0,
            user_offset_ms: 0,
            word_synced: true,
            word_sync_failed: false,
        },
        rate,
    ))
}

#[derive(Serialize, Clone)]
struct ProgressEvent<'a> {
    id: &'a str,
    stage: &'a str,
    progress: f32,
}

#[derive(Serialize, Clone)]
struct RefinedEvent<'a> {
    id: &'a str,
    doc: &'a LyricsDoc,
}

struct Running {
    id: String,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Inner {
    pending: Mutex<Option<String>>,
    cv: Condvar,
    running: Mutex<Option<Running>>,
}

/// The background worker (managed Tauri state).
pub struct WordSync {
    inner: Arc<Inner>,
}

impl WordSync {
    pub fn start(app: AppHandle) -> Self {
        let inner = Arc::new(Inner::default());
        let w = inner.clone();
        std::thread::Builder::new()
            .name("oto-wordsync".into())
            .spawn(move || loop {
                let id = {
                    let mut p = w.pending.lock();
                    while p.is_none() {
                        w.cv.wait(&mut p);
                    }
                    p.take().unwrap()
                };
                let cancel = Arc::new(AtomicBool::new(false));
                *w.running.lock() = Some(Running { id: id.clone(), cancel: cancel.clone() });
                run(&app, &id, &cancel);
                *w.running.lock() = None;
            })
            .expect("wordsync thread");
        Self { inner }
    }

    /// Queues track `id` for refinement, superseding (and cancelling) any other track.
    pub fn request(&self, id: &str) {
        if let Some(r) = &*self.inner.running.lock() {
            if r.id == id {
                return;
            }
            r.cancel.store(true, Ordering::Relaxed);
        }
        *self.inner.pending.lock() = Some(id.to_string());
        self.inner.cv.notify_one();
    }

    /// Stops whatever is running or queued (e.g. lyrics force-reloaded, feature turned off).
    pub fn cancel(&self, id: Option<&str>) {
        let mut p = self.inner.pending.lock();
        if id.is_none() || p.as_deref() == id {
            *p = None;
        }
        if let Some(r) = &*self.inner.running.lock() {
            if id.is_none() || Some(r.id.as_str()) == id {
                r.cancel.store(true, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(not(feature = "wordsync"))]
fn run(_app: &AppHandle, _id: &str, _cancel: &AtomicBool) {}

#[cfg(feature = "wordsync")]
fn run(app: &AppHandle, id: &str, cancel: &AtomicBool) {
    use crate::db::Db;
    let emit = |stage: &str, progress: f32| {
        let _ = app.emit("oto://wordsync-progress", ProgressEvent { id, stage, progress });
    };
    let Some(model) = model::ready_path(app) else { return };
    if !app.state::<crate::AppState>().settings.read().lyrics.word_sync {
        return;
    }
    let db = app.state::<Db>();
    let Some(track) = db.track(id) else { return };
    if track.source == "spotify" {
        return; // its audio only exists inside the player
    }
    let Some(doc) = cached_doc(&db, id) else { return };
    if !wants(&doc) {
        return;
    }

    let throttled = |stage: &'static str| {
        let last = Mutex::new(Instant::now());
        move |p: f32| {
            let mut l = last.lock();
            if l.elapsed().as_millis() >= 200 || p >= 1.0 {
                *l = Instant::now();
                emit(stage, p);
            }
        }
    };
    emit("decode", 0.0);
    let pcm = match audio::decode_track(app, &track, cancel, &throttled("decode")) {
        Ok(Some(p)) => p,
        Ok(None) if cancel.load(Ordering::Relaxed) => return emit("cancelled", 0.0),
        Ok(None) => {
            mark_failed(&db, id, &doc);
            return emit("failed", 0.0);
        }
        Err(e) => {
            eprintln!("wordsync: decode {id}: {e}");
            return emit("failed", 0.0);
        }
    };
    if cancel.load(Ordering::Relaxed) {
        return emit("cancelled", 0.0);
    }

    emit("recognize", 0.0);
    let lang = lang_for(&doc.lines);
    let texts: Vec<String> = doc.lines.iter().map(|l| l.text.clone()).collect();
    let opt = asr::Options { lang, dtw: true, beam: false, threads: asr::default_threads() };
    let mono = pcm.mono();
    drop(pcm);
    let mut prompt = |heard: &[RecWord]| chunk_prompt(&texts, heard);
    let rec = match asr::transcribe(&model, &mono, &opt, &mut prompt, cancel, &throttled("recognize")) {
        Ok(Some(r)) => r,
        Ok(None) => return emit("cancelled", 0.0),
        Err(e) => {
            eprintln!("wordsync: recognize {id}: {e}");
            return emit("failed", 0.0);
        }
    };

    emit("align", 0.0);
    let Some((mut refined, _rate)) = apply(&doc, &rec) else {
        mark_failed(&db, id, &doc);
        return emit("failed", 1.0);
    };
    // The lyrics may have been reloaded while we listened: only replace what we started from.
    match cached_doc(&db, id) {
        Some(now) if same_text(&now, &doc) && !now.word_synced => {}
        _ => return emit("cancelled", 1.0),
    }
    if let Ok(json) = serde_json::to_string(&refined) {
        db.cache_lyrics(id, &json);
    }
    refined.user_offset_ms = db.lyrics_offset(id).unwrap_or(0);
    let _ = app.emit("oto://lyrics-refined", RefinedEvent { id, doc: &refined });
    emit("done", 1.0);
}

#[cfg(feature = "wordsync")]
fn cached_doc(db: &crate::db::Db, id: &str) -> Option<LyricsDoc> {
    db.lyrics_cached(id).and_then(|(json, _)| serde_json::from_str(&json).ok())
}

#[cfg(feature = "wordsync")]
fn same_text(a: &LyricsDoc, b: &LyricsDoc) -> bool {
    a.kind == b.kind && a.lines.len() == b.lines.len() && a.lines.iter().zip(&b.lines).all(|(x, y)| x.text == y.text)
}

/// Remembers that this text could not be matched to this recording, so it is not retried
/// on every play (a forced lyrics reload clears it).
#[cfg(feature = "wordsync")]
fn mark_failed(db: &crate::db::Db, id: &str, doc: &LyricsDoc) {
    if let Some(mut now) = cached_doc(db, id) {
        if same_text(&now, doc) {
            now.word_sync_failed = true;
            if let Ok(json) = serde_json::to_string(&now) {
                db.cache_lyrics(id, &json);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: u32, text: &str) -> LyricLine {
        LyricLine { t, d: None, text: text.into(), words: None }
    }

    #[test]
    fn picks_language_from_text() {
        assert_eq!(lang_for(&[line(0, "Группа крови на рукаве")]), "ru");
        assert_eq!(lang_for(&[line(0, "Hello from the other side")]), "en");
        assert_eq!(lang_for(&[line(0, "Я люблю rock and roll, детка")]), "ru");
        assert_eq!(lang_for(&[line(0, "飞行模式 飞行模式")]), "auto");
    }

    #[test]
    fn chunk_prompt_follows_the_singer() {
        let l: Vec<String> = (0..20).map(|i| format!("line number {i}")).collect();
        assert!(chunk_prompt(&l, &[]).starts_with("line number 0
line number 1"));
        let heard: Vec<RecWord> = ["line", "number", "10"].iter().map(|w| RecWord { t0: 0, t1: 1, text: w.to_string() }).collect();
        let p = chunk_prompt(&l, &heard);
        assert!(p.starts_with("line number 9
"), "{p}");
        assert!(p.ends_with("line number 16"), "{p}");
    }

    #[test]
    fn apply_retimes_plain_lyrics_and_guards_garbage() {
        let doc = LyricsDoc {
            kind: "plain".into(),
            source: "LRCLIB".into(),
            lines: vec![line(0, "one two three"), line(0, "four five six")],
            auto_scale: 1.0,
            ..Default::default()
        };
        assert!(wants(&doc));
        let rec: Vec<RecWord> = ["one", "two", "three", "four", "five", "six"]
            .iter()
            .enumerate()
            .map(|(i, w)| RecWord { t0: 1000 + i as u32 * 500, t1: 1400 + i as u32 * 500, text: w.to_string() })
            .collect();
        let (r, rate) = apply(&doc, &rec).expect("refined");
        assert_eq!(rate, 1.0);
        assert_eq!(r.kind, "synced");
        assert!(r.word_synced);
        assert_eq!(r.source, "LRCLIB · по голосу");
        assert_eq!(r.lines[1].t, 2500);
        assert!(!wants(&r));

        let junk: Vec<RecWord> = vec![RecWord { t0: 0, t1: 500, text: "music".into() }];
        assert!(apply(&doc, &junk).is_none());
    }
}

/// End-to-end benchmark on real songs. Not run by default: needs the model, network access
/// and a fixture. See `bench_fixture` for the fixture format.
#[cfg(all(test, feature = "wordsync"))]
mod bench {
    use super::*;
    use std::path::PathBuf;

    fn env(k: &str) -> Option<String> {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    }

    /// Reads/writes a 16 kHz stereo f32 cache file: [u32 frames][l...][r...].
    fn load_cache(p: &std::path::Path) -> Option<audio::Pcm> {
        let b = std::fs::read(p).ok()?;
        let n = u32::from_le_bytes(b[0..4].try_into().ok()?) as usize;
        let f = |o: usize| -> Vec<f32> { (0..n).map(|i| f32::from_le_bytes(b[4 + (o + i) * 4..8 + (o + i) * 4].try_into().unwrap())).collect() };
        Some(audio::Pcm { l: f(0), r: f(n) })
    }

    fn save_cache(p: &std::path::Path, pcm: &audio::Pcm) {
        let mut b = Vec::with_capacity(4 + pcm.l.len() * 8);
        b.extend_from_slice(&(pcm.l.len() as u32).to_le_bytes());
        for v in pcm.l.iter().chain(&pcm.r) {
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(p, b).unwrap();
    }

    fn fetch(remote_id: &str, creds: &str, duration_ms: u64) -> audio::Pcm {
        use crate::providers::Provider;
        let c: crate::sources::Credentials = serde_json::from_str(&std::fs::read_to_string(creds).unwrap()).unwrap();
        let y = crate::providers::yandex::Yandex::new(c.yandex_token);
        let o = crate::stream::open(y.stream(remote_id).unwrap()).unwrap();
        let mut b = rodio::Decoder::builder().with_data(o.reader).with_seekable(true).with_coarse_seek(true);
        if let Some(l) = o.byte_len {
            b = b.with_byte_len(l);
        }
        if let Some(m) = o.mime.as_deref() {
            b = b.with_mime_type(m);
        }
        audio::collect(b.build().unwrap(), duration_ms, &AtomicBool::new(false), &|_| {}).unwrap()
    }

    /// Fixture: JSON array of `{ "id": "yandex:123", "remoteId": "123", "title": "...",
    /// "durationMs": 207000, "doc": <cached LyricsDoc with human-timed synced lines> }`.
    ///
    /// OTO_WS_FIXTURE, OTO_WS_MODEL, OTO_WS_CACHE (dir), OTO_WS_CREDS (credentials.json);
    /// options: OTO_WS_DTW=0, OTO_WS_BEAM=1, OTO_WS_ONLY=<substring of title>.
    #[test]
    #[ignore = "needs a model, network and a fixture"]
    fn bench_fixture() {
        let fixture = env("OTO_WS_FIXTURE").expect("OTO_WS_FIXTURE");
        let model = PathBuf::from(env("OTO_WS_MODEL").expect("OTO_WS_MODEL"));
        let cache = PathBuf::from(env("OTO_WS_CACHE").expect("OTO_WS_CACHE"));
        let creds = env("OTO_WS_CREDS").unwrap_or_default();
        let dtw = env("OTO_WS_DTW").as_deref() != Some("0");
        let beam = env("OTO_WS_BEAM").as_deref() == Some("1");
        let only = env("OTO_WS_ONLY");
        let items: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let mut all_err: Vec<i64> = vec![];
        let (mut sum_secs, mut sum_audio) = (0f32, 0f32);
        for it in items {
            let title = it["title"].as_str().unwrap_or("").to_string();
            if let Some(o) = &only {
                if !title.contains(o.as_str()) {
                    continue;
                }
            }
            let remote = it["remoteId"].as_str().unwrap();
            let dur = it["durationMs"].as_u64().unwrap_or(200_000);
            let truth: LyricsDoc = serde_json::from_value(it["doc"].clone()).unwrap();
            let cf = cache.join(format!("{}.pcm", remote.replace([':', '/'], "_")));
            let pcm = load_cache(&cf).unwrap_or_else(|| {
                let p = fetch(remote, &creds, dur);
                save_cache(&cf, &p);
                p
            });
            let audio_s = pcm.secs();
            let input = pcm.mono();
            // Plain text in: no prior timing at all.
            let plain = LyricsDoc {
                kind: "plain".into(),
                lines: truth.lines.iter().map(|l| LyricLine { t: 0, d: None, text: l.text.clone(), words: None }).collect(),
                ..Default::default()
            };
            let texts: Vec<String> = plain.lines.iter().map(|l| l.text.clone()).collect();
            let mut prompt = |heard: &[RecWord]| chunk_prompt(&texts, heard);
            let opt = asr::Options { lang: lang_for(&plain.lines), dtw, beam, threads: asr::default_threads() };
            let t = Instant::now();
            let rec = asr::transcribe(&model, &input, &opt, &mut prompt, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
            let secs = t.elapsed().as_secs_f32();
            sum_secs += secs;
            sum_audio += audio_s;
            let res = align::align(&plain.lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(), &vec![None; plain.lines.len()], &rec);
            let mut errs: Vec<i64> = vec![];
            for (a, b) in res.lines.iter().zip(&truth.lines) {
                if b.text.trim().is_empty() {
                    continue;
                }
                errs.push(a.t as i64 - b.t as i64);
            }
            let mut abs: Vec<i64> = errs.iter().map(|e| e.abs()).collect();
            abs.sort();
            let pct = |ms: i64| 100.0 * abs.iter().filter(|e| **e <= ms).count() as f32 / abs.len().max(1) as f32;
            let mut signed = errs.clone();
            signed.sort();
            eprintln!(
                "{title:40} {audio_s:5.0}s audio, {secs:5.1}s asr ({:.2}x) | {} words heard | match {:4.0}% | line start err: median {:5} ms, signed median {:+5}, p90 {:5} | ≤300ms {:3.0}% ≤500ms {:3.0}% ≤1s {:3.0}%",
                secs / audio_s,
                rec.len(),
                res.match_rate() * 100.0,
                abs.get(abs.len() / 2).copied().unwrap_or(0),
                signed.get(signed.len() / 2).copied().unwrap_or(0),
                abs.get(abs.len() * 9 / 10).copied().unwrap_or(0),
                pct(300),
                pct(500),
                pct(1000)
            );
            if env("OTO_WS_DUMP").is_some() {
                for (a, b) in res.lines.iter().zip(&truth.lines) {
                    eprintln!("  {:7} {:7} {:+6} {}", b.t, a.t, a.t as i64 - b.t as i64, b.text);
                }
                let heard: Vec<String> = rec.iter().map(|r| format!("{}@{}", r.text, r.t0)).collect();
                eprintln!("  heard: {}", heard.join(" "));
            }
            all_err.extend(abs);
        }
        all_err.sort();
        let pct = |ms: i64| 100.0 * all_err.iter().filter(|e| **e <= ms).count() as f32 / all_err.len().max(1) as f32;
        eprintln!(
            "ALL: median {} ms, p90 {} ms, ≤300ms {:.0}%, ≤500ms {:.0}%, ≤1s {:.0}% | asr {:.1}s for {:.0}s audio ({:.2}x) | dtw={dtw} beam={beam} model={}",
            all_err.get(all_err.len() / 2).copied().unwrap_or(0),
            all_err.get(all_err.len() * 9 / 10).copied().unwrap_or(0),
            pct(300),
            pct(500),
            pct(1000),
            sum_secs,
            sum_audio,
            sum_secs / sum_audio.max(1.0),
            model.display()
        );
    }
}
