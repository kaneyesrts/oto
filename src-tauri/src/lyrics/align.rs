//! Fitting lyrics timing to the audio without any ML: we decode the track once,
//! track energy in the vocal band (300–3400 Hz), and use vocal onsets as anchors.
//!
//! * `fit` — synced lyrics from another cut of the song: search shift and tempo scale
//!   that best line up line starts with vocal onsets. Only applied on a clear win.
//! * `estimate` — plain text only: spread lines over the stretches where vocals are present,
//!   proportionally to line length. Marked as approximate in the UI.

use super::{lrc, LyricLine};
use realfft::RealFftPlanner;
use rodio::Source;
use std::fs::File;

pub struct Features {
    pub fps: f32,
    pub onset: Vec<f32>,
    pub vocal: Vec<f32>,
}

const N: usize = 1024;
const HOP: usize = 256;

pub fn analyse(path: &str) -> Option<Features> {
    let dec = rodio::Decoder::try_from(File::open(path).ok()?).ok()?;
    let ch = dec.channels().get() as usize;
    let sr = dec.sample_rate().get() as f32;
    // Downmix and halve the rate when it is high: the vocal band sits well below 11 kHz.
    let decim = if sr > 32000.0 { 2 } else { 1 };
    let eff_sr = sr / decim as f32;
    let mut mono = Vec::with_capacity((sr as usize / decim) * 300);
    let (mut acc, mut n, mut k) = (0f32, 0usize, 0usize);
    for s in dec {
        acc += s;
        n += 1;
        if n == ch * decim {
            mono.push(acc / n as f32);
            acc = 0.0;
            n = 0;
        }
        k += 1;
        if k > (sr as usize) * ch * 60 * 15 {
            break; // 15 minutes is plenty
        }
    }
    if mono.len() < N * 4 {
        return None;
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let mut input = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
    let bin = eff_sr / N as f32;
    let (b0, b1) = ((300.0 / bin) as usize, (3400.0 / bin) as usize);

    let frames = (mono.len() - N) / HOP;
    let mut vocal = Vec::with_capacity(frames);
    for f in 0..frames {
        for i in 0..N {
            input[i] = mono[f * HOP + i] * win[i];
        }
        if fft.process(&mut input, &mut spec).is_err() {
            return None;
        }
        let e: f32 = spec[b0..=b1].iter().map(|c| c.norm_sqr()).sum();
        vocal.push(e);
    }

    let log: Vec<f32> = vocal.iter().map(|e| (1.0 + e * 1e3).ln()).collect();
    let mut onset = vec![0f32; frames];
    for i in 3..frames {
        // Compare to the recent past, not just the previous frame, to catch soft entries.
        let past = (log[i - 3] + log[i - 2] + log[i - 1]) / 3.0;
        onset[i] = (log[i] - past).max(0.0);
    }
    Some(Features { fps: eff_sr / HOP as f32, onset, vocal })
}

fn percentile(v: &[f32], p: f32) -> f32 {
    let mut s: Vec<f32> = v.iter().cloned().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f32 * p) as usize]
}

/// Returns `(shift_ms, scale)` to apply as `t' = t * scale + shift`, or None if no clear improvement.
pub fn fit(starts_ms: &[u32], f: &Features, mismatch: bool) -> Option<(i32, f32)> {
    if starts_ms.len() < 6 {
        return None;
    }
    // Widen each onset peak a little: line timestamps are human-made and imprecise.
    let n = f.onset.len();
    let mut env = vec![0f32; n];
    for i in 0..n {
        let lo = i.saturating_sub(3);
        let hi = (i + 3).min(n - 1);
        env[i] = f.onset[lo..=hi].iter().cloned().fold(0.0, f32::max);
    }
    let p = percentile(&env, 0.98).max(1e-6);
    for e in env.iter_mut() {
        *e = (*e / p).min(1.0);
    }
    let score = |shift_s: f32, scale: f32| -> f32 {
        let mut s = 0.0;
        for &t in starts_ms {
            let x = ((t as f32 / 1000.0) * scale + shift_s) * f.fps;
            if x >= 0.0 && (x as usize) < n {
                s += env[x as usize];
            }
        }
        s / starts_ms.len() as f32
    };

    let base = score(0.0, 1.0);
    let (range, scales): (f32, Vec<f32>) = if mismatch {
        (12.0, (0..=12).map(|i| 0.97 + i as f32 * 0.005).collect())
    } else {
        (1.5, vec![1.0])
    };
    let mut best = (base, 0.0f32, 1.0f32);
    for &a in &scales {
        let mut s = -range;
        while s <= range {
            let v = score(s, a);
            if v > best.0 {
                best = (v, s, a);
            }
            s += 0.04;
        }
    }
    let (v, shift, scale) = best;
    let need = if mismatch { 1.2 } else { 1.35 };
    let meaningful = shift.abs() > 0.15 || (scale - 1.0).abs() > 0.001;
    if meaningful && v > base * need && v - base > 0.06 {
        Some(((shift * 1000.0) as i32, scale))
    } else {
        None
    }
}

