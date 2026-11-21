//! Speech recognition with whisper.cpp: words with timestamps.
//!
//! Songs are hard on whisper's usual long-form mode: with the previous window's text as
//! context it easily locks into a loop ("СПОКОЙНАЯ МУЗЫКА" for the whole song). So we cut
//! the audio ourselves into ≤30 s chunks at quiet moments and decode each one on its own,
//! prompted with the lyrics around where the singer should be by then.

use super::align::RecWord;
use super::audio::SR;
use std::ffi::{c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use whisper_rs::{DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

pub struct Options<'a> {
    /// "ru", "en", … or "auto".
    pub lang: &'a str,
    /// Word timing from DTW over the alignment heads instead of token timestamps.
    pub dtw: bool,
    pub beam: bool,
    pub threads: usize,
}

/// DTW word starts land consistently late against human-timed LRC of the same recording
/// (median +340 ms over 5 songs, small model); pull them forward by most of that.
const DTW_LAG_MS: i64 = 250;

pub fn default_threads() -> usize {
    // whisper.cpp scales with physical cores; leave the rest for the UI and playback.
    let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (logical / 2).clamp(1, 8)
}

fn dtw_preset(model: &Path) -> Option<DtwModelPreset> {
    let name = model.file_name()?.to_string_lossy().to_lowercase();
    let en = name.contains(".en");
    Some(match () {
        _ if name.contains("tiny") => if en { DtwModelPreset::TinyEn } else { DtwModelPreset::Tiny },
        _ if name.contains("base") => if en { DtwModelPreset::BaseEn } else { DtwModelPreset::Base },
        _ if name.contains("small") => if en { DtwModelPreset::SmallEn } else { DtwModelPreset::Small },
        _ if name.contains("medium") => if en { DtwModelPreset::MediumEn } else { DtwModelPreset::Medium },
        _ if name.contains("turbo") => DtwModelPreset::LargeV3Turbo,
        _ if name.contains("large-v3") => DtwModelPreset::LargeV3,
        _ if name.contains("large-v2") => DtwModelPreset::LargeV2,
        _ => return None,
    })
}

/// Chunk boundaries (sample ranges), each ≤ 30 s, cut at the quietest moment of the last
/// third so that words are not split.
pub fn chunks(pcm: &[f32]) -> Vec<(usize, usize)> {
    const FRAME: usize = (SR / 20) as usize; // 50 ms
    let energy: Vec<f32> = pcm.chunks(FRAME).map(|c| c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).collect();
    // 300 ms moving average.
    let smooth: Vec<f32> = (0..energy.len())
        .map(|i| {
            let lo = i.saturating_sub(3);
            let hi = (i + 3).min(energy.len() - 1);
            energy[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
        })
        .collect();
    let max_len = 29 * SR as usize + SR as usize / 2;
    let mut out = vec![];
    let mut s = 0usize;
    while s < pcm.len() {
        if pcm.len() - s <= max_len {
            out.push((s, pcm.len()));
            break;
        }
        let (a, b) = ((s + 18 * SR as usize) / FRAME, (s + max_len) / FRAME);
        let cut = (a..b.min(smooth.len())).min_by(|&x, &y| smooth[x].partial_cmp(&smooth[y]).unwrap()).unwrap_or(b);
        let e = (cut * FRAME).clamp(s + SR as usize, s + max_len);
        out.push((s, e));
        s = e;
    }
    out
}

struct Callbacks<'a> {
    progress: &'a dyn Fn(f32),
    cancel: &'a AtomicBool,
    /// Progress range of the current chunk.
    from: f32,
    span: f32,
}

unsafe extern "C" fn on_progress(_: *mut whisper_rs::whisper_rs_sys::whisper_context, _: *mut whisper_rs::whisper_rs_sys::whisper_state, p: c_int, ud: *mut c_void) {
    // SAFETY: `ud` points at the `Callbacks` that outlives the `state.full` call.
    let cb = unsafe { &*(ud as *const Callbacks) };
    (cb.progress)((cb.from + cb.span * (p as f32 / 100.0).clamp(0.0, 1.0)).min(1.0));
}

unsafe extern "C" fn on_abort(ud: *mut c_void) -> bool {
    // SAFETY: as above.
    let cb = unsafe { &*(ud as *const Callbacks) };
    cb.cancel.load(Ordering::Relaxed)
}

/// Transcribes 16 kHz mono `pcm`. `prompt(heard_so_far)` gives the decoder prompt for the
/// next chunk. Returns None when cancelled.
pub fn transcribe(
    model: &Path,
    pcm: &[f32],
    opt: &Options,
    prompt: &mut dyn FnMut(&[RecWord]) -> String,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> anyhow::Result<Option<Vec<RecWord>>> {
    static LOGS: std::sync::Once = std::sync::Once::new();
    LOGS.call_once(whisper_rs::install_logging_hooks); // silences whisper.cpp's stderr chatter

    let mut cp = WhisperContextParameters::default();
    cp.use_gpu(false);
    let preset = if opt.dtw { dtw_preset(model) } else { None };
    let use_dtw = preset.is_some();
    if let Some(p) = preset {
        cp.dtw_parameters(DtwParameters { mode: DtwMode::ModelPreset { model_preset: p }, ..Default::default() });
    }
    let ctx = WhisperContext::new_with_params(model, cp).map_err(|e| anyhow::anyhow!("whisper model: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| anyhow::anyhow!("whisper state: {e}"))?;
    let eot = ctx.token_eot();

    let parts = chunks(pcm);
    let total = pcm.len().max(1) as f32;
    let mut heard: Vec<RecWord> = vec![];
    for &(s, e) in &parts {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        // Prompt budget is n_text_ctx / 2 = 224 tokens; keep the end (nearest to this chunk).
        let text: String = prompt(&heard).chars().filter(|c| *c != '\0').collect();
        let mut tokens = ctx.tokenize(&text, 4096).unwrap_or_default();
        if tokens.len() > 200 {
            tokens.drain(..tokens.len() - 200);
        }
        let mut p = if opt.beam {
            FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 })
        } else {
            FullParams::new(SamplingStrategy::Greedy { best_of: 1 })
        };
        p.set_n_threads(opt.threads as c_int);
        p.set_language(Some(opt.lang));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_token_timestamps(true);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        // No temperature fallback: on music it mostly produces confident nonsense, slowly.
        p.set_temperature(0.0);
        p.set_temperature_inc(0.0);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        if !tokens.is_empty() {
            p.set_tokens(&tokens);
        }
        let cbs = Callbacks { progress, cancel, from: s as f32 / total, span: (e - s) as f32 / total };
        // SAFETY: the callbacks only read `cbs`, which lives until after `state.full` returns.
        unsafe {
            p.set_progress_callback(Some(on_progress));
            p.set_progress_callback_user_data(&cbs as *const Callbacks as *mut c_void);
            p.set_abort_callback(Some(on_abort));
            p.set_abort_callback_user_data(&cbs as *const Callbacks as *mut c_void);
        }
        let res = state.full(p, &pcm[s..e]);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        res.map_err(|e| anyhow::anyhow!("whisper: {e}"))?;
        let offset_ms = (s as u64 * 1000 / SR as u64) as u32;
        let chunk_ms = ((e - s) as u64 * 1000 / SR as u64) as u32;
        for mut w in words(&state, eot, use_dtw) {
            if w.t0 >= chunk_ms + 500 {
                continue; // timestamps past the audio: hallucinated
            }
            w.t0 += offset_ms;
            w.t1 = (w.t1 + offset_ms).min(offset_ms + chunk_ms);
            heard.push(w);
        }
        progress((e as f32 / total).min(1.0));
    }
    Ok(Some(heard))
}

/// Tokens → words: a token starting with a space begins a new word. Bytes are collected per
/// word because Cyrillic letters may be split across tokens. Times are ms within the chunk.
fn words(state: &WhisperState, eot: c_int, use_dtw: bool) -> Vec<RecWord> {
    struct Acc {
        bytes: Vec<u8>,
        t0: i64,
        t1: i64,
        dtw: i64,
    }
    let mut acc: Vec<Acc> = vec![];
    for seg in state.as_iter() {
        for i in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(i) else { continue };
            if tok.token_id() >= eot {
                continue; // special and timestamp tokens
            }
            let Ok(bytes) = tok.to_bytes() else { continue };
            if bytes.is_empty() || bytes.starts_with(b"[_") {
                continue;
            }
            let d = tok.token_data();
            if bytes[0] == b' ' || acc.is_empty() {
                acc.push(Acc { bytes: bytes.to_vec(), t0: d.t0, t1: d.t1, dtw: d.t_dtw });
            } else if let Some(w) = acc.last_mut() {
                w.bytes.extend_from_slice(bytes);
                w.t1 = w.t1.max(d.t1);
            }
        }
    }
    let mut out = Vec::with_capacity(acc.len());
    for k in 0..acc.len() {
        let w = &acc[k];
        let text = String::from_utf8_lossy(&w.bytes).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let (mut t0, mut t1) = (w.t0 * 10, w.t1 * 10);
        if use_dtw && w.dtw >= 0 {
            t0 = w.dtw * 10 - DTW_LAG_MS;
            // A word lasts until the next one starts (capped later), or its own token end.
            let next = acc.get(k + 1).filter(|n| n.dtw >= 0).map(|n| n.dtw * 10 - DTW_LAG_MS);
            t1 = match next {
                Some(n) if n > t0 => n.min(t0.max(t1)).max(t0 + 80),
                _ => t1.max(t0 + 200),
            };
        }
        let t0 = t0.max(0);
        out.push(RecWord { t0: t0 as u32, t1: t1.max(t0 + 1) as u32, text });
    }
    drop_collapsed(out)
}

/// DTW sometimes piles a run of words onto one instant (typically text the decoder made up
/// at the end of a chunk, or a stretch it lost track of). Such times are meaningless: keep
/// the first word of the run and drop the rest.
pub fn drop_collapsed(words: Vec<RecWord>) -> Vec<RecWord> {
    let mut keep = vec![true; words.len()];
    let mut i = 0;
    while i < words.len() {
        let mut j = i + 1;
        while j < words.len() && words[j].t0.abs_diff(words[j - 1].t0) < 40 {
            j += 1;
        }
        if j - i >= 3 {
            keep[i + 1..j].iter_mut().for_each(|k| *k = false);
        }
        i = j;
    }
    words.into_iter().zip(keep).filter_map(|(w, k)| k.then_some(w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_audio_and_cut_in_quiet_spots() {
        // 70 s of noise with a silent gap at 25–26 s and 50–51 s.
        let n = 70 * SR as usize;
        let mut seed = 1u32;
        let pcm: Vec<f32> = (0..n)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let t = i as f32 / SR as f32;
                let quiet = (25.0..26.0).contains(&t) || (50.0..51.0).contains(&t);
                if quiet { 0.0 } else { (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5 }
            })
            .collect();
        let c = chunks(&pcm);
        assert_eq!(c.first().unwrap().0, 0);
        assert_eq!(c.last().unwrap().1, n);
        for w in c.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        for (s, e) in &c {
            assert!(e - s <= 30 * SR as usize);
        }
        let cut = c[0].1 as f32 / SR as f32;
        assert!((25.0..26.0).contains(&cut), "cut at {cut}");
    }
}
