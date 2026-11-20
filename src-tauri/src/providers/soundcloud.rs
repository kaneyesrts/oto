//! SoundCloud provider, built on the public web API (`api-v2.soundcloud.com`) the way the
//! soundcloud.com web client uses it. App registration is closed, so the `client_id` is
//! discovered from SoundCloud's own pages and cached; when api-v2 starts rejecting it
//! (401/403) the cache is dropped, a fresh one is discovered and the request retried once.
//!
//! Public tracks need no login. A user OAuth token (`Authorization: OAuth <token>`) is only
//! needed for the user's likes and account name; when present it is sent with every request,
//! like the logged-in web client does (Go+ subscribers then get full-length tracks).
//!
//! Streams: most tracks now only offer AAC in fragmented-MP4 HLS (`EXT-X-MAP` init segment +
//! `.m4s` media segments); progressive / HLS MP3 is mostly left on legacy uploads and 30 s
//! previews. DRM-encrypted HLS variants (`cbc-` / `ctr-encrypted-hls`) are never chosen.

use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{StatusCode, Url};
use serde_json::Value;

use super::{AlbumPage, ArtistPage, Provider, RemoteAlbum, RemoteArtist, RemoteTrack, StreamSource};

const API: &str = "https://api-v2.soundcloud.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// Upper bound on how many likes `liked()` pages through.
const MAX_LIKES: usize = 2000;

pub struct SoundCloud {
    oauth: Option<String>,
    client_id: Mutex<Option<String>>,
    http: Client,
}

impl SoundCloud {
    pub fn new(oauth: Option<String>) -> Self {
        let oauth = oauth
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self {
            oauth,
            client_id: Mutex::new(None),
            http,
        }
    }

    /// The logged-in user's display name (`username` from `/me`). Requires the OAuth token.
    pub fn account_name(&self) -> anyhow::Result<String> {
        let me = self.me()?;
        str_field(&me, "username")
            .or_else(|| str_field(&me, "full_name"))
            .ok_or_else(|| anyhow!("SoundCloud /me returned no username"))
    }

    fn me(&self) -> anyhow::Result<Value> {
        if self.oauth.is_none() {
            bail!("not logged in to SoundCloud (no OAuth token)");
        }
        self.api_get(&format!("{API}/me"), &[])
    }

    // ---- client_id -------------------------------------------------------------------------

    fn client_id(&self) -> anyhow::Result<String> {
        let mut cached = self.client_id.lock();
        if let Some(id) = cached.as_ref() {
            return Ok(id.clone());
        }
        let id = self.discover_client_id()?;
        *cached = Some(id.clone());
        Ok(id)
    }

    fn forget_client_id(&self, stale: &str) {
        let mut cached = self.client_id.lock();
        if cached.as_deref() == Some(stale) {
            *cached = None;
        }
    }

    /// m.soundcloud.com embeds the id in its Next.js page data, which costs one request; the
    /// desktop site keeps it inside one of its (large) JS bundles, so it is the fallback.
    fn discover_client_id(&self) -> anyhow::Result<String> {
        let mut errors = Vec::new();

        match self.fetch_text("https://m.soundcloud.com/") {
            Ok(html) => match extract_client_id(&html) {
                Some(id) => return Ok(id),
                None => errors.push("m.soundcloud.com: no client_id in page".to_string()),
            },
            Err(e) => errors.push(format!("m.soundcloud.com: {e:#}")),
        }

        match self.fetch_text("https://soundcloud.com/") {
            Ok(html) => {
                if let Some(id) = extract_client_id(&html) {
                    return Ok(id);
                }
                let scripts = extract_script_urls(&html);
                if scripts.is_empty() {
                    errors.push("soundcloud.com: no asset scripts found".to_string());
                }
                // The app bundle carrying the id is among the last scripts on the page.
                for src in scripts.iter().rev() {
                    match self.fetch_text(src) {
                        Ok(js) => {
                            if let Some(id) = extract_client_id(&js) {
                                return Ok(id);
                            }
                        }
                        Err(e) => errors.push(format!("{src}: {e:#}")),
                    }
                }
                errors.push("soundcloud.com: no client_id in asset scripts".to_string());
            }
            Err(e) => errors.push(format!("soundcloud.com: {e:#}")),
        }

        bail!(
            "could not discover a SoundCloud client_id ({})",
            errors.join("; ")
        )
    }

    fn fetch_text(&self, url: &str) -> anyhow::Result<String> {
        let resp = self.http.get(url).send()?;
        let status = resp.status();
        if !status.is_success() {
            bail!("HTTP {status}");
        }
        Ok(resp.text()?)
    }

    // ---- api-v2 ----------------------------------------------------------------------------

