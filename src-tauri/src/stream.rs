//! Network audio for the decoder: a `Read + Seek` over a buffer that a background thread
//! fills from HTTP (progressive file) or HLS (segments appended in order). Playback starts
//! as soon as the first bytes arrive; reads past the downloaded edge wait for more.

use crate::providers::StreamSource;
use parking_lot::{Condvar, Mutex};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct Buf {
    data: Vec<u8>,
    done: bool,
    error: Option<String>,
    /// Total size when the server tells us (Content-Length); unknown for HLS.
    len: Option<u64>,
}

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

struct Shared {
    buf: Mutex<Buf>,
    cv: Condvar,
    cancel: AtomicBool,
    /// Decrypts encrypted transports (Yandex lossless) as bytes arrive. Bytes are always
    /// appended in file order, so the keystream position matches without seeking.
    cipher: Mutex<Option<Aes128Ctr>>,
}

impl Shared {
    fn new() -> Self {
        Self { buf: Mutex::new(Buf::default()), cv: Condvar::new(), cancel: AtomicBool::new(false), cipher: Mutex::new(None) }
    }

    fn append(&self, bytes: &[u8]) {
        let mut chunk = bytes.to_vec();
        if let Some(c) = self.cipher.lock().as_mut() {
            use ctr::cipher::StreamCipher;
            c.apply_keystream(&mut chunk);
        }
        let mut b = self.buf.lock();
        b.data.extend_from_slice(&chunk);
        self.cv.notify_all();
    }
}

pub struct NetStream {
    shared: Arc<Shared>,
    pos: u64,
}

impl Drop for NetStream {
    fn drop(&mut self) {
        // Stop downloading when the track is skipped.
        self.shared.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct Opened {
    pub reader: NetStream,
    pub byte_len: Option<u64>,
    pub mime: Option<String>,
}

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(8))
        // The blocking client applies this to each body read: a connection that delivers
        // nothing for this long is treated as throttled.
        .timeout(Duration::from_secs(5))
        .build()
        .expect("http client")
}

/// No connection reuse: some networks throttle a TLS connection to a crawl after its first
/// ~16 KB, so each small range request gets a fresh connection.
fn fresh_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(8))
        .pool_max_idle_per_host(0)
        .build()
        .expect("http client")
}

/// Starts downloading and returns once the first chunk is in (or the request failed),
/// so the decoder can probe the format immediately.
pub fn open(src: StreamSource) -> anyhow::Result<Opened> {
    let shared = Arc::new(Shared::new());
    // Pull the internal decryption key out of the headers before anything is sent.
    let src = match src {
        StreamSource::Http { url, headers, mime } => {
            let (key, headers): (Vec<_>, Vec<_>) = headers.into_iter().partition(|(k, _)| k == "x-oto-aes-ctr-key");
            if let Some((_, hexkey)) = key.first() {
                use ctr::cipher::KeyIvInit;
                let k = hex::decode(hexkey).map_err(|e| anyhow::anyhow!("bad stream key: {e}"))?;
                if k.len() != 16 {
                    anyhow::bail!("unexpected stream key length {}", k.len());
                }
                *shared.cipher.lock() = Some(Aes128Ctr::new(k.as_slice().into(), &[0u8; 16].into()));
            }
            StreamSource::Http { url, headers, mime }
        }
        other => other,
    };
    let mime = match &src {
        StreamSource::Http { mime, .. } => mime.clone(),
        StreamSource::Hls { mime, .. } => match mime.as_deref() {
            Some(m) if m.contains("mp4") => Some("audio/aac".to_string()),
            _ => mime.clone(),
        },
        StreamSource::Spotify { .. } => anyhow::bail!("Spotify plays through its own session"),
    };
    let sh = shared.clone();
    std::thread::Builder::new().name("oto-net".into()).spawn(move || {
        let res = match src {
            StreamSource::Http { url, headers, .. } => fetch_http(&sh, &url, &headers),
            StreamSource::Hls { playlist_url, headers, .. } => fetch_hls(&sh, &playlist_url, &headers),
            StreamSource::Spotify { .. } => Ok(()),
        };
        let mut b = sh.buf.lock();
        b.done = true;
        if let Err(e) = res {
            b.error = Some(e.to_string());
        }
        sh.cv.notify_all();
    })?;

    // Wait for the first 64 KB (or completion) so probing doesn't stall in the decoder.
    let mut b = shared.buf.lock();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while b.data.len() < 64 * 1024 && !b.done {
        if shared.cv.wait_until(&mut b, deadline).timed_out() {
            break;
        }
    }
    if b.data.is_empty() {
        let e = b.error.clone().unwrap_or_else(|| "no data".into());
        drop(b);
        shared.cancel.store(true, Ordering::Relaxed);
        anyhow::bail!("stream: {e}");
    }
    let byte_len = b.len;
    drop(b);
    Ok(Opened { reader: NetStream { shared, pos: 0 }, byte_len, mime })
}

fn get(url: &str, headers: &[(String, String)]) -> anyhow::Result<reqwest::blocking::Response> {
    let mut rq = client().get(url);
    for (k, v) in headers {
        rq = rq.header(k, v);
    }
    let r = rq.send()?;
    if !r.status().is_success() {
        anyhow::bail!("HTTP {}", r.status());
    }
    Ok(r)
}

