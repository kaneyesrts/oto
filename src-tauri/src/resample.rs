//! High-quality sample-rate conversion.
//!
//! Windows mixes at the device rate (usually 48 kHz) while most music is 44.1 kHz. rodio's
//! built-in converter interpolates linearly, which dulls the top end and folds images back
//! into the audible band. This is a polyphase windowed-sinc resampler (Kaiser window,
//! 64 taps, 512 phases with linear interpolation between them): flat to ~20 kHz and
//! about −100 dB of aliasing, at a cost of ~64 multiply-adds per output sample per channel.

use rodio::Source;
use std::sync::OnceLock;
use std::time::Duration;

const TAPS: usize = 64; // total taps per phase
const HALF: usize = TAPS / 2;
const PHASES: usize = 512;
const BETA: f64 = 9.0;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    let q = x * x / 4.0;
    for k in 1..50 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// Filter table for a given cutoff (fraction of the input Nyquist), PHASES+1 rows so the
/// interpolation between phases never reads past the end.
fn table(cutoff: f64) -> Vec<f32> {
    let mut t = vec![0f32; (PHASES + 1) * TAPS];
    let i0b = bessel_i0(BETA);
    for p in 0..=PHASES {
        let frac = p as f64 / PHASES as f64;
        let mut sum = 0.0;
        let row = &mut t[p * TAPS..(p + 1) * TAPS];
        for (k, v) in row.iter_mut().enumerate() {
            // Tap k sits at input offset (k - HALF + 1); distance from the output point:
            let x = (k as f64 - HALF as f64 + 1.0) - frac;
            let s = if x.abs() < 1e-9 { cutoff } else { (std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x) };
            let r = x / HALF as f64;
            let w = if r.abs() >= 1.0 { 0.0 } else { bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0b };
            *v = (s * w) as f32;
            sum += s * w;
        }
        // Unity DC gain for every phase.
        for v in row.iter_mut() {
            *v /= sum as f32;
        }
    }
    t
}

fn cached_table(cutoff_key: u32) -> &'static [f32] {
    use parking_lot::Mutex;
    use std::collections::HashMap;
    static CACHE: OnceLock<Mutex<HashMap<u32, &'static [f32]>>> = OnceLock::new();
    let m = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = m.lock();
    *g.entry(cutoff_key).or_insert_with(|| Box::leak(table(cutoff_key as f64 / 10_000.0).into_boxed_slice()))
}

pub struct Resample<S: Source> {
    inner: S,
    ch: usize,
    in_rate: u32,
    out_rate: u32,
    /// Input frames → output frames ratio.
    step: f64,
    /// Position of the next output sample, in input frames, relative to `hist[HALF-1]`.
    pos: f64,
    /// Sliding window of input frames, interleaved (TAPS frames).
    hist: Vec<f32>,
    out: Vec<f32>,
    out_i: usize,
    tbl: &'static [f32],
    ended: usize,
    /// Rates already match: pass samples straight through.
    bypass: bool,
}

impl<S: Source> Resample<S> {
    pub fn new(inner: S, out_rate: u32) -> Self {
        let in_rate = inner.sample_rate().get();
        let ch = inner.channels().get() as usize;
        // Downsampling: cut below the new Nyquist. Upsampling: keep the full band (95%).
        let cutoff = (out_rate as f64 / in_rate as f64).min(1.0) * 0.95;
        let mut r = Self {
            inner,
            ch,
            in_rate,
            out_rate,
            step: in_rate as f64 / out_rate as f64,
            pos: 0.0,
            hist: vec![0.0; TAPS * ch],
            out: Vec::with_capacity(ch),
            out_i: 0,
            tbl: cached_table((cutoff * 10_000.0).round() as u32),
            ended: 0,
            bypass: in_rate == out_rate,
        };
        if r.bypass {
            return r;
        }
        // Prime the window so the first output aligns with the first input frame.
        for _ in 0..HALF {
            r.push_frame();
        }
        r
    }

    /// Shifts one input frame into the window. Returns false once the input is exhausted
    /// and the window has been flushed with zeros.
    fn push_frame(&mut self) -> bool {
        self.hist.copy_within(self.ch.., 0);
        let base = (TAPS - 1) * self.ch;
        let mut got = 0;
        for c in 0..self.ch {
            match self.inner.next() {
                Some(v) => {
                    self.hist[base + c] = v;
                    got += 1;
                }
                None => self.hist[base + c] = 0.0,
            }
        }
        if got == 0 {
            self.ended += 1;
        }
        self.ended <= HALF
    }

