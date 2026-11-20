//! Playback engine. One thread owns the output device and the queue; everything else
//! talks to it over a channel, so the tray, hotkeys, SMTC and UI all share one source of truth.
//! A tap inside the decode chain feeds the analyser, so the visuals react to every source
//! the same way.

use crate::db::{now_ms, Db, Track};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::{Mutex, RwLock};
use realfft::RealFftPlanner;
use rodio::{DeviceSinkBuilder, Player, Source};
use serde::Serialize;
use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug)]
pub enum Cmd {
    PlayList { ids: Vec<String>, start: usize },
    Enqueue(Vec<String>),
    PlayNext(Vec<String>),
    JumpTo(usize),
    Toggle,
    Play,
    Pause,
    Next,
    Prev,
    Seek(u64),
    SeekBy(i64),
    Volume(f32),
    VolumeDelta(f32),
    Shuffle(bool),
    Repeat(String),
    RefreshTrack(String),
    QueueRemove(usize),
    QueueMove { from: usize, to: usize },
    QueueClear,
    Audio(crate::settings::AudioSettings),
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub track: Option<Track>,
    pub playing: bool,
    pub pos_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: String,
    pub queue: Vec<String>,
    pub index: usize,
    /// Wall-clock ms when `pos_ms` was sampled; the UI extrapolates from it.
    pub at: i64,
    /// Waiting for a network stream to start.
    pub buffering: bool,
}

#[derive(Serialize, Clone)]
pub struct SpectrumFrame {
    /// 48 log-spaced bands, 0–255.
    pub b: Vec<u8>,
    pub lvl: f32,
    pub bass: f32,
    pub mid: f32,
    pub high: f32,
    pub beat: f32,
    pub pos: u64,
}

pub struct Audio {
    tx: Sender<Cmd>,
    pub state: Arc<RwLock<PlayerState>>,
}

/// A track opened ahead of time for gapless or crossfade.
struct Prepared {
    index: usize,
    track: Track,
    sample_rate: u32,
}

impl Audio {
    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }
    pub fn snapshot(&self) -> PlayerState {
        self.state.read().clone()
    }
}

// ---------- analysis tap ----------

const RING: usize = 4096;

pub struct Ring {
    data: Mutex<(Vec<f32>, usize)>,
    sr: AtomicU32,
    fresh: AtomicBool,
}

impl Ring {
    fn new() -> Self {
        Self { data: Mutex::new((vec![0.0; RING], 0)), sr: AtomicU32::new(44100), fresh: AtomicBool::new(false) }
    }
    fn push(&self, s: &[f32]) {
        let mut g = self.data.lock();
        let (buf, w) = &mut *g;
        for &x in s {
            buf[*w] = x;
            *w = (*w + 1) % RING;
        }
        self.fresh.store(true, Ordering::Relaxed);
    }
    fn latest(&self, out: &mut [f32]) {
        let g = self.data.lock();
        let (buf, w) = &*g;
        let n = out.len();
        for (i, o) in out.iter_mut().enumerate() {
            *o = buf[(w + RING - n + i) % RING];
        }
    }
}

struct Tap<S: Source> {
    inner: S,
    ring: Arc<Ring>,
    acc: f32,
    n: u16,
    local: Vec<f32>,
}

impl<S: Source> Iterator for Tap<S> {
    type Item = rodio::Sample;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let s = self.inner.next()?;
        self.acc += s;
        self.n += 1;
        if self.n >= self.inner.channels().get() {
            self.local.push(self.acc / self.n as f32);
            self.acc = 0.0;
            self.n = 0;
            if self.local.len() >= 256 {
                self.ring.push(&self.local);
                self.local.clear();
            }
        }
        Some(s)
    }
}

impl<S: Source> Source for Tap<S> {
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
        self.inner.try_seek(pos)
    }
}

// ---------- engine ----------

