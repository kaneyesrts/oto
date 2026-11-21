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
    /// Random access (HTTP with Range support and a known size): `data` is the whole file,
    /// `have` the byte ranges downloaded so far. Otherwise bytes are appended in order.
    sparse: bool,
    have: Vec<(u64, u64)>,
    /// A position the reader is blocked on that the download isn't about to reach.
    want: Option<u64>,
    /// Where the downloader is writing now.
    head: u64,
    /// Where the reader last read: holes after it are filled first.
    read_pos: u64,
}

impl Buf {
    /// End of the downloaded range containing `pos`.
    fn covered_until(&self, pos: u64) -> Option<u64> {
        self.have.iter().find(|(a, b)| *a <= pos && pos < *b).map(|r| r.1)
    }

    fn add_range(&mut self, a: u64, b: u64) {
        if a >= b {
            return;
        }
        self.have.push((a, b));
        self.have.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.have.len());
        for &(s, e) in &self.have {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        self.have = merged;
    }

    /// The first missing byte range at or after `from`, wrapping around to the start.
    fn next_gap(&self, from: u64) -> Option<(u64, u64)> {
        let len = self.len?;
        let gap_from = |mut p: u64| -> Option<(u64, u64)> {
            for &(a, b) in &self.have {
                if b <= p {
                    continue;
                }
                if a > p {
                    return Some((p, a));
                }
                p = b;
            }
            (p < len).then_some((p, len))
        };
        gap_from(from).or_else(|| gap_from(0))
    }
}

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

struct Shared {
    buf: Mutex<Buf>,
    cv: Condvar,
    cancel: AtomicBool,
    /// AES-128-CTR key for encrypted transports (Yandex lossless). CTR can start at any
    /// byte offset, so ranges downloaded out of order decrypt independently.
    key: Option<[u8; 16]>,
}

impl Shared {
    fn new() -> Self {
        Self::with_key(None)
    }

    fn with_key(key: Option<[u8; 16]>) -> Self {
        Self { buf: Mutex::new(Buf::default()), cv: Condvar::new(), cancel: AtomicBool::new(false), key }
    }

    fn decrypt(&self, at: u64, chunk: &mut [u8]) {
        if let Some(k) = &self.key {
            use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
            let mut c = Aes128Ctr::new(k.as_slice().into(), &[0u8; 16].into());
            c.seek(at);
            c.apply_keystream(chunk);
        }
    }

    /// Sequential mode: bytes arrive in file order.
    fn append(&self, bytes: &[u8]) {
        let mut chunk = bytes.to_vec();
        let at = self.buf.lock().data.len() as u64;
        self.decrypt(at, &mut chunk);
        let mut b = self.buf.lock();
        b.data.extend_from_slice(&chunk);
        self.cv.notify_all();
    }