    fn produce(&mut self) -> bool {
        while self.pos >= 1.0 {
            if !self.push_frame() {
                return false;
            }
            self.pos -= 1.0;
        }
        let ph = self.pos * PHASES as f64;
        let p0 = ph.floor() as usize;
        let t = (ph - p0 as f64) as f32;
        let a = &self.tbl[p0 * TAPS..(p0 + 1) * TAPS];
        let b = &self.tbl[(p0 + 1) * TAPS..(p0 + 2) * TAPS];
        self.out.clear();
        for c in 0..self.ch {
            let mut acc = 0f32;
            for k in 0..TAPS {
                let w = a[k] + (b[k] - a[k]) * t;
                acc += w * self.hist[k * self.ch + c];
            }
            self.out.push(acc);
        }
        self.out_i = 0;
        self.pos += self.step;
        true
    }
}

impl<S: Source> Iterator for Resample<S> {
    type Item = rodio::Sample;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.bypass {
            return self.inner.next();
        }
        if self.out_i >= self.out.len() && !self.produce() {
            return None;
        }
        let v = self.out[self.out_i];
        self.out_i += 1;
        Some(v)
    }
}

impl<S: Source> Source for Resample<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(self.ch as u16).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(self.out_rate).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        if self.bypass {
            return Ok(());
        }
        self.hist.iter_mut().for_each(|v| *v = 0.0);
        self.pos = 0.0;
        self.out.clear();
        self.out_i = 0;
        self.ended = 0;
        for _ in 0..HALF {
            self.push_frame();
        }
        let _ = self.in_rate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sine {
        f: f32,
        sr: u32,
        n: usize,
        len: usize,
    }
    impl Iterator for Sine {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.n >= self.len {
                return None;
            }
            let v = ((2.0 * std::f64::consts::PI * self.f as f64 * self.n as f64 / self.sr as f64).sin() * 0.5) as f32;
            self.n += 1;
            Some(v)
        }
    }
    impl Source for Sine {
        fn current_span_len(&self) -> Option<usize> { None }
        fn channels(&self) -> rodio::ChannelCount { std::num::NonZero::new(1).unwrap() }
        fn sample_rate(&self) -> rodio::SampleRate { std::num::NonZero::new(self.sr).unwrap() }
        fn total_duration(&self) -> Option<Duration> { None }
    }

    /// Converts a 44.1 kHz sine to 48 kHz and checks the result against an ideal 48 kHz
    /// sine: the error (noise + distortion) must be far below the signal.
    fn snr_db(f: f32) -> f64 {
        let src = Sine { f, sr: 44_100, n: 0, len: 44_100 };
        snr_of(&Resample::new(src, 48_000).collect::<Vec<f32>>(), f)
    }

    fn snr_of(out: &[f32], f: f32) -> f64 {
        // Skip filter edges; fit amplitude/phase by least squares on sin/cos.
        let s = &out[4800..43200];
        let (mut ss, mut sc, mut cc, mut ys, mut yc) = (0f64, 0f64, 0f64, 0f64, 0f64);
        for (i, &y) in s.iter().enumerate() {
            let t = 2.0 * std::f64::consts::PI * f as f64 * (i + 4800) as f64 / 48_000.0;
            let (sn, cs) = t.sin_cos();
            ss += sn * sn; sc += sn * cs; cc += cs * cs; ys += y as f64 * sn; yc += y as f64 * cs;
        }
        let det = ss * cc - sc * sc;
        let (a, b) = ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det);
        let (mut sig, mut err) = (0f64, 0f64);
        for (i, &y) in s.iter().enumerate() {
            let t = 2.0 * std::f64::consts::PI * f as f64 * (i + 4800) as f64 / 48_000.0;
            let ideal = a * t.sin() + b * t.cos();
            sig += ideal * ideal;
            err += (y as f64 - ideal).powi(2);
        }
        10.0 * (sig / err).log10()
    }

    #[test]
    fn clean_conversion_across_the_band() {
        for f in [100.0, 1000.0, 10_000.0, 18_000.0] {
            let snr = snr_db(f);
            println!("{f} Hz: SNR {snr:.1} dB");
            assert!(snr > 80.0, "{f} Hz: {snr:.1} dB");
        }
    }

    /// For comparison: what linear interpolation (rodio's converter) gives.
    #[test]
    fn linear_interpolation_reference() {
        for f in [1000.0f32, 10_000.0, 18_000.0] {
            let src: Vec<f32> = Sine { f, sr: 44_100, n: 0, len: 44_100 }.collect();
            let out: Vec<f32> = (0..48_000)
                .map(|i| {
                    let x = i as f64 * 44_100.0 / 48_000.0;
                    let k = x.floor() as usize;
                    let t = (x - k as f64) as f32;
                    let a = src.get(k).copied().unwrap_or(0.0);
                    let b = src.get(k + 1).copied().unwrap_or(0.0);
                    a + (b - a) * t
                })
                .collect();
            println!("linear {f} Hz: SNR {:.1} dB", snr_of(&out, f));
        }
    }

    #[test]
    fn length_follows_rate() {
        let out: Vec<f32> = Resample::new(Sine { f: 440.0, sr: 44_100, n: 0, len: 44_100 }, 48_000).collect();
        assert!((out.len() as i64 - 48_000).abs() < 80, "{}", out.len());
    }
}