struct Engine {
    app: AppHandle,
    state: Arc<RwLock<PlayerState>>,
    ring: Arc<Ring>,
    sink: rodio::MixerDeviceSink,
    device_lost: Arc<AtomicBool>,
    player: Option<Player>,
    original_queue: Option<Vec<String>>,
    /// Set when the volume changes; persisted shortly after the user stops dragging.
    volume_dirty: Option<Instant>,
    eq: Arc<crate::eq::EqParams>,
    fx: crate::settings::AudioSettings,
    /// Next track already appended to the current player (gapless).
    prepared: Option<Prepared>,
    /// Outgoing player during a crossfade, with when the fade started.
    fading_out: Option<(Player, Instant)>,
    /// When the current player's fade-in started (crossfade).
    fading_in: Option<Instant>,
    // play accounting
    started_at: i64,
    listened_ms: u64,
    last_tick: Instant,
}

pub fn start(app: AppHandle, volume: f32) -> Audio {
    let (tx, rx) = unbounded();
    let state = Arc::new(RwLock::new(PlayerState { volume, repeat: "off".into(), ..Default::default() }));
    let ring = Arc::new(Ring::new());

    {
        let (app, state, ring) = (app.clone(), state.clone(), ring.clone());
        std::thread::Builder::new()
            .name("oto-audio".into())
            .spawn(move || {
                let device_lost = Arc::new(AtomicBool::new(false));
                let Some(sink) = open_sink(&device_lost) else {
                    eprintln!("audio output unavailable");
                    return;
                };
                let mut eng = Engine {
                    app,
                    state,
                    ring,
                    sink,
                    device_lost,
                    volume_dirty: None,
                    eq: Arc::new(crate::eq::EqParams::default()),
                    fx: crate::settings::AudioSettings::default(),
                    prepared: None,
                    fading_out: None,
                    fading_in: None,
                    player: None,
                    original_queue: None,
                    started_at: 0,
                    listened_ms: 0,
                    last_tick: Instant::now(),
                };
                eng.run(rx);
            })
            .expect("spawn audio thread");
    }
    {
        let (app, state) = (app.clone(), state.clone());
        std::thread::Builder::new().name("oto-analyser".into()).spawn(move || analyser(app, state, ring)).ok();
    }
    Audio { tx, state }
}

/// Opens the default output and flags `lost` when the stream dies (device unplugged,
/// default device switched), so the engine can move playback to the new device instead of
/// hanging: a dead stream never processes seek orders, so `try_seek` would block forever.
fn open_sink(lost: &Arc<AtomicBool>) -> Option<rodio::MixerDeviceSink> {
    let flag = lost.clone();
    let builder = DeviceSinkBuilder::from_default_device().ok()?.with_error_callback(move |e| {
        eprintln!("audio stream error: {e}");
        flag.store(true, Ordering::Relaxed);
    });
    let mut sink = builder.open_sink_or_fallback().ok()?;
    sink.log_on_drop(false);
    lost.store(false, Ordering::Relaxed);
    Some(sink)
}

impl Engine {
    /// Re-opens output on the current default device and resumes where we were.
    fn recover_device(&mut self) {
        let Some(sink) = open_sink(&self.device_lost) else { return };
        self.player = None;
        self.sink = sink;
        let (idx, pos, playing, has) = {
            let s = self.state.read();
            (s.index, s.pos_ms, s.playing, s.track.is_some())
        };
        if has {
            let (listened, started) = (self.listened_ms, self.started_at);
            self.load(idx, playing);
            if let Some(p) = &self.player {
                let _ = p.try_seek(Duration::from_millis(pos));
            }
            self.state.write().pos_ms = pos;
            self.listened_ms = listened;
            self.started_at = started;
        }
        self.emit();
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        let mut last_emit = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(cmd) => {
                    self.handle(cmd);
                    self.emit();
                    last_emit = Instant::now();
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.device_lost.load(Ordering::Relaxed) {
                self.recover_device();
            }
            self.tick();
            if self.volume_dirty.is_some_and(|t| t.elapsed() > Duration::from_millis(700)) {
                self.volume_dirty = None;
                crate::shell::persist(&self.app);
            }
            if self.state.read().playing && last_emit.elapsed() > Duration::from_millis(1000) {
                self.emit();
                last_emit = Instant::now();
            }
        }
    }