/// Copies a response body into the buffer. Returns how many bytes it added; an error means
/// the transfer broke off part-way, with the byte count reached so far.
fn pump(sh: &Shared, mut r: impl Read) -> Result<u64, (u64, anyhow::Error)> {
    let mut chunk = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        if sh.cancel.load(Ordering::Relaxed) {
            return Ok(got);
        }
        let n = match r.read(&mut chunk) {
            Ok(n) => n,
            Err(e) => return Err((got, e.into())),
        };
        if n == 0 {
            return Ok(got);
        }
        got += n as u64;
        sh.append(&chunk[..n]);
    }
}

const CHUNK: u64 = 14 * 1024;
const WORKERS: u64 = 4;

/// Downloads `url` from byte `from` in small ranges, `WORKERS` at a time, appended in order.
fn fetch_ranged(sh: &Shared, url: &str, headers: &[(String, String)], mut from: u64, mut total: Option<u64>) -> anyhow::Result<()> {
    let c = fresh_client();
    let fetch = |start: u64| -> anyhow::Result<(Vec<u8>, Option<u64>)> {
        let mut last = None;
        for _ in 0..4 {
            let mut rq = c.get(url).header("Range", format!("bytes={}-{}", start, start + CHUNK - 1));
            for (k, v) in headers {
                rq = rq.header(k, v);
            }
            match rq.send() {
                Ok(r) if r.status().as_u16() == 206 => {
                    let t = r
                        .headers()
                        .get("content-range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.rsplit('/').next())
                        .and_then(|t| t.parse::<u64>().ok());
                    match r.bytes() {
                        Ok(b) => return Ok((b.to_vec(), t)),
                        Err(e) => last = Some(anyhow::Error::from(e)),
                    }
                }
                Ok(r) if r.status().as_u16() == 416 => return Ok((vec![], None)),
                Ok(r) => anyhow::bail!("range request: HTTP {}", r.status()),
                Err(e) => last = Some(e.into()),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("range request failed")))
    };
    loop {
        if sh.cancel.load(Ordering::Relaxed) || total.is_some_and(|t| from >= t) {
            return Ok(());
        }
        let starts: Vec<u64> = (0..WORKERS).map(|i| from + i * CHUNK).filter(|s| total.is_none_or(|t| *s < t)).collect();
        let results: Vec<anyhow::Result<(Vec<u8>, Option<u64>)>> = std::thread::scope(|sc| {
            let handles: Vec<_> = starts.iter().map(|&s| sc.spawn(move || fetch(s))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("worker panicked")))).collect()
        });
        for r in results {
            let (bytes, t) = r?;
            if total.is_none() {
                total = t;
            }
            let n = bytes.len() as u64;
            if n > 0 {
                sh.append(&bytes);
            }
            from += n;
            if n < CHUNK {
                return Ok(()); // short read: end of the file
            }
        }
    }
}

/// Plain GET; if the connection stalls part-way, carries on with small ranged requests.
fn fetch_resilient(sh: &Shared, url: &str, headers: &[(String, String)], set_len: bool) -> anyhow::Result<()> {
    let r = get(url, headers)?;
    let len = r.content_length();
    if set_len {
        if let Some(l) = len {
            let mut b = sh.buf.lock();
            b.len = Some(l);
            b.data.reserve(l as usize);
        }
    }
    match pump(sh, r) {
        Ok(_) => Ok(()),
        Err((got, e)) => {
            eprintln!("stream stalled after {got} bytes ({e}); continuing with ranged requests");
            fetch_ranged(sh, url, headers, got, len)
        }
    }
}

/// Downloads a whole (small) resource with the same stall handling, into memory.
fn download(url: &str, headers: &[(String, String)]) -> anyhow::Result<Vec<u8>> {
    let tmp = Shared::new();
    fetch_resilient(&tmp, url, headers, false)?;
    Ok(std::mem::take(&mut tmp.buf.lock().data))
}

fn fetch_http(sh: &Shared, url: &str, headers: &[(String, String)]) -> anyhow::Result<()> {
    fetch_resilient(sh, url, headers, true)
}

/// Value of `KEY="value"` in an HLS tag line.
fn attr(line: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=\"");
    let i = line.find(&pat)? + pat.len();
    let j = line[i..].find('"')? + i;
    Some(line[i..j].to_string())
}