    /// GET an api-v2 URL as JSON with `client_id` (and the OAuth header when we have a token).
    /// On 401/403 the client_id is rediscovered once and the request retried.
    fn api_get(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<Value> {
        let mut url = Url::parse(url).with_context(|| format!("bad SoundCloud URL {url}"))?;
        // `next_href`s may already carry a (possibly stale) client_id; we add our own.
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| k != "client_id")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if pairs.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(pairs);
        }

        let mut retried = false;
        loop {
            let cid = self.client_id()?;
            let mut req = self
                .http
                .get(url.clone())
                .header(ACCEPT, "application/json")
                .query(&[("client_id", cid.as_str())])
                .query(query);
            if let Some(token) = &self.oauth {
                req = req.header(AUTHORIZATION, format!("OAuth {token}"));
            }
            let resp = req
                .send()
                .with_context(|| format!("SoundCloud request to {} failed", url.path()))?;
            let status = resp.status();
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                if !retried {
                    retried = true;
                    self.forget_client_id(&cid);
                    continue;
                }
                bail!(
                    "SoundCloud refused {} (HTTP {status}){}",
                    url.path(),
                    if self.oauth.is_some() {
                        " — the OAuth token may have expired, or the content is region-blocked"
                    } else {
                        " — the content may be private or region-blocked"
                    }
                );
            }
            return json_or_error(resp, url.path());
        }
    }

    fn fetch_track(&self, remote_id: &str) -> anyhow::Result<Value> {
        let id: u64 = remote_id
            .trim()
            .parse()
            .map_err(|_| anyhow!("invalid SoundCloud track id {remote_id:?}"))?;
        self.api_get(&format!("{API}/tracks/{id}"), &[])
    }
}

fn json_or_error(resp: Response, what: &str) -> anyhow::Result<Value> {
    let status = resp.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        bail!("SoundCloud rate limit hit (HTTP 429) on {what}; try again later");
    }
    if status == StatusCode::NOT_FOUND {
        bail!("SoundCloud: {what} not found (HTTP 404)");
    }
    if !status.is_success() {
        bail!("SoundCloud {what} failed: HTTP {status}");
    }
    resp.json::<Value>()
        .with_context(|| format!("SoundCloud {what}: invalid JSON"))
}

