//! 10-band graphic equaliser (RBJ peaking biquads) with preamp and a soft limiter.
//! Parameters are shared atomics, so the UI can move sliders while audio plays.

use rodio::Source;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const BANDS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
const Q: f32 = 1.1;
const MAX_CH: usize = 8;

pub struct EqParams {
    enabled: AtomicBool,
    preamp_db: AtomicU32,
    gains_db: [AtomicU32; 10],
    version: AtomicU32,
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            preamp_db: AtomicU32::new(0f32.to_bits()),
            gains_db: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
            version: AtomicU32::new(0),
        }
    }
}

impl EqParams {
    pub fn set(&self, enabled: bool, preamp_db: f32, gains_db: &[f32]) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.preamp_db.store(preamp_db.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
        for (i, g) in gains_db.iter().take(10).enumerate() {
            self.gains_db[i].store(g.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
        }
        self.version.fetch_add(1, Ordering::Release);
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    fn peaking(f: f32, gain_db: f32, fs: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * (f / fs).min(0.49);
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * Q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cs) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cs) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

pub struct Eq<S: Source> {
    inner: S,
    params: Arc<EqParams>,
    seen: u32,
    active: Vec<(usize, Biquad)>,
    /// Direct-form-I state per band per channel: x1, x2, y1, y2.
    z: Vec<[[f32; 4]; MAX_CH]>,
    pre: f32,
    on: bool,
    ch: usize,
    fs: u32,
    tick: u32,
}

impl<S: Source> Eq<S> {
    pub fn new(inner: S, params: Arc<EqParams>) -> Self {
        let mut e = Self { inner, params, seen: u32::MAX, active: vec![], z: vec![], pre: 1.0, on: false, ch: 0, fs: 0, tick: 0 };
        e.refresh();
        e
    }

    fn refresh(&mut self) {
        let v = self.params.version.load(Ordering::Acquire);
        let fs = self.inner.sample_rate().get();
        if v == self.seen && fs == self.fs {
            return;
        }
        self.seen = v;
        self.fs = fs;
        self.on = self.params.enabled.load(Ordering::Relaxed);
        self.pre = 10f32.powf(f32::from_bits(self.params.preamp_db.load(Ordering::Relaxed)) / 20.0);
        self.active = BANDS
            .iter()
            .enumerate()
            .filter_map(|(i, &f)| {
                let g = f32::from_bits(self.params.gains_db[i].load(Ordering::Relaxed));
                (g.abs() > 0.05).then(|| (i, Biquad::peaking(f, g, fs as f32)))
            })
            .collect();
        if self.z.len() != BANDS.len() {
            self.z = vec![[[0.0; 4]; MAX_CH]; BANDS.len()];
        }
    }
}

#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 { x } else { x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh()) }
}

impl<S: Source> Iterator for Eq<S> {
    type Item = rodio::Sample;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let mut x = self.inner.next()?;
        self.tick = self.tick.wrapping_add(1);
        if self.tick % 1024 == 0 {
            self.refresh();
        }
        let chans = self.inner.channels().get() as usize;
        let c = self.ch.min(MAX_CH - 1);
        self.ch = (self.ch + 1) % chans.max(1);
        if !self.on {
            return Some(x);
        }
        x *= self.pre;
        for (band, q) in &self.active {
            let z = &mut self.z[*band][c];
            let y = q.b0 * x + q.b1 * z[0] + q.b2 * z[1] - q.a1 * z[2] - q.a2 * z[3];
            z[1] = z[0];
            z[0] = x;
            z[3] = z[2];
            z[2] = y;
            x = y;
        }
        Some(soft_clip(x))
    }
}

impl<S: Source> Source for Eq<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        // Clear filter memory so a seek doesn't click.
        for b in self.z.iter_mut() {
            *b = [[0.0; 4]; MAX_CH];
        }
        self.ch = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state gain in dB: output RMS over input RMS (sample peaks would
    /// under-read high frequencies that don't land on the waveform's crest).
    fn gain_at(f: f32, band_f: f32, g: f32) -> f32 {
        let fs = 48000.0;
        let q = Biquad::peaking(band_f, g, fs);
        let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
        let (mut ein, mut eout) = (0f64, 0f64);
        for n in 0..48000 {
            let x = (2.0 * std::f32::consts::PI * f * n as f32 / fs).sin();
            let y = q.b0 * x + q.b1 * x1 + q.b2 * x2 - q.a1 * y1 - q.a2 * y2;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
            if n > 24000 {
                ein += (x * x) as f64;
                eout += (y * y) as f64;
            }
        }
        (10.0 * (eout / ein).log10()) as f32
    }

    #[test]
    fn peaking_band_hits_its_gain_and_leaves_far_bands_alone() {
        assert!((gain_at(1000.0, 1000.0, 6.0) - 6.0).abs() < 0.3);
        assert!((gain_at(1000.0, 1000.0, -6.0) + 6.0).abs() < 0.3);
        assert!(gain_at(8000.0, 125.0, 9.0).abs() < 0.3);
    }
}