    /// Random-access mode: bytes for `at..`.
    fn write_at(&self, at: u64, bytes: &[u8]) {
        let mut chunk = bytes.to_vec();
        self.decrypt(at, &mut chunk);
        let mut b = self.buf.lock();
        let start = (at as usize).min(b.data.len());
        let end = (at as usize + chunk.len()).min(b.data.len());
        b.data[start..end].copy_from_slice(&chunk[..end - start]);
        b.add_range(start as u64, end as u64);
        b.head = end as u64;
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

/// One shared client: kept-alive TLS connections make the next track (same CDN) start
/// without a fresh handshake.
fn client() -> reqwest::blocking::Client {
    static C: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(UA)
            .connect_timeout(Duration::from_secs(8))
            // The blocking client applies this to each body read: a connection that delivers
            // nothing for this long is treated as throttled.
            .timeout(Duration::from_secs(5))
            .build()
            .expect("http client")
    })
    .clone()
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

/// Starts downloading and returns as soon as the first bytes are in (or the request
/// failed); the decoder then reads on while the rest streams in.
pub fn open(src: StreamSource) -> anyhow::Result<Opened> {
    // Pull the internal decryption key out of the headers before anything is sent.
    let mut key = None;
    let src = match src {
        StreamSource::Http { url, headers, mime } => {
            let (k, headers): (Vec<_>, Vec<_>) = headers.into_iter().partition(|(k, _)| k == "x-oto-aes-ctr-key");
            if let Some((_, hexkey)) = k.first() {
                let k = hex::decode(hexkey).map_err(|e| anyhow::anyhow!("bad stream key: {e}"))?;
                key = Some(<[u8; 16]>::try_from(k.as_slice()).map_err(|_| anyhow::anyhow!("unexpected stream key length {}", k.len()))?);
            }
            StreamSource::Http { url, headers, mime }
        }
        other => other,
    };
    let shared = Arc::new(Shared::with_key(key));
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

    let mut b = shared.buf.lock();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let first = |b: &Buf| if b.sparse { !b.have.is_empty() } else { !b.data.is_empty() };
    while !b.done && !first(&b) {
        if shared.cv.wait_until(&mut b, deadline).timed_out() {
            break;
        }
    }
    if !first(&b) {
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
    let fetch = |start: u64| range_chunk(&c, url, headers, start);
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
    // Ask for a range from byte 0: a 206 with a total size means random access works, so a
    // seek can jump the download straight to where the listener wants to be.
    let r = get_range(url, headers, 0)?;
    let total = if r.status().as_u16() == 206 { content_range_total(&r) } else { None };
    let Some(total) = total else {
        // No range support: sequential download, as before.
        let len = r.content_length();
        if let Some(l) = len {
            let mut b = sh.buf.lock();
            b.len = Some(l);
            b.data.reserve(l as usize);
        }
        return match pump(sh, r) {
            Ok(_) => Ok(()),
            Err((got, e)) => {
                eprintln!("stream stalled after {got} bytes ({e}); continuing with ranged requests");
                fetch_ranged(sh, url, headers, got, len)
            }
        };
    };
    {
        let mut b = sh.buf.lock();
        b.sparse = true;
        b.len = Some(total);
        b.data = vec![0; total as usize];
    }
    let mut first = Some(r);
    loop {
        if sh.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let (start, r) = match first.take() {
            Some(r) => (0, r),
            None => {
                let pick = {
                    let mut b = sh.buf.lock();
                    let from = b.want.take().unwrap_or(b.read_pos);
                    b.next_gap(from)
                };
                let Some((start, _)) = pick else { return Ok(()) };
                let mut last = None;
                let mut got = None;
                for _ in 0..3 {
                    match get_range(url, headers, start) {
                        Ok(r) if r.status().as_u16() == 206 => {
                            got = Some(r);
                            break;
                        }
                        Ok(r) => last = Some(anyhow::anyhow!("range request: HTTP {}", r.status())),
                        Err(e) => last = Some(e),
                    }
                }
                match got {
                    Some(r) => (start, r),
                    None => return Err(last.unwrap_or_else(|| anyhow::anyhow!("range request failed"))),
                }
            }
        };
        if let Err((at, e)) = stream_gap(sh, r, start) {
            eprintln!("stream stalled at {at} ({e}); continuing with ranged requests");
            ranged_gap(sh, url, headers, at)?;
        }
    }
}

/// A wanted byte at most this far ahead of the running download is simply waited for;
/// further away (or behind), the download restarts there.
const WINDOW: u64 = 512 * 1024;

/// True when the current download should stop: it ran into bytes we already have, or the
/// reader wants a position elsewhere.
fn should_yield(b: &Buf, at: u64) -> bool {
    b.covered_until(at).is_some() || b.want.is_some_and(|w| w < at || w > at + WINDOW)
}

/// Writes a range response into the sparse buffer until it should yield or ends.
fn stream_gap(sh: &Shared, mut r: impl Read, mut at: u64) -> Result<(), (u64, anyhow::Error)> {
    let mut chunk = vec![0u8; 32 * 1024];
    loop {
        if sh.cancel.load(Ordering::Relaxed) || should_yield(&sh.buf.lock(), at) {
            return Ok(());
        }
        let n = match r.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) => return Err((at, e.into())),
        };
        sh.write_at(at, &chunk[..n]);
        at += n as u64;
    }
}