    fn db(&self) -> tauri::State<'_, Db> {
        self.app.state::<Db>()
    }

    fn tick(&mut self) {
        let dt = self.last_tick.elapsed().as_millis() as u64;
        self.last_tick = Instant::now();
        self.ramp();
        // Gapless hand-over: the prepared source started, the old one is gone.
        if self.prepared.is_some() && self.player.as_ref().is_some_and(|p| p.len() <= 1) {
            let prep = self.prepared.take().unwrap();
            self.account(false);
            self.ring.sr.store(prep.sample_rate, Ordering::Relaxed);
            {
                let mut s = self.state.write();
                s.duration_ms = prep.track.duration_ms;
                s.track = Some(prep.track);
                s.index = prep.index;
                s.pos_ms = 0;
            }
            self.started_at = now_ms();
            self.emit();
        }
        let Some(p) = &self.player else { return };
        let playing = !p.is_paused();
        let pos = p.get_pos().as_millis() as u64;
        {
            let mut s = self.state.write();
            s.pos_ms = pos.min(s.duration_ms.max(pos));
            s.at = now_ms();
        }
        if playing {
            self.listened_ms += dt;
        }
        if p.empty() {
            self.on_track_end();
            return;
        }
        if playing {
            self.maybe_prepare_next(pos);
        }
    }

    /// What plays after the current track, honouring repeat.
    fn next_index(&self) -> Option<usize> {
        let s = self.state.read();
        match s.repeat.as_str() {
            "one" => Some(s.index),
            _ if s.index + 1 < s.queue.len() => Some(s.index + 1),
            "all" if !s.queue.is_empty() => Some(0),
            _ => None,
        }
    }

    /// Near the end of a track: start the crossfade, or append the next track for gapless.
    fn maybe_prepare_next(&mut self, pos: u64) {
        if self.prepared.is_some() || self.fading_out.is_some() {
            return;
        }
        let dur = self.state.read().duration_ms;
        if dur < 5000 || pos == 0 {
            return;
        }
        let remaining = dur.saturating_sub(pos);
        let cf = self.fx.crossfade_ms as u64;
        let Some(next) = self.next_index() else { return };
        let spotify_involved = self.state.read().track.as_ref().is_some_and(|t| t.source == "spotify")
            || self.state.read().queue.get(next).is_some_and(|id| id.starts_with("spotify:"));
        if cf > 0 && !spotify_involved {
            if remaining <= cf.min(dur / 3) {
                // Crossfade: keep the old player running and fade it out while the next fades in.
                self.account(false);
                if let Some(old) = self.player.take() {
                    self.fading_out = Some((old, Instant::now()));
                }
                self.load(next, true);
                if let Some(p) = &self.player {
                    p.set_volume(0.0);
                }
                self.fading_in = Some(Instant::now());
                self.emit();
            }
            return;
        }
        if self.fx.gapless && remaining <= 1800 {
            let id = self.state.read().queue.get(next).cloned();
            let Some(track) = id.and_then(|id| self.db().track(&id)) else { return };
            // Only local files: opening a network stream here could stall the audio thread
            // past the end of the current track. Remote tracks take the regular path.
            if track.source != "local" {
                return;
            }
            let Ok(dec) = File::open(&track.path).map_err(anyhow::Error::from).and_then(|f| Ok(rodio::Decoder::try_from(f)?)) else { return };
            let sample_rate = dec.sample_rate().get();
            if let Some(p) = &self.player {
                p.append(Tap { inner: crate::eq::Eq::new(dec, self.eq.clone()), ring: self.ring.clone(), acc: 0.0, n: 0, local: Vec::with_capacity(256) });
                self.prepared = Some(Prepared { index: next, track, sample_rate });
            }
        }
    }

    /// Volume ramps for crossfade (equal-power curve, so the middle doesn't dip).
    fn ramp(&mut self) {
        let cf = (self.fx.crossfade_ms.max(1)) as f32;
        let v = self.state.read().volume;
        let target = v * v;
        if let Some((old, t0)) = &self.fading_out {
            let k = (t0.elapsed().as_millis() as f32 / cf).min(1.0);
            old.set_volume(target * (k * std::f32::consts::FRAC_PI_2).cos());
            if k >= 1.0 {
                self.fading_out = None;
            }
        }
        if let Some(t0) = self.fading_in {
            let k = (t0.elapsed().as_millis() as f32 / cf).min(1.0);
            if let Some(p) = &self.player {
                p.set_volume(target * (k * std::f32::consts::FRAC_PI_2).sin());
            }
            if k >= 1.0 {
                self.fading_in = None;
            }
        }
    }

    fn on_track_end(&mut self) {
        self.account(false);
        let (repeat, idx, len) = {
            let s = self.state.read();
            (s.repeat.clone(), s.index, s.queue.len())
        };
        if repeat == "one" {
            self.load(idx, true);
        } else if idx + 1 < len {
            self.load(idx + 1, true);
        } else if repeat == "all" && len > 0 {
            self.load(0, true);
        } else {
            // End of queue: keep the last track selected but stopped at the start.
            self.load(idx, false);
        }
        self.emit();
    }

    /// Writes a play record for the outgoing track.
    fn account(&mut self, skipped_by_user: bool) {
        let s = self.state.read().clone();
        if let Some(t) = &s.track {
            if self.listened_ms >= 3000 {
                let counted = self.listened_ms >= 30_000.min(t.duration_ms / 2);
                self.db().record_play(&t.id, self.started_at, self.listened_ms, t.duration_ms, skipped_by_user && !counted);
            }
        }
        self.listened_ms = 0;
    }

    fn load(&mut self, index: usize, autoplay: bool) {
        self.load_inner(index, autoplay, 0);
    }

    fn load_inner(&mut self, index: usize, autoplay: bool, depth: usize) {
        let id = match self.state.read().queue.get(index) {
            Some(id) => id.clone(),
            None => return,
        };
        let Some(track) = self.db().track(&id) else { return };
        self.player = None; // dropping the old player stops it immediately
        self.prepared = None;
        self.fading_in = None;
        self.app.state::<crate::sources::Sources>().spotify().stop();

        // Show the new track right away (cover, title) even if the network takes a moment.
        {
            let mut s = self.state.write();
            s.duration_ms = track.duration_ms;
            s.track = Some(track.clone());
            s.index = index;
            s.pos_ms = 0;
            s.playing = false;
            s.buffering = track.source != "local";
        }
        if track.source != "local" {
            self.emit();
        }

        // The requested copy first, then other copies of the same recording
        // (a local file for a Yandex track, SoundCloud for a missing file, …).
        let mut candidates = vec![track.clone()];
        candidates.extend(self.db().alternates(&track));
        let mut last_err = String::new();
        let mut opened = false;
        for cand in &candidates {
            match self.open_source(cand, autoplay) {
                Ok(()) => {
                    opened = true;
                    break;
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        self.state.write().buffering = false;
        if opened {
            self.started_at = now_ms();
            self.state.write().playing = autoplay;
            return;
        }
        let _ = self.app.emit("oto://error", format!("Не удалось открыть «{}»: {last_err}", track.title));
        let len = self.state.read().queue.len();
        if index + 1 < len && depth < 5 {
            self.load_inner(index + 1, autoplay, depth + 1);
        }
    }

    /// Opens one copy of a track (file or network) and starts it on a fresh player.
    fn open_source(&mut self, t: &Track, autoplay: bool) -> anyhow::Result<()> {
        if t.source == "local" {
            let dec = rodio::Decoder::try_from(File::open(&t.path)?)?;
            self.start(dec, autoplay);
            return Ok(());
        }
        let src = crate::sources::stream_for(&self.app, t)?;
        if let crate::providers::StreamSource::Spotify { uri } = &src {
            let backend = self.app.state::<crate::sources::Sources>().spotify().backend()?;
            backend.play(uri, 0)?;
            backend.wait_started(Duration::from_secs(12))?;
            let source = crate::providers::spotify::SpotifySource { backend, duration: Duration::from_millis(t.duration_ms) };
            self.start(source, autoplay);
            return Ok(());
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
        self.start(dec, autoplay);
        Ok(())
    }

    fn start<S: Source + Send + 'static>(&mut self, dec: S, autoplay: bool) {
        self.ring.sr.store(dec.sample_rate().get(), Ordering::Relaxed);
        let player = Player::connect_new(self.sink.mixer());
        let vol = self.state.read().volume;
        player.set_volume(vol * vol);
        player.append(Tap { inner: crate::eq::Eq::new(dec, self.eq.clone()), ring: self.ring.clone(), acc: 0.0, n: 0, local: Vec::with_capacity(256) });
        if autoplay {
            player.play();
        } else {
            player.pause();
        }
        self.player = Some(player);
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::PlayList { ids, start } => {
                self.account(true);
                let shuffle = self.state.read().shuffle;
                let mut ids = ids;
                let mut start = start.min(ids.len().saturating_sub(1));
                if shuffle && !ids.is_empty() {
                    self.original_queue = Some(ids.clone());
                    let first = ids.remove(start);
                    shuffle_vec(&mut ids);
                    ids.insert(0, first);
                    start = 0;
                } else {
                    self.original_queue = None;
                }
                self.state.write().queue = ids;
                self.load(start, true);
            }
            Cmd::Enqueue(ids) => {
                self.state.write().queue.extend(ids.iter().cloned());
                if let Some(o) = &mut self.original_queue {
                    o.extend(ids);
                }
            }
            Cmd::PlayNext(ids) => {
                let mut s = self.state.write();
                let at = (s.index + 1).min(s.queue.len());
                for (i, id) in ids.into_iter().enumerate() {
                    s.queue.insert(at + i, id);
                }
            }
            Cmd::JumpTo(i) => {
                self.fading_out = None;
                self.account(true);
                self.load(i, true);
            }
            Cmd::Toggle => {
                let playing = self.state.read().playing;
                self.handle(if playing { Cmd::Pause } else { Cmd::Play });
            }
            Cmd::Play => {
                if let Some(p) = &self.player {
                    p.play();
                    self.state.write().playing = true;
                }
            }
            Cmd::Pause => {
                self.fading_out = None;
                if let Some(p) = &self.player {
                    p.pause();
                    self.state.write().playing = false;
                }
            }
            Cmd::Next => {
                self.fading_out = None;
                self.account(true);
                let (idx, len, repeat, playing) = {
                    let s = self.state.read();
                    (s.index, s.queue.len(), s.repeat.clone(), s.playing || self.player.is_none())
                };
                if idx + 1 < len {
                    self.load(idx + 1, playing);
                } else if len > 0 && repeat != "off" {
                    self.load(0, playing);
                }
            }
            Cmd::Prev => {
                let (idx, len, pos, playing) = {
                    let s = self.state.read();
                    (s.index, s.queue.len(), s.pos_ms, s.playing)
                };
                if pos > 3000 || len == 0 {
                    self.handle(Cmd::Seek(0));
                } else {
                    self.account(true);
                    self.load(if idx == 0 { len - 1 } else { idx - 1 }, playing);
                }
            }
            Cmd::Seek(ms) => {
                if let Some(p) = &self.player {
                    let _ = p.try_seek(Duration::from_millis(ms));
                    self.state.write().pos_ms = ms;
                }
            }
            Cmd::SeekBy(d) => {
                let (pos, dur) = {
                    let s = self.state.read();
                    (s.pos_ms as i64, s.duration_ms as i64)
                };
                self.handle(Cmd::Seek((pos + d).clamp(0, (dur - 500).max(0)) as u64));
            }
            Cmd::Volume(v) => self.set_volume(v),
            Cmd::VolumeDelta(d) => {
                let v = self.state.read().volume;
                self.set_volume(v + d);
            }
            Cmd::Shuffle(on) => {
                let mut s = self.state.write();
                if on == s.shuffle {
                    return;
                }
                s.shuffle = on;
                if on {
                    self.original_queue = Some(s.queue.clone());
                    let at = s.index + 1;
                    if at < s.queue.len() {
                        let mut rest = s.queue.split_off(at);
                        shuffle_vec(&mut rest);
                        s.queue.extend(rest);
                    }
                } else if let Some(orig) = self.original_queue.take() {
                    let cur = s.track.as_ref().map(|t| t.id.clone());
                    s.index = cur.and_then(|id| orig.iter().position(|x| *x == id)).unwrap_or(0);
                    s.queue = orig;
                }
            }
            Cmd::Repeat(mode) => self.state.write().repeat = mode,
            Cmd::QueueRemove(i) => {
                let cur = self.state.read().index;
                if i == cur {
                    // Removing the playing track: skip to what follows it.
                    let len = self.state.read().queue.len();
                    self.state.write().queue.remove(i);
                    if len > 1 {
                        let next = i.min(len - 2);
                        let playing = self.state.read().playing;
                        self.account(true);
                        self.load(next, playing);
                    } else {
                        self.player = None;
                        let mut s = self.state.write();
                        s.track = None;
                        s.playing = false;
                        s.index = 0;
                    }
                } else {
                    let mut s = self.state.write();
                    if i < s.queue.len() {
                        s.queue.remove(i);
                        if i < s.index {
                            s.index -= 1;
                        }
                    }
                }
            }
            Cmd::QueueMove { from, to } => {
                let mut s = self.state.write();
                let len = s.queue.len();
                if from < len && to < len && from != to {
                    let id = s.queue.remove(from);
                    s.queue.insert(to, id);
                    let cur = s.index;
                    s.index = if cur == from {
                        to
                    } else if from < cur && to >= cur {
                        cur - 1
                    } else if from > cur && to <= cur {
                        cur + 1
                    } else {
                        cur
                    };
                }
            }
            Cmd::Audio(a) => {
                self.eq.set(a.eq_enabled, a.eq_preamp, &a.eq_gains);
                self.fx = a;
            }
            Cmd::QueueClear => {
                // Keep only what is playing now.
                let mut s = self.state.write();
                if let Some(id) = s.queue.get(s.index).cloned() {
                    s.queue = vec![id];
                    s.index = 0;
                } else {
                    s.queue.clear();
                }
                self.original_queue = None;
            }
            Cmd::RefreshTrack(id) => {
                let fresh = self.db().track(&id);
                let mut s = self.state.write();
                if s.track.as_ref().map(|t| t.id == id).unwrap_or(false) {
                    s.track = fresh;
                }
            }
        }
    }

    fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.state.write().volume = v;
        self.volume_dirty = Some(Instant::now());
        if self.fading_in.is_some() {
            return; // the ramp applies the new level
        }
        if let Some(p) = &self.player {
            // Squared curve: linear sliders feel like they do nothing in the top half.
            p.set_volume(v * v);
        }
    }

    fn emit(&self) {
        let s = self.state.read().clone();
        crate::shell::on_player_state(&self.app, &s);
        let _ = self.app.emit("oto://state", s);
    }
}

