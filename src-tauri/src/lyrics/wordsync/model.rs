//! The speech model: a ggml whisper.cpp file in `<app data>/models/`, downloaded on request.

use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Multilingual "small", 5-bit quantised: the best accuracy on sung Russian/English that runs
/// in well under real time on a 6-core CPU (see the benchmark notes in `mod.rs`).
pub const MODEL_FILE: &str = "ggml-small-q5_1.bin";
/// Exact size of that file on Hugging Face, bytes.
pub const MODEL_BYTES: u64 = 190_085_487;
const URL_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static CANCEL_DOWNLOAD: AtomicBool = AtomicBool::new(false);

pub fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("models"))
}

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join(MODEL_FILE))
}

/// The model file if it is fully there.
pub fn ready_path(app: &AppHandle) -> Option<PathBuf> {
    let p = path(app)?;
    let len = std::fs::metadata(&p).ok()?.len();
    (len == MODEL_BYTES).then_some(p)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Built with speech recognition (the `wordsync` cargo feature).
    pub available: bool,
    pub model_ready: bool,
    pub model_size_mb: u32,
    pub downloading: bool,
}

pub fn status(app: &AppHandle) -> Status {
    Status {
        available: cfg!(feature = "wordsync"),
        model_ready: ready_path(app).is_some(),
        model_size_mb: (MODEL_BYTES / 1_000_000) as u32,
        downloading: DOWNLOADING.load(Ordering::Relaxed),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Starts the download in the background (no-op if one is running or the model is there).
/// Progress: `oto://wordsync-model`.
pub fn start_download(app: AppHandle) {
    if ready_path(&app).is_some() {
        let _ = app.emit("oto://wordsync-model", Progress { downloaded: MODEL_BYTES, total: MODEL_BYTES, done: true, error: None });
        return;
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return;
    }
    CANCEL_DOWNLOAD.store(false, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("oto-model-dl".into())
        .spawn(move || {
            let res = dir(&app).ok_or_else(|| anyhow::anyhow!("no app data dir")).and_then(|d| {
                download(&d, &|got, total| {
                    let _ = app.emit("oto://wordsync-model", Progress { downloaded: got, total, done: false, error: None });
                })
            });
            DOWNLOADING.store(false, Ordering::SeqCst);
            let ev = match res {
                Ok(()) => Progress { downloaded: MODEL_BYTES, total: MODEL_BYTES, done: true, error: None },
                Err(e) => Progress { downloaded: 0, total: MODEL_BYTES, done: true, error: Some(e.to_string()) },
            };
            let _ = app.emit("oto://wordsync-model", ev);
        })
        .ok();
}

/// Downloads to `<file>.part`, then renames. `progress(downloaded, total)` is throttled.
fn download(dir: &Path, progress: &dyn Fn(u64, u64)) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(MODEL_FILE);
    let part = dir.join(format!("{MODEL_FILE}.part"));
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(None::<Duration>)
        .build()?;
    let mut resp = client.get(format!("{URL_BASE}{MODEL_FILE}")).send()?.error_for_status()?;
    let total = resp.content_length().unwrap_or(MODEL_BYTES);
    let mut f = std::io::BufWriter::new(std::fs::File::create(&part)?);
    let mut buf = vec![0u8; 256 * 1024];
    let (mut got, mut last) = (0u64, Instant::now());
    progress(0, total);
    loop {
        if CANCEL_DOWNLOAD.load(Ordering::Relaxed) {
            drop(f);
            let _ = std::fs::remove_file(&part);
            anyhow::bail!("cancelled");
        }
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        got += n as u64;
        if last.elapsed() > Duration::from_millis(250) {
            progress(got, total);
            last = Instant::now();
        }
    }
    f.flush()?;
    drop(f);
    if got != MODEL_BYTES {
        let _ = std::fs::remove_file(&part);
        anyhow::bail!("model download incomplete: {got} of {MODEL_BYTES} bytes");
    }
    progress(got, total);
    std::fs::rename(&part, &dest)?;
    Ok(())
}

/// Removes the model (and a partial download; a running download is cancelled).
pub fn delete(app: &AppHandle) {
    CANCEL_DOWNLOAD.store(true, Ordering::SeqCst);
    if let Some(d) = dir(app) {
        let _ = std::fs::remove_file(d.join(MODEL_FILE));
        let _ = std::fs::remove_file(d.join(format!("{MODEL_FILE}.part")));
    }
}
