//! Audio for the recogniser: the whole track decoded to 16 kHz.
//!
//! Vocal emphasis (centre-channel extraction + 150–4000 Hz band-pass) was tried and dropped:
//! it lowered the share of lyric words matched (93 % → 57 %, 57 % → 43 % on two test songs).

use rodio::Source;
use std::sync::atomic::{AtomicBool, Ordering};

pub const SR: u32 = 16_000;

/// Stereo at 16 kHz (mono sources have `l == r`).
pub struct Pcm {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Pcm {
    #[cfg(test)]
    pub fn secs(&self) -> f32 {
        self.l.len() as f32 / SR as f32
    }

    pub fn mono(&self) -> Vec<f32> {
        self.l.iter().zip(&self.r).map(|(a, b)| 0.5 * (a + b)).collect()
    }
}

/// Decodes any rodio source to 16 kHz stereo through the polyphase sinc resampler.
/// `expected_ms` drives `progress` (0..1). Returns None when cancelled or empty.
pub fn collect<S: Source + Send>(src: S, expected_ms: u64, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Option<Pcm> {
    let ch = src.channels().get() as usize;
    let rs = crate::resample::Resample::new(src, SR);
    let cap = (expected_ms.max(1000) as usize * SR as usize / 1000) + SR as usize;
    let (mut l, mut r) = (Vec::with_capacity(cap), Vec::with_capacity(cap));
    let expected = (expected_ms as f32 / 1000.0 * SR as f32).max(1.0);
    let mut frame = Vec::with_capacity(ch);
    for s in rs {
        frame.push(s);
        if frame.len() < ch {
            continue;
        }
        l.push(frame[0]);
        r.push(if ch > 1 { frame[1] } else { frame[0] });
        frame.clear();
        if l.len() % (SR as usize * 5) == 0 {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            progress((l.len() as f32 / expected).min(1.0));
        }
        if l.len() > SR as usize * 60 * 20 {
            break; // 20 minutes is plenty
        }
    }
    if l.len() < SR as usize * 5 {
        return None;
    }
    progress(1.0);
    Some(Pcm { l, r })
}

/// Opens a track (local file or network stream) as a rodio decoder and decodes it.
pub fn decode_track(app: &tauri::AppHandle, track: &crate::db::Track, cancel: &AtomicBool, progress: &dyn Fn(f32)) -> anyhow::Result<Option<Pcm>> {
    if track.source == "local" {
        let dec = rodio::Decoder::try_from(std::fs::File::open(&track.path)?)?;
        return Ok(collect(dec, track.duration_ms, cancel, progress));
    }
    let src = crate::sources::stream_for(app, track)?;
    if matches!(src, crate::providers::StreamSource::Spotify { .. }) {
        anyhow::bail!("Spotify audio cannot be decoded outside the player");
    }
    let opened = crate::stream::open(src)?;
    let mut b = rodio::Decoder::builder().with_data(opened.reader).with_seekable(true).with_coarse_seek(true);
    if let Some(len) = opened.byte_len {
        b = b.with_byte_len(len);
    }
    if let Some(m) = opened.mime.as_deref() {
        b = b.with_mime_type(m);
    }
    let dec = b.build()?;
    Ok(collect(dec, track.duration_ms, cancel, progress))
}