/// Media playlist only (the providers hand us a media playlist URL). Supports fragmented
/// MP4 (`#EXT-X-MAP` init segment followed by .m4s fragments) and plain MPEG segments.
fn fetch_hls(sh: &Shared, playlist_url: &str, headers: &[(String, String)]) -> anyhow::Result<()> {
    let base = reqwest::Url::parse(playlist_url)?;
    let text = get(playlist_url, headers)?.text()?;
    let mut init = None;
    let mut segs = vec![];
    for line in text.lines().map(str::trim) {
        if line.starts_with("#EXT-X-KEY") && !line.contains("METHOD=NONE") {
            anyhow::bail!("encrypted HLS is not supported");
        }
        if line.starts_with("#EXT-X-MAP") {
            if let Some(uri) = attr(line, "URI") {
                init = Some(base.join(&uri)?.to_string());
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        segs.push(base.join(line)?.to_string());
    }
    if segs.is_empty() {
        anyhow::bail!("empty HLS playlist");
    }
    // fMP4 with AAC: transmux every fragment to ADTS (see fmp4.rs). Other init segments
    // are passed through in front of the fragments.
    let mut aac = None;
    if let Some(i) = init {
        let bytes = download(&i, headers)?;
        match crate::fmp4::parse_init(&bytes) {
            Some(cfg) => aac = Some(cfg),
            None => {
                let mut b = sh.buf.lock();
                b.data.extend_from_slice(&bytes);
                sh.cv.notify_all();
            }
        }
    }
    for s in segs {
        if sh.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut attempt = 0;
        let bytes = loop {
            match download(&s, headers) {
                Ok(b) => break b,
                Err(e) if attempt < 2 => {
                    attempt += 1;
                    eprintln!("hls segment retry {attempt}: {e}");
                    std::thread::sleep(Duration::from_millis(400));
                }
                Err(e) => return Err(e),
            }
        };
        let bytes = match aac {
            Some(cfg) => crate::fmp4::segment_to_adts(&bytes, cfg),
            None => bytes,
        };
        let mut b = sh.buf.lock();
        b.data.extend_from_slice(&bytes);
        sh.cv.notify_all();
    }
    Ok(())
}

impl Read for NetStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut b = self.shared.buf.lock();
        loop {
            let have = b.data.len() as u64;
            if self.pos < have {
                let n = ((have - self.pos) as usize).min(out.len());
                let start = self.pos as usize;
                out[..n].copy_from_slice(&b.data[start..start + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if b.done {
                return match &b.error {
                    Some(e) if self.pos < b.len.unwrap_or(u64::MAX) && have == 0 => Err(std::io::Error::other(e.clone())),
                    _ => Ok(0),
                };
            }
            self.shared.cv.wait_for(&mut b, Duration::from_millis(500));
        }
    }
}

impl Seek for NetStream {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let new = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                let mut b = self.shared.buf.lock();
                // Unknown length (HLS): the end is only known once everything is in.
                while b.len.is_none() && !b.done {
                    self.shared.cv.wait_for(&mut b, Duration::from_millis(500));
                }
                b.len.unwrap_or(b.data.len() as u64) as i64 + d
            }
        };
        if new < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_waits_for_bytes_and_seeks() {
        let shared = Arc::new(Shared::new());
        let sh = shared.clone();
        std::thread::spawn(move || {
            for i in 0..4u8 {
                std::thread::sleep(Duration::from_millis(30));
                let mut b = sh.buf.lock();
                b.data.extend_from_slice(&[i; 100]);
                sh.cv.notify_all();
            }
            sh.buf.lock().done = true;
            sh.cv.notify_all();
        });
        let mut r = NetStream { shared, pos: 0 };
        let mut all = vec![];
        r.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 400);
        assert_eq!(all[399], 3);
        assert_eq!(r.seek(SeekFrom::Start(150)).unwrap(), 150);
        let mut one = [0u8; 1];
        r.read_exact(&mut one).unwrap();
        assert_eq!(one[0], 1);
        assert_eq!(r.seek(SeekFrom::End(-1)).unwrap(), 399);
    }

    /// Real services: resolve a stream, decode 3 s of audio through NetStream + rodio.
    fn decode_seconds(src: StreamSource) -> f32 {
        use rodio::Source;
        let o = open(src).expect("open stream");
        let mut b = rodio::Decoder::builder().with_data(o.reader).with_seekable(true).with_coarse_seek(true);
        if let Some(l) = o.byte_len {
            b = b.with_byte_len(l);
        }
        if let Some(m) = o.mime.as_deref() {
            b = b.with_mime_type(m);
        }
        let dec = b.build().expect("decoder");
        let (sr, ch) = (dec.sample_rate().get() as f32, dec.channels().get() as f32);
        let n = dec.take((sr * ch * 3.0) as usize).count() as f32;
        n / (sr * ch)
    }

    #[test]
    #[ignore = "network"]
    fn net_yandex_preview_decodes() {
        use crate::providers::Provider;
        let y = crate::providers::yandex::Yandex::new(None);
        let hit = y.search("Кино Группа крови", 3).unwrap().remove(0);
        let secs = decode_seconds(y.stream(&hit.remote_id).unwrap());
        assert!(secs > 2.9, "decoded only {secs}s");
    }

    #[test]
    #[ignore = "network"]
    fn net_soundcloud_hls_decodes() {
        use crate::providers::Provider;
        let sc = crate::providers::soundcloud::SoundCloud::new(None);
        let hits = sc.search("lofi hip hop", 10).unwrap();
        let src = hits.iter().find_map(|h| sc.stream(&h.remote_id).ok()).expect("a playable track");
        eprintln!("source: {src:?}");
        let secs = decode_seconds(src);
        assert!(secs > 2.9, "decoded only {secs}s");
    }
}