impl Provider for SoundCloud {
    fn id(&self) -> &'static str {
        "soundcloud"
    }

    /// Public tracks play in full without login; all we need is a working client_id.
    fn authorized(&self) -> bool {
        self.client_id().is_ok()
    }

    fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteTrack>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 200).to_string();
        let res = self.api_get(
            &format!("{API}/search/tracks"),
            &[
                ("q", query),
                ("limit", &limit),
                ("offset", "0"),
                ("linked_partitioning", "1"),
            ],
        )?;
        Ok(collection(&res).iter().filter_map(map_track).collect())
    }

    fn liked(&self) -> anyhow::Result<Vec<RemoteTrack>> {
        let me = self.me()?;
        let user_id = me
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("SoundCloud /me returned no user id"))?;

        let mut out = Vec::new();
        let mut url = format!("{API}/users/{user_id}/track_likes");
        let mut first = true;
        loop {
            let page = if first {
                self.api_get(
                    &url,
                    &[
                        ("limit", "200"),
                        ("offset", "0"),
                        ("linked_partitioning", "1"),
                    ],
                )?
            } else {
                self.api_get(&url, &[])?
            };
            first = false;
            let items = collection(&page);
            // Likes come newest first; each item wraps the track.
            out.extend(
                items
                    .iter()
                    .filter_map(|it| it.get("track"))
                    .filter_map(map_track),
            );
            if out.len() >= MAX_LIKES || items.is_empty() {
                break;
            }
            match page.get("next_href").and_then(Value::as_str) {
                Some(next) if !next.is_empty() => url = next.to_string(),
                _ => break,
            }
        }
        out.truncate(MAX_LIKES);
        Ok(out)
    }

    fn stream(&self, remote_id: &str) -> anyhow::Result<StreamSource> {
        let track = self.fetch_track(remote_id)?;
        match track.get("policy").and_then(Value::as_str) {
            Some("SNIP") => bail!(
                "SoundCloud only offers a 30-second preview of this track (full track requires Go+)"
            ),
            Some("BLOCK") => bail!("this SoundCloud track is not available in your region"),
            _ => {}
        }

        let transcodings = track
            .pointer("/media/transcodings")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let tc = choose_transcoding(transcodings).ok_or_else(|| {
            let offered: Vec<String> = transcodings
                .iter()
                .map(|t| {
                    format!(
                        "{}/{}",
                        t.pointer("/format/protocol")
                            .and_then(Value::as_str)
                            .unwrap_or("?"),
                        t.pointer("/format/mime_type")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                    )
                })
                .collect();
            anyhow!(
                "no playable SoundCloud stream for this track (offered: {})",
                offered.join(", ")
            )
        })?;

        let tc_url = tc
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("transcoding has no url"))?;
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(auth) = track.get("track_authorization").and_then(Value::as_str) {
            query.push(("track_authorization", auth));
        }
        let resolved = self.api_get(tc_url, &query)?;
        let url = resolved
            .get("url")
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
            .ok_or_else(|| anyhow!("SoundCloud did not return a stream URL"))?
            .to_string();

        let mime = tc
            .pointer("/format/mime_type")
            .and_then(Value::as_str)
            .map(str::to_string);
        let headers = vec![("User-Agent".to_string(), USER_AGENT.to_string())];
        let protocol = tc
            .pointer("/format/protocol")
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(if protocol == "progressive" {
            StreamSource::Http { url, headers, mime }
        } else {
            StreamSource::Hls {
                playlist_url: url,
                headers,
                mime,
            }
        })
    }

    fn search_artists(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteArtist>> {
        let l = limit.clamp(1, 50).to_string();
        let v = self.api_get(&format!("{API}/search/users"), &[("q", query.trim()), ("limit", &l)])?;
        Ok(collection(&v).iter().filter_map(map_user).collect())
    }

    fn artist(&self, remote_id: &str) -> anyhow::Result<ArtistPage> {
        let u = self.api_get(&format!("{API}/users/{remote_id}"), &[])?;
        let artist = map_user(&u).ok_or_else(|| anyhow::anyhow!("SoundCloud: user not found"))?;
        let description = str_field(&u, "description").filter(|d| !d.trim().is_empty());
        let top = self.api_get(&format!("{API}/users/{remote_id}/toptracks"), &[("limit", "20")])
            .or_else(|_| self.api_get(&format!("{API}/users/{remote_id}/tracks"), &[("limit", "20")]))?;
        let top_tracks = collection(&top).iter().filter_map(map_track).collect();
        let albums = self
            .api_get(&format!("{API}/users/{remote_id}/albums"), &[("limit", "30")])
            .map(|v| collection(&v).iter().filter_map(map_playlist).collect())
            .unwrap_or_default();
        Ok(ArtistPage { artist, description, top_tracks, albums, similar: vec![] })
    }

    fn album(&self, remote_id: &str) -> anyhow::Result<AlbumPage> {
        let p = self.api_get(&format!("{API}/playlists/{remote_id}"), &[])?;
        let album = map_playlist(&p).ok_or_else(|| anyhow::anyhow!("SoundCloud: playlist not found"))?;
        // Playlists list only the first few tracks in full; the rest are id stubs.
        let items: Vec<&Value> = p.get("tracks").and_then(Value::as_array).into_iter().flatten().collect();
        let mut tracks: Vec<RemoteTrack> = items.iter().filter_map(|t| map_track(t)).collect();
        let stubs: Vec<String> = items
            .iter()
            .filter(|t| t.get("title").is_none())
            .filter_map(|t| t.get("id").and_then(Value::as_u64).map(|i| i.to_string()))
            .collect();
        for chunk in stubs.chunks(50) {
            if let Ok(v) = self.api_get(&format!("{API}/tracks"), &[("ids", &chunk.join(","))]) {
                let full: Vec<RemoteTrack> = v.as_array().into_iter().flatten().filter_map(map_track).collect();
                tracks.extend(full);
            }
        }
        Ok(AlbumPage { album, tracks })
    }

    fn lyrics(&self, _remote_id: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

// ---- pure helpers --------------------------------------------------------------------------

static CLIENT_ID_RES: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        // desktop app bundle: `client_id:"…"` / `client_id="…"`
        r#"client_id\s*[:=]\s*"([a-zA-Z0-9]{32})""#,
        // m.soundcloud.com __NEXT_DATA__: `"clientId":"…"`
        r#""clientId"\s*:\s*"([a-zA-Z0-9]{32})""#,
        // a URL with the id baked in: `client_id=…`
        r#"client_id=([a-zA-Z0-9]{32})\b"#,
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid regex"))
    .collect()
});

static SCRIPT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<script[^>]*\ssrc="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#)
        .expect("valid regex")
});

static YEAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\d{4})").expect("valid regex"));

/// Find a 32-char client_id in a page or JS bundle.
fn extract_client_id(text: &str) -> Option<String> {
    CLIENT_ID_RES
        .iter()
        .find_map(|re| re.captures(text).map(|c| c[1].to_string()))
}