/// Throttled connection: the rest of the gap in small ranges on fresh connections.
fn ranged_gap(sh: &Shared, url: &str, headers: &[(String, String)], mut at: u64) -> anyhow::Result<()> {
    let c = fresh_client();
    loop {
        let len = {
            let b = sh.buf.lock();
            if should_yield(&b, at) {
                return Ok(());
            }
            b.len.unwrap_or(0)
        };
        if at >= len || sh.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let starts: Vec<u64> = (0..WORKERS).map(|i| at + i * CHUNK).filter(|s| *s < len).collect();
        let results: Vec<anyhow::Result<(Vec<u8>, Option<u64>)>> = std::thread::scope(|sc| {
            let hs: Vec<_> = starts.iter().map(|&s| { let c = &c; sc.spawn(move || range_chunk(c, url, headers, s)) }).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("worker panicked")))).collect()
        });
        for (s, r) in starts.iter().zip(results) {
            let (bytes, _) = r?;
            if !bytes.is_empty() {
                sh.write_at(*s, &bytes);
            }
        }
        at = (starts.last().copied().unwrap_or(at) + CHUNK).min(len);
    }
}

fn get_range(url: &str, headers: &[(String, String)], from: u64) -> anyhow::Result<reqwest::blocking::Response> {
    let mut rq = client().get(url).header("Range", format!("bytes={from}-"));
    for (k, v) in headers {
        rq = rq.header(k, v);
    }
    let r = rq.send()?;
    if !r.status().is_success() {
        anyhow::bail!("HTTP {}", r.status());
    }
    Ok(r)
}

fn content_range_total(r: &reqwest::blocking::Response) -> Option<u64> {
    r.headers().get("content-range")?.to_str().ok()?.rsplit('/').next()?.parse().ok()
}