/// Distributes plain lines across detected vocal regions.
pub fn estimate(raw: &[String], f: &Features) -> Vec<LyricLine> {
    let lines: Vec<&String> = raw.iter().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return vec![];
    }
    let n = f.vocal.len();
    let w = (f.fps * 0.5) as usize;
    let mut smooth = vec![0f32; n];
    let mut acc = 0f32;
    for i in 0..n {
        acc += f.vocal[i];
        if i >= w {
            acc -= f.vocal[i - w];
        }
        smooth[i] = acc / w.min(i + 1) as f32;
    }
    let th = percentile(&smooth, 0.4);
    let active: Vec<bool> = smooth.iter().map(|v| *v > th).collect();

    // Runs of activity, merging short gaps and dropping blips.
    let mut segs: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < n {
        if active[i] {
            let s = i;
            while i < n && active[i] {
                i += 1;
            }
            if let Some(last) = segs.last_mut() {
                if (s - last.1) as f32 / f.fps < 1.0 {
                    last.1 = i;
                    continue;
                }
            }
            segs.push((s, i));
        } else {
            i += 1;
        }
    }
    segs.retain(|(a, b)| (b - a) as f32 / f.fps > 0.8);
    let total: usize = segs.iter().map(|(a, b)| b - a).sum();
    if (total as f32 / f.fps) < 20.0 {
        return vec![];
    }

    let weights: Vec<f32> = lines.iter().map(|l| l.chars().count().max(6) as f32 + 4.0).collect();
    let wsum: f32 = weights.iter().sum();
    let at_active = |k: f32| -> u32 {
        // Map a position along concatenated active time to a real frame index.
        let mut left = k * total as f32;
        for (a, b) in &segs {
            let len = (b - a) as f32;
            if left <= len {
                return (((*a as f32 + left) / f.fps) * 1000.0) as u32;
            }
            left -= len;
        }
        ((segs.last().unwrap().1 as f32 / f.fps) * 1000.0) as u32
    };
    let mut out = vec![];
    let mut cum = 0f32;
    for (l, wt) in lines.iter().zip(&weights) {
        out.push(LyricLine { t: at_active(cum / wsum), d: None, text: l.trim().to_string(), words: None });
        cum += wt;
    }
    lrc::fill_durations(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a mono 22.05 kHz WAV with 1 kHz "vocal" bursts starting at `starts_s`.
    fn synth(path: &std::path::Path, starts_s: &[f32], len_s: f32) {
        let sr = 22050u32;
        let n = (len_s * sr as f32) as usize;
        let mut pcm = vec![0i16; n];
        for &s in starts_s {
            let a = (s * sr as f32) as usize;
            for i in 0..(sr as usize * 2).min(n.saturating_sub(a)) {
                let t = i as f32 / sr as f32;
                let env = (t * 30.0).min(1.0) * (-t * 1.2).exp();
                pcm[a + i] = ((2.0 * std::f32::consts::PI * 1000.0 * t).sin() * env * 12000.0) as i16;
            }
        }
        let mut b = Vec::with_capacity(44 + n * 2);
        let data_len = (n * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn fits_shifted_lyrics_to_vocal_onsets() {
        let starts: Vec<f32> = (0..14).map(|i| 6.0 + i as f32 * 3.7 + (i % 3) as f32 * 0.6).collect();
        let path = std::env::temp_dir().join("oto_align_test.wav");
        synth(&path, &starts, 70.0);
        let f = analyse(path.to_str().unwrap()).expect("decode");
        // Lyrics timed against a cut with 3 s less intro.
        let lrc: Vec<u32> = starts.iter().map(|s| ((s - 3.0) * 1000.0) as u32).collect();
        let (shift, scale) = fit(&lrc, &f, true).expect("should find a fit");
        assert!((shift - 3000).abs() < 150, "shift {shift}");
        assert!((scale - 1.0).abs() < 0.006, "scale {scale}");
        // Already aligned lyrics must be left alone.
        let ok: Vec<u32> = starts.iter().map(|s| (s * 1000.0) as u32).collect();
        assert!(fit(&ok, &f, false).is_none());
    }
}