/// The `a-v2.sndcdn.com/assets/*.js` bundles referenced by soundcloud.com, in page order.
fn extract_script_urls(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in SCRIPT_RE.captures_iter(html) {
        let src = c[1].to_string();
        if !out.contains(&src) {
            out.push(src);
        }
    }
    out
}

/// `…-large.jpg` (100 px) → `…-t500x500.jpg`.
fn hires_artwork(url: &str) -> String {
    match url.rfind("-large.") {
        Some(i) => format!("{}-t500x500{}", &url[..i], &url[i + "-large".len()..]),
        None => url.to_string(),
    }
}

/// `"Artist - Title"` → `("Artist", "Title")`. Splits on the first spaced dash
/// (hyphen, en dash or em dash); both sides must be non-empty.
fn split_artist_title(title: &str) -> Option<(String, String)> {
    let (i, sep) = [" - ", " – ", " — "]
        .iter()
        .filter_map(|sep| title.find(sep).map(|i| (i, *sep)))
        .min_by_key(|(i, _)| *i)?;
    let artist = title[..i].trim();
    let rest = title[i + sep.len()..].trim();
    (!artist.is_empty() && !rest.is_empty()).then(|| (artist.to_string(), rest.to_string()))
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn collection(v: &Value) -> &[Value] {
    v.get("collection")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn parse_year(s: &str) -> Option<u32> {
    YEAR_RE
        .captures(s.trim())
        .and_then(|c| c[1].parse::<u32>().ok())
        .filter(|y| (1900..=2100).contains(y))
}

/// api-v2 track JSON → `RemoteTrack`. `None` for things that aren't playable tracks.
fn map_track(t: &Value) -> Option<RemoteTrack> {
    if t.get("kind")
        .and_then(Value::as_str)
        .is_some_and(|k| k != "track")
    {
        return None;
    }
    let id = t.get("id").and_then(Value::as_u64)?;
    let raw_title = str_field(t, "title").unwrap_or_default();
    let meta = t.get("publisher_metadata").filter(|m| m.is_object());
    let meta_artist = meta.and_then(|m| str_field(m, "artist"));
    let username = t.get("user").and_then(|u| str_field(u, "username"));

    let (artist, title) = match (meta_artist, split_artist_title(&raw_title)) {
        // Publisher metadata is authoritative; still strip the artist if the title repeats it.
        (Some(artist), Some((a, rest))) if a.eq_ignore_ascii_case(&artist) => (artist, rest),
        (Some(artist), Some((before, b))) if b.eq_ignore_ascii_case(&artist) => (artist, before),
        (Some(artist), _) => (artist, raw_title),
        (None, Some((a, rest))) => (a, rest),
        (None, None) => (username.unwrap_or_default(), raw_title),
    };

    let album = meta
        .and_then(|m| str_field(m, "album_title"))
        .unwrap_or_default();
    let isrc = meta.and_then(|m| str_field(m, "isrc"));
    let cover_url = str_field(t, "artwork_url")
        .or_else(|| t.get("user").and_then(|u| str_field(u, "avatar_url")))
        .map(|u| hires_artwork(&u));
    // SNIP tracks report the 30 s preview as `duration`; `full_duration` is the real length.
    let duration_ms = t
        .get("full_duration")
        .and_then(Value::as_u64)
        .filter(|d| *d > 0)
        .or_else(|| t.get("duration").and_then(Value::as_u64))
        .unwrap_or(0);
    let year = str_field(t, "release_date")
        .and_then(|d| parse_year(&d))
        .or_else(|| str_field(t, "created_at").and_then(|d| parse_year(&d)));

    Some(RemoteTrack {
        source: "soundcloud".to_string(),
        remote_id: id.to_string(),
        title,
        artist,
        album,
        duration_ms,
        cover_url,
        isrc,
        year,
        genre: str_field(t, "genre"),
        artist_id: t.get("user").and_then(|u| u.get("id")).and_then(Value::as_u64).map(|i| i.to_string()),
        album_id: None,
    })
}

fn map_user(u: &Value) -> Option<RemoteArtist> {
    Some(RemoteArtist {
        source: "soundcloud".into(),
        remote_id: u.get("id").and_then(Value::as_u64)?.to_string(),
        name: str_field(u, "username")?,
        image_url: str_field(u, "avatar_url").map(|a| hires_artwork(&a)),
        genres: vec![],
        followers: u.get("followers_count").and_then(Value::as_u64),
    })
}

fn map_playlist(p: &Value) -> Option<RemoteAlbum> {
    Some(RemoteAlbum {
        source: "soundcloud".into(),
        remote_id: p.get("id").and_then(Value::as_u64)?.to_string(),
        title: str_field(p, "title")?,
        artist: p.get("user").and_then(|u| str_field(u, "username")).unwrap_or_default(),
        year: str_field(p, "release_date").and_then(|d| parse_year(&d)).or_else(|| str_field(p, "created_at").and_then(|d| parse_year(&d))),
        cover_url: str_field(p, "artwork_url").map(|a| hires_artwork(&a)),
        track_count: p.get("track_count").and_then(Value::as_u64).map(|n| n as u32),
        kind: str_field(p, "set_type").filter(|s| !s.is_empty()).or(Some("album".into())),
    })
}

/// Rank: progressive MP3, HLS MP3, other HLS (best quality first), with the adaptive master
/// playlist (`audio/mpegurl`) last. Encrypted / unknown protocols are never chosen.
fn choose_transcoding(transcodings: &[Value]) -> Option<&Value> {
    fn quality_rank(tc: &Value) -> u8 {
        match tc.get("quality").and_then(Value::as_str) {
            Some("hq") => 0,
            Some("sq") => 1,
            Some("lq") => 3,
            _ => 2,
        }
    }
    fn tier(tc: &Value) -> Option<u8> {
        if tc
            .get("url")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return None;
        }
        let protocol = tc.pointer("/format/protocol").and_then(Value::as_str)?;
        let mime = tc
            .pointer("/format/mime_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mpeg = mime.starts_with("audio/mpeg") && !mime.starts_with("audio/mpegurl");
        match protocol {
            "progressive" if mpeg => Some(0),
            "progressive" => Some(1),
            "hls" if mpeg => Some(2),
            "hls" if mime.starts_with("audio/mpegurl") || mime.contains("mpegurl") => Some(4),
            "hls" => Some(3),
            _ => None,
        }
    }
    transcodings
        .iter()
        .filter_map(|tc| tier(tc).map(|t| (t, quality_rank(tc), tc)))
        .min_by_key(|(t, q, _)| (*t, *q))
        .map(|(_, _, tc)| tc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn client_id_from_js_bundle() {
        let js = r#"…,e.exports={apiHost:"api-v2.soundcloud.com",client_id:"aBcDeFgHiJkLmNoPqRsTuVwXyZ012345",env:"production"}"#;
        assert_eq!(
            extract_client_id(js).as_deref(),
            Some("aBcDeFgHiJkLmNoPqRsTuVwXyZ012345")
        );
        let js2 = r#"n.set("client_id","x"),t={client_id: "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"}"#;
        assert_eq!(
            extract_client_id(js2).as_deref(),
            Some("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ")
        );
    }

    #[test]
    fn client_id_from_mobile_page_and_url() {
        let html = r#"<script id="__NEXT_DATA__">{"runtimeConfig":{"clientId":"KKzJxmw11tYpCs6T24P4uUYhqmjalG6M"}}</script>"#;
        assert_eq!(
            extract_client_id(html).as_deref(),
            Some("KKzJxmw11tYpCs6T24P4uUYhqmjalG6M")
        );
        let url = "https://api-v2.soundcloud.com/x?client_id=0123456789abcdef0123456789ABCDEF&a=b";
        assert_eq!(
            extract_client_id(url).as_deref(),
            Some("0123456789abcdef0123456789ABCDEF")
        );
    }

    #[test]
    fn client_id_rejects_wrong_length() {
        assert_eq!(extract_client_id(r#"client_id:"tooShort123""#), None);
        assert_eq!(extract_client_id("no id here"), None);
    }

    #[test]
    fn script_urls_from_homepage() {
        let html = r#"<script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>
            <script src="https://example.com/x.js"></script>
            <script crossorigin src="https://a-v2.sndcdn.com/assets/49-def.js"></script>
            <script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>"#;
        assert_eq!(
            extract_script_urls(html),
            vec![
                "https://a-v2.sndcdn.com/assets/0-abc.js",
                "https://a-v2.sndcdn.com/assets/49-def.js"
            ]
        );
    }

    #[test]
    fn artwork_rewrite() {
        assert_eq!(
            hires_artwork("https://i1.sndcdn.com/artworks-000045765973-c2x8ko-large.jpg"),
            "https://i1.sndcdn.com/artworks-000045765973-c2x8ko-t500x500.jpg"
        );
        assert_eq!(
            hires_artwork("https://i1.sndcdn.com/avatars-x7ck1t-large.png"),
            "https://i1.sndcdn.com/avatars-x7ck1t-t500x500.png"
        );
        assert_eq!(
            hires_artwork("https://i1.sndcdn.com/a-t500x500.jpg"),
            "https://i1.sndcdn.com/a-t500x500.jpg"
        );
    }

    #[test]
    fn title_split() {
        assert_eq!(
            split_artist_title("Radiohead - Creep (slowed)"),
            Some(("Radiohead".into(), "Creep (slowed)".into()))
        );
        assert_eq!(
            split_artist_title("Artist – Song - Remix"),
            Some(("Artist".into(), "Song - Remix".into()))
        );
        assert_eq!(split_artist_title("Just a title"), None);
        assert_eq!(split_artist_title("Hyphen-ated-title"), None);
        assert_eq!(split_artist_title(" - Title"), None);
    }

    #[test]
    fn year_parse() {
        assert_eq!(parse_year("1997-01-17T00:00:00Z"), Some(1997));
        assert_eq!(parse_year("2013/04/17 22:05:25 +0000"), Some(2013));
        assert_eq!(parse_year("soon"), None);
    }

    fn fixture() -> Value {
        json!({
            "kind": "track",
            "id": 88335161,
            "title": "Get Lucky [Original Version] - Daft Punk",
            "duration": 246381,
            "full_duration": 246381,
            "genre": "Disco Electronic",
            "release_date": null,
            "created_at": "2013-04-17T22:05:25Z",
            "artwork_url": "https://i1.sndcdn.com/artworks-000045765973-c2x8ko-large.jpg",
            "policy": "ALLOW",
            "publisher_metadata": {
                "id": 88335161, "artist": "Daft Punk", "isrc": "GBSMU1355464",
                "album_title": "Random Access Memories"
            },
            "user": { "username": "DJ KB", "avatar_url": "https://i1.sndcdn.com/avatars-1-large.jpg" }
        })
    }

    #[test]
    fn maps_track_with_publisher_metadata() {
        let t = map_track(&fixture()).unwrap();
        assert_eq!(
            t,
            RemoteTrack {
                source: "soundcloud".into(),
                remote_id: "88335161".into(),
                title: "Get Lucky [Original Version]".into(),
                artist: "Daft Punk".into(),
                album: "Random Access Memories".into(),
                duration_ms: 246381,
                cover_url: Some(
                    "https://i1.sndcdn.com/artworks-000045765973-c2x8ko-t500x500.jpg".into()
                ),
                isrc: Some("GBSMU1355464".into()),
                year: Some(2013),
                genre: Some("Disco Electronic".into()),
                artist_id: None,
                album_id: None,
            }
        );
    }

    #[test]
    fn maps_track_without_metadata() {
        let mut v = fixture();
        v["publisher_metadata"] = Value::Null;
        v["title"] = json!("Some Remix");
        v["artwork_url"] = Value::Null;
        v["genre"] = json!("");
        v["release_date"] = json!("1997-01-17T00:00:00Z");
        v["full_duration"] = json!(300000);
        v["duration"] = json!(30000);
        let t = map_track(&v).unwrap();
        assert_eq!(t.artist, "DJ KB");
        assert_eq!(t.title, "Some Remix");
        assert_eq!(t.album, "");
        assert_eq!(
            t.cover_url.as_deref(),
            Some("https://i1.sndcdn.com/avatars-1-t500x500.jpg")
        );
        assert_eq!(t.genre, None);
        assert_eq!(t.year, Some(1997));
        assert_eq!(t.duration_ms, 300000);

        v["title"] = json!("Radiohead - Creep (slowed)");
        let t = map_track(&v).unwrap();
        assert_eq!(
            (t.artist.as_str(), t.title.as_str()),
            ("Radiohead", "Creep (slowed)")
        );
    }

    #[test]
    fn provider_basics() {
        let sc = SoundCloud::new(Some("  ".into()));
        assert_eq!(sc.id(), "soundcloud");
        assert!(sc.oauth.is_none());
        assert_eq!(sc.lyrics("123").unwrap(), None);
        assert!(sc.liked().is_err());
        assert!(sc.account_name().is_err());
    }

    #[test]
    fn skips_non_tracks() {
        assert_eq!(map_track(&json!({"kind": "playlist", "id": 1})), None);
        assert_eq!(map_track(&json!({"kind": "track"})), None);
    }

    fn tc(protocol: &str, mime: &str, preset: &str, quality: &str) -> Value {
        json!({
            "url": format!("https://api-v2.soundcloud.com/media/x/{preset}/stream/{protocol}"),
            "preset": preset, "quality": quality,
            "format": { "protocol": protocol, "mime_type": mime }
        })
    }

    fn preset_of(v: Option<&Value>) -> Option<(&str, &str)> {
        v.map(|v| {
            (
                v["preset"].as_str().unwrap(),
                v["format"]["protocol"].as_str().unwrap(),
            )
        })
    }

    #[test]
    fn transcoding_choice() {
        let aac = r#"audio/mp4; codecs="mp4a.40.2""#;
        // Legacy upload: progressive MP3 wins.
        let legacy = [
            tc("hls", "audio/mpeg", "mp3_1_0", "sq"),
            tc("progressive", "audio/mpeg", "mp3_1_0", "sq"),
        ];
        assert_eq!(
            preset_of(choose_transcoding(&legacy)),
            Some(("mp3_1_0", "progressive"))
        );
        // HLS MP3 before AAC.
        let mixed = [
            tc("hls", aac, "aac_160k", "sq"),
            tc("hls", "audio/mpeg", "mp3_0_1", "sq"),
        ];
        assert_eq!(
            preset_of(choose_transcoding(&mixed)),
            Some(("mp3_0_1", "hls"))
        );
        // Today's typical track: AAC 160k over 96k over the ABR master playlist.
        let modern = [
            tc("hls", aac, "aac_96k", "lq"),
            tc("hls", "audio/mpegurl", "abr_sq", "sq"),
            tc("hls", aac, "aac_160k", "sq"),
        ];
        assert_eq!(
            preset_of(choose_transcoding(&modern)),
            Some(("aac_160k", "hls"))
        );
        let abr_only = [tc("hls", "audio/mpegurl", "abr_sq", "sq")];
        assert_eq!(
            preset_of(choose_transcoding(&abr_only)),
            Some(("abr_sq", "hls"))
        );
        // DRM-encrypted variants are never picked.
        let drm = [
            tc("cbc-encrypted-hls", aac, "aac_160k", "sq"),
            tc("ctr-encrypted-hls", aac, "aac_160k", "sq"),
        ];
        assert!(choose_transcoding(&drm).is_none());
        assert!(choose_transcoding(&[]).is_none());
    }

    // ---- network (run with `cargo test --lib providers::soundcloud -- --ignored`) -----------

    fn is_audio_prefix(bytes: &[u8]) -> bool {
        let id3 = bytes.starts_with(b"ID3");
        let mpeg_sync = bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0;
        let mp4 = bytes.len() >= 8 && matches!(&bytes[4..8], b"ftyp" | b"styp" | b"moof" | b"sidx");
        id3 || mpeg_sync || mp4
    }

    /// GET the first `max` bytes of `url` as a series of 8 KiB range requests, each on a fresh
    /// connection. Some networks (seen from Russia against SoundCloud's CloudFront media host)
    /// freeze any single TCP connection after ~16 KB on the wire, so one plain 64 KB GET stalls
    /// there. If the server ignores `Range` (the signed playlists do), or a transfer stalls,
    /// whatever prefix arrived is returned.
    fn ranged_get_max(url: &str, headers: &[(String, String)], max: usize) -> Vec<u8> {
        use std::io::Read;
        const CHUNK: usize = 8 * 1024;
        let mut out: Vec<u8> = Vec::new();
        while out.len() < max {
            let client = Client::builder()
                .timeout(Duration::from_secs(10))
                .pool_max_idle_per_host(0)
                .build()
                .unwrap();
            let want = CHUNK.min(max - out.len());
            let range = format!("bytes={}-{}", out.len(), out.len() + want - 1);
            let mut req = client.get(url).header("Range", range);
            for (k, v) in headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let mut resp = req.send().unwrap();
            if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                break;
            }
            assert!(resp.status().is_success(), "GET {url}: {}", resp.status());
            let whole = resp.status() == StatusCode::OK;
            let limit = if whole { max } else { want };
            let mut buf = [0u8; 4096];
            let mut got = 0;
            let mut stalled = false;
            while got < limit {
                match resp.read(&mut buf[..(limit - got).min(4096)]) {
                    Ok(0) => break,
                    Ok(n) => {
                        out.extend_from_slice(&buf[..n]);
                        got += n;
                    }
                    Err(e) => {
                        println!("  (transfer stalled after {got} bytes: {e})");
                        stalled = true;
                        break;
                    }
                }
            }
            if whole || stalled || got < want {
                break; // range ignored, stalled, or end of file
            }
        }
        out.truncate(max);
        out
    }

    fn ranged_get(url: &str, headers: &[(String, String)]) -> Vec<u8> {
        ranged_get_max(url, headers, 64 * 1024)
    }

    #[test]
    #[ignore]
    fn net_discovers_client_id() {
        let sc = SoundCloud::new(None);
        let id = sc.client_id().unwrap();
        println!("client_id = {id}");
        assert_eq!(id.len(), 32);
        assert!(sc.authorized());
    }

    #[test]
    #[ignore]
    fn net_search() {
        let sc = SoundCloud::new(None);
        let res = sc.search("daft punk", 10).unwrap();
        assert!(!res.is_empty());
        for t in &res {
            println!(
                "{} | {} | {} | {:?} | {} ms | {:?}",
                t.remote_id, t.artist, t.title, t.year, t.duration_ms, t.cover_url
            );
            assert_eq!(t.source, "soundcloud");
            assert!(t.remote_id.parse::<u64>().is_ok());
        }
    }

    #[test]
    #[ignore]
    fn net_stream_resolves_to_audio() {
        let sc = SoundCloud::new(None);
        let res = sc.search("daft punk", 20).unwrap();
        let mut played = false;
        // Skip hour-long mixes: their playlists alone are hundreds of KB.
        for t in res.iter().filter(|t| t.duration_ms < 15 * 60 * 1000) {
            match sc.stream(&t.remote_id) {
                Ok(StreamSource::Http { url, headers, mime }) => {
                    println!("{}: progressive {mime:?}", t.remote_id);
                    let bytes = ranged_get(&url, &headers);
                    assert!(
                        is_audio_prefix(&bytes),
                        "not audio: {:02x?}",
                        &bytes[..bytes.len().min(16)]
                    );
                    played = true;
                }
                Ok(StreamSource::Hls {
                    playlist_url,
                    headers,
                    mime,
                }) => {
                    println!("{}: HLS {mime:?}", t.remote_id);
                    // Long mixes have playlists of 1 MB+ (each segment URL is signed, ~1.6 KB).
                    let started = std::time::Instant::now();
                    let playlist =
                        String::from_utf8_lossy(&ranged_get_max(&playlist_url, &headers, 8 << 20))
                            .into_owned();
                    println!(
                        "  playlist: {} bytes in {:?}",
                        playlist.len(),
                        started.elapsed()
                    );
                    assert!(
                        playlist.starts_with("#EXTM3U"),
                        "not a playlist: {playlist:.200}"
                    );
                    let map = Regex::new(r#"#EXT-X-MAP:URI="([^"]+)""#).unwrap();
                    if let Some(init) = map.captures(&playlist) {
                        let bytes = ranged_get(&init[1], &headers);
                        println!(
                            "  init segment: {} bytes, box {:?}",
                            bytes.len(),
                            String::from_utf8_lossy(&bytes[4..8])
                        );
                        assert!(is_audio_prefix(&bytes));
                    }
                    let first = playlist
                        .lines()
                        .find(|l| !l.is_empty() && !l.starts_with('#'))
                        .unwrap();
                    let bytes = ranged_get(first, &headers);
                    println!(
                        "  {} segments seen, key={}, first segment: {} bytes read, starts {:02x?}",
                        playlist.matches("#EXTINF").count(),
                        playlist.contains("#EXT-X-KEY"),
                        bytes.len(),
                        &bytes[..8]
                    );
                    assert!(
                        is_audio_prefix(&bytes),
                        "not audio: {:02x?}",
                        &bytes[..bytes.len().min(16)]
                    );
                    played = true;
                }
                Ok(StreamSource::Spotify { .. }) => unreachable!(),
                Err(e) => println!("{}: {e:#}", t.remote_id),
            }
            if played {
                break;
            }
        }
        assert!(played, "no search result resolved to a playable stream");
    }

    /// Needs a real token: `SOUNDCLOUD_OAUTH=<token> cargo test … -- --ignored net_account`.
    #[test]
    #[ignore]
    fn net_account_and_likes() {
        let Ok(token) = std::env::var("SOUNDCLOUD_OAUTH") else {
            println!("SOUNDCLOUD_OAUTH not set; skipping");
            assert!(SoundCloud::new(None).liked().is_err());
            return;
        };
        let sc = SoundCloud::new(Some(token));
        println!("account: {}", sc.account_name().unwrap());
        let liked = sc.liked().unwrap();
        println!(
            "{} liked tracks; newest: {:?}",
            liked.len(),
            liked.first().map(|t| &t.title)
        );
    }

    #[test]
    #[ignore]
    fn net_snip_track_is_rejected() {
        let sc = SoundCloud::new(None);
        // Label-uploaded tracks are often previews for non-Go+ listeners.
        let cid = sc.client_id().unwrap();
        let res: Value = Client::new()
            .get(format!("{API}/search/tracks"))
            .query(&[
                ("q", "techno mix"),
                ("limit", "50"),
                ("client_id", cid.as_str()),
            ])
            .header("User-Agent", USER_AGENT)
            .send()
            .unwrap()
            .json()
            .unwrap();
        let Some(snip) = collection(&res).iter().find(|t| t["policy"] == "SNIP") else {
            println!("no SNIP track in results; skipping");
            return;
        };
        let err = sc
            .stream(&snip["id"].as_u64().unwrap().to_string())
            .unwrap_err();
        println!("{err:#}");
        assert!(format!("{err}").contains("30-second preview"));
    }
}