/// One small range on the given client, retried a few times.
fn range_chunk(c: &reqwest::blocking::Client, url: &str, headers: &[(String, String)], start: u64) -> anyhow::Result<(Vec<u8>, Option<u64>)> {
    let mut last = None;
    for _ in 0..4 {
        let mut rq = c.get(url).header("Range", format!("bytes={}-{}", start, start + CHUNK - 1));
        for (k, v) in headers {
            rq = rq.header(k, v);
        }
        match rq.send() {
            Ok(r) if r.status().as_u16() == 206 => {
                let t = content_range_total(&r);
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
            if b.sparse {
                let len = b.len.unwrap_or(0);
                if self.pos >= len {
                    return Ok(0);
                }
                if let Some(end) = b.covered_until(self.pos) {
                    let n = ((end - self.pos) as usize).min(out.len());
                    let start = self.pos as usize;
                    out[..n].copy_from_slice(&b.data[start..start + n]);
                    self.pos += n as u64;
                    b.read_pos = self.pos;
                    return Ok(n);
                }
                if b.done {
                    return Err(std::io::Error::other(b.error.clone().unwrap_or_else(|| "stream ended early".into())));
                }
                // Not here yet: unless the running download is about to reach it, ask for
                // the download to jump here (a seek).
                let near = self.pos >= b.head && self.pos <= b.head + WINDOW;
                if !near && b.want != Some(self.pos) {
                    b.want = Some(self.pos);
                }
            } else {
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
            }
            self.shared.cv.wait_for(&mut b, Duration::from_millis(200));
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
    fn ranges_merge_and_gaps_wrap() {
        let mut b = Buf { len: Some(100), ..Default::default() };
        b.add_range(10, 20);
        b.add_range(30, 40);
        b.add_range(20, 30);
        assert_eq!(b.have, vec![(10, 40)]);
        assert_eq!(b.next_gap(15), Some((40, 100)));
        b.add_range(40, 100);
        assert_eq!(b.next_gap(50), Some((0, 10)));
        b.add_range(0, 10);
        assert_eq!(b.next_gap(0), None);
        assert_eq!(b.covered_until(5), Some(100));
    }

    #[test]
    fn sparse_reader_jumps_and_decrypts_at_offsets() {
        use ctr::cipher::{KeyIvInit, StreamCipher};
        const LEN: usize = 2_000_000;
        let key = [7u8; 16];
        let plain: Vec<u8> = (0..LEN as u32).map(|i| (i % 251) as u8).collect();
        let mut enc = plain.clone();
        Aes128Ctr::new(key.as_slice().into(), &[0u8; 16].into()).apply_keystream(&mut enc);
        let shared = Arc::new(Shared::with_key(Some(key)));
        {
            let mut b = shared.buf.lock();
            b.sparse = true;
            b.len = Some(LEN as u64);
            b.data = vec![0; LEN];
        }
        // Behaves like fetch_http: serves the wanted position, else carries on after the
        // reader, 64 KB at a time.
        let sh = shared.clone();
        let served = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let served2 = served.clone();
        std::thread::spawn(move || loop {
            let gap = {
                let mut b = sh.buf.lock();
                if sh.cancel.load(Ordering::Relaxed) {
                    return;
                }
                let from = b.want.take().unwrap_or(b.read_pos);
                b.next_gap(from)
            };
            let Some((at, end)) = gap else { return };
            let end = end.min(at + 65_536) as usize;
            sh.write_at(at, &enc[at as usize..end]);
            served2.fetch_add((end - at as usize) as u64, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(2));
        });
        let mut r = NetStream { shared, pos: 0 };
        // Far beyond the download window: the download must jump there, not crawl.
        r.seek(SeekFrom::Start(1_500_000)).unwrap();
        let mut buf = [0u8; 300];
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &plain[1_500_000..1_500_300]);
        assert!(served.load(Ordering::Relaxed) < 600_000, "downloaded {} bytes to reach the seek target", served.load(Ordering::Relaxed));
        // And back to the start.
        r.seek(SeekFrom::Start(1000)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &plain[1000..1300]);
    }

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

    /// Start-up and seek latency against the real Yandex account of this machine.
    #[test]
    #[ignore = "network + local account"]
    fn net_yandex_latency() {
        use crate::providers::Provider;
        use rodio::Source;
        use std::time::Instant;
        let creds = std::fs::read_to_string(std::env::var("APPDATA").unwrap() + "/app.oto.player/credentials.json").unwrap();
        let token = serde_json::from_str::<serde_json::Value>(&creds).unwrap()["yandex_token"].as_str().map(String::from);
        for q in ["lossless", "high"] {
            let y = crate::providers::yandex::Yandex::new(token.clone());
            y.set_quality(q);
            let liked = y.liked().unwrap();
            for t in liked.iter().skip(3).take(3) {
                let t0 = Instant::now();
                let src = y.stream(&t.remote_id).unwrap();
                let t1 = t0.elapsed();
                let o = open(src).unwrap();
                let t2 = t0.elapsed();
                let mut b = rodio::Decoder::builder().with_data(o.reader).with_seekable(true).with_coarse_seek(true);
                if let Some(l) = o.byte_len {
                    b = b.with_byte_len(l);
                }
                if let Some(m) = o.mime.as_deref() {
                    b = b.with_mime_type(m);
                }
                let mut dec = b.build().unwrap();
                let t3 = t0.elapsed();
                let _ = dec.next();
                let s0 = Instant::now();
                let r = dec.try_seek(Duration::from_secs(120));
                let _ = dec.next();
                println!(
                    "{q} {}: resolve {:?} | first bytes {:?} | decoder {:?} | seek 2:00 {:?} ({})",
                    t.title, t1, t2 - t1, t3 - t2, s0.elapsed(), if r.is_ok() { "ok" } else { "err" }
                );
            }
        }
    }
}