fn shuffle_vec<T>(v: &mut [T]) {
    let mut x = (now_ms() as u64) | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
}

// ---------- analyser ----------

const FFT: usize = 2048;
const BANDS: usize = 48;

fn analyser(app: AppHandle, state: Arc<RwLock<PlayerState>>, ring: Arc<Ring>) {
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT);
    let mut input = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    let window: Vec<f32> = (0..FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT as f32).cos()).collect();

    let mut bands = [0f32; BANDS];
    let mut prev_raw = [0f32; BANDS];
    let mut flux_hist: Vec<f32> = Vec::with_capacity(64);
    let mut agc = [0.05f32; 3];
    let mut levels = [0f32; 4]; // lvl, bass, mid, high
    let mut beat = 0f32;
    let mut since_beat = Instant::now();
    let mut idle_frames = 0;
    let frame = Duration::from_micros(16_667);

    loop {
        let t0 = Instant::now();
        let playing = state.read().playing;
        let fresh = ring.fresh.swap(false, Ordering::Relaxed);

        if playing && fresh {
            idle_frames = 0;
            ring.latest(&mut input);
            let mut rms = 0f32;
            for (x, w) in input.iter_mut().zip(&window) {
                rms += *x * *x;
                *x *= w;
            }
            rms = (rms / FFT as f32).sqrt();
            if fft.process(&mut input, &mut spec).is_err() {
                continue;
            }
            let sr = ring.sr.load(Ordering::Relaxed) as f32;
            let hz_per_bin = sr / FFT as f32;
            let mags: Vec<f32> = spec.iter().map(|c| c.norm() / (FFT as f32 / 4.0)).collect();

            let (lo, hi) = (32f32, 16000f32);
            let mut raw = [0f32; BANDS];
            for (b, r) in raw.iter_mut().enumerate() {
                let f0 = lo * (hi / lo).powf(b as f32 / BANDS as f32);
                let f1 = lo * (hi / lo).powf((b + 1) as f32 / BANDS as f32);
                let i0 = (f0 / hz_per_bin) as usize;
                let i1 = ((f1 / hz_per_bin) as usize).max(i0 + 1).min(mags.len() - 1);
                let m = mags[i0..=i1].iter().cloned().fold(0f32, f32::max);
                // +3 dB/oct tilt so the top end is visible next to the bass.
                let tilt = 1.0 + (b as f32 / BANDS as f32) * 3.0;
                let db = 20.0 * (m * tilt + 1e-9).log10();
                *r = ((db + 62.0) / 52.0).clamp(0.0, 1.0);
            }
            for i in 0..BANDS {
                let k = if raw[i] > bands[i] { 0.55 } else { 0.12 };
                bands[i] += (raw[i] - bands[i]) * k;
            }

            // Band energies with automatic gain so quiet and loud masters both move the visuals.
            let energy = |f0: f32, f1: f32| {
                let (i0, i1) = ((f0 / hz_per_bin) as usize, ((f1 / hz_per_bin) as usize).min(mags.len() - 1));
                mags[i0..=i1].iter().map(|m| m * m).sum::<f32>().sqrt()
            };
            let e = [energy(30.0, 160.0), energy(300.0, 2500.0), energy(4000.0, 12000.0)];
            let mut norm = [0f32; 3];
            for i in 0..3 {
                agc[i] = (agc[i] * 0.997).max(e[i]).max(0.02);
                norm[i] = (e[i] / agc[i]).clamp(0.0, 1.0);
            }
            let target = [(rms * 3.0).min(1.0), norm[0], norm[1], norm[2]];
            for i in 0..4 {
                let k = if target[i] > levels[i] { 0.5 } else { 0.1 };
                levels[i] += (target[i] - levels[i]) * k;
            }

            // Onset detection on the low bands: spectral flux against an adaptive threshold.
            let flux: f32 = (0..10).map(|i| (raw[i] - prev_raw[i]).max(0.0)).sum();
            prev_raw = raw;
            if flux_hist.len() == 43 {
                flux_hist.remove(0);
            }
            flux_hist.push(flux);
            let mean = flux_hist.iter().sum::<f32>() / flux_hist.len() as f32;
            let var = flux_hist.iter().map(|f| (f - mean).powi(2)).sum::<f32>() / flux_hist.len() as f32;
            if flux > mean + 1.4 * var.sqrt() && flux > 0.25 && since_beat.elapsed() > Duration::from_millis(240) {
                beat = 1.0;
                since_beat = Instant::now();
            } else {
                beat *= 0.86;
            }
        } else {
            idle_frames += 1;
            for b in bands.iter_mut() {
                *b *= 0.85;
            }
            for l in levels.iter_mut() {
                *l *= 0.85;
            }
            beat *= 0.8;
            if idle_frames > 40 {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        }

        let pos = state.read().pos_ms;
        let _ = app.emit(
            "oto://spectrum",
            SpectrumFrame {
                b: bands.iter().map(|v| (v * 255.0) as u8).collect(),
                lvl: round3(levels[0]),
                bass: round3(levels[1]),
                mid: round3(levels[2]),
                high: round3(levels[3]),
                beat: round3(beat),
                pos,
            },
        );
        if let Some(rest) = frame.checked_sub(t0.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

fn round3(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a short tone through the real output device at zero volume and checks that
    /// the decode → tap → mixer path feeds the analyser and reports progress.
    #[test]
    fn tap_feeds_analyser_during_real_playback() {
        let path = std::env::temp_dir().join("oto_tap_test.wav");
        let sr = 44100u32;
        let n = sr as usize; // 1 s
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n as u32 * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        for v in [16u32] { b.extend_from_slice(&v.to_le_bytes()); }
        for v in [1u16, 1] { b.extend_from_slice(&v.to_le_bytes()); }
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 2).to_le_bytes());
        for v in [2u16, 16] { b.extend_from_slice(&v.to_le_bytes()); }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n as u32 * 2).to_le_bytes());
        for i in 0..n {
            let s = ((i as f32 / sr as f32) * 440.0 * std::f32::consts::TAU).sin() * 8000.0;
            b.extend_from_slice(&(s as i16).to_le_bytes());
        }
        std::fs::write(&path, b).unwrap();

        let Ok(mut sink) = DeviceSinkBuilder::open_default_sink() else {
            eprintln!("no audio device, skipping");
            return;
        };
        sink.log_on_drop(false);
        let ring = Arc::new(Ring::new());
        let dec = rodio::Decoder::try_from(File::open(&path).unwrap()).unwrap();
        let player = Player::connect_new(sink.mixer());
        player.set_volume(0.0);
        player.append(Tap { inner: dec, ring: ring.clone(), acc: 0.0, n: 0, local: Vec::new() });
        player.play();
        std::thread::sleep(Duration::from_millis(400));
        assert!(ring.fresh.load(Ordering::Relaxed), "analyser received no samples");
        let mut buf = vec![0f32; 1024];
        ring.latest(&mut buf);
        let peak = buf.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.1, "tap saw silence (peak {peak})");
        assert!(player.get_pos() > Duration::from_millis(100), "position did not advance");
        assert!(player.try_seek(Duration::from_millis(800)).is_ok(), "seek failed");
    }
}

#[cfg(test)]
mod seek_probe {
    use super::*;
    #[test]
    #[ignore]
    fn probe_seek_on_real_file() {
        let path = std::env::var("OTO_PROBE").unwrap();
        let mut sink = DeviceSinkBuilder::open_default_sink().unwrap();
        sink.log_on_drop(false);
        let ring = Arc::new(Ring::new());
        let dec = rodio::Decoder::try_from(File::open(&path).unwrap()).unwrap();
        println!("total {:?}", dec.total_duration());
        let player = Player::connect_new(sink.mixer());
        player.set_volume(0.0);
        player.append(Tap { inner: dec, ring, acc: 0.0, n: 0, local: Vec::new() });
        player.play();
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        let r = player.try_seek(Duration::from_secs(60));
        println!("seek result {:?} took {:?}", r, t.elapsed());
        std::thread::sleep(Duration::from_millis(300));
        println!("pos after {:?}", player.get_pos());
        player.pause();
        let r = player.try_seek(Duration::from_secs(30));
        println!("paused seek {:?} pos {:?}", r, player.get_pos());
    }
}
