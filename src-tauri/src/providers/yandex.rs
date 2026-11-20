//! Yandex Music provider, built on the unofficial `api.music.yandex.net` API used by the
//! official apps. Endpoints, signing keys and JSON shapes follow MarshalX/yandex-music-api.
//!
//! * Search and (preview) streaming work anonymously.
//! * Liked tracks, lyrics and full-length streams need an OAuth token, obtained through the
//!   implicit-grant flow at [`OAUTH_URL`] and parsed with [`token_from_redirect`].
//! * Without a Plus subscription the stream endpoints only offer 30 s previews.

// Nothing calls into `providers` yet; drop this once the provider is wired into commands.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use parking_lot::Mutex;
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;
use sha2::Sha256;

use super::{Provider, RemoteTrack, StreamSource};

/// Implicit-grant login for the official Yandex Music client. After login Yandex redirects to
/// `https://music.yandex.ru/#access_token=...`; feed that URL to [`token_from_redirect`].
pub const OAUTH_URL: &str =
    "https://oauth.yandex.ru/authorize?response_type=token&client_id=23cabbbdc6cd418abb4b39c32c41195d";

const API: &str = "https://api.music.yandex.net";
/// Client header of the Android app; the API keys its behaviour (and the lyrics key) off it.
const ANDROID_CLIENT: &str = "YandexMusicAndroid/24023621";
/// `/get-file-info` signatures are only accepted together with this client header.
const DESKTOP_CLIENT: &str = "YandexMusicDesktopAppWindows/5.95.0";
const USER_AGENT: &str = "Yandex-Music-API";
/// Salt of the classic `download-info` → `get-mp3` link signature.
const MP3_SIGN_SALT: &str = "XGRlBW9FXlekgbPrRHuSiA";
/// HMAC key for `/tracks/{id}/lyrics` (Android app).
const LYRICS_SIGN_KEY: &str = "p93jhgh689SBReK6ghtw62";
/// HMAC key for `/get-file-info` (Windows desktop app).
const FILE_INFO_SIGN_KEY: &str = "kzqU4XhfCaY6B6JTHODeq5";
const COVER_SIZE: &str = "1000x1000";
const TRACKS_BATCH: usize = 100;
const MAX_SEARCH_PAGES: u32 = 5;

pub struct Yandex {
    token: Option<String>,
    uid: Mutex<Option<u64>>,
    /// Built lazily so `new()` is safe to call from inside an async runtime.
    client: OnceLock<Client>,
}

impl Yandex {
    pub fn new(token: Option<String>) -> Self {
        let token = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        Self { token, uid: Mutex::new(None), client: OnceLock::new() }
    }

    /// Display name for the settings UI: display name, else login, else full name.
    pub fn account_name(&self) -> anyhow::Result<String> {
        let account = self.account_status()?;
        ["displayName", "login", "fullName"]
            .iter()
            .filter_map(|k| account[*k].as_str())
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Yandex: not logged in"))
    }

    fn http(&self) -> &Client {
        self.client.get_or_init(|| {
            Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client")
        })
    }

    /// A request to the API with the auth and client headers.
    fn api(&self, method: reqwest::Method, path: &str, client_header: &str) -> RequestBuilder {
        let mut rb = self
            .http()
            .request(method, format!("{API}{path}"))
            .header("X-Yandex-Music-Client", client_header)
            .header("Accept-Language", "ru");
        if let Some(t) = &self.token {
            rb = rb.header("Authorization", format!("OAuth {t}"));
        }
        rb
    }

    fn get(&self, path: &str) -> RequestBuilder {
        self.api(reqwest::Method::GET, path, ANDROID_CLIENT)
    }

    /// Sends a request; returns the status and the unwrapped `result` (or the whole body).
    fn send_raw(&self, rb: RequestBuilder) -> anyhow::Result<(StatusCode, Value)> {
        let resp = rb.send().context("Yandex: request failed")?;
        let status = resp.status();
        let text = resp.text().context("Yandex: reading response")?;
        let mut json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let body = match json.get_mut("result") {
            Some(r) if !r.is_null() => r.take(),
            _ => json,
        };
        Ok((status, body))
    }

    fn send(&self, rb: RequestBuilder) -> anyhow::Result<Value> {
        let (status, body) = self.send_raw(rb)?;
        if !status.is_success() {
            bail!(api_error(status, &body));
        }
        Ok(body)
    }

    fn account_status(&self) -> anyhow::Result<Value> {
        if self.token.is_none() {
            bail!("Yandex: not logged in");
        }
        let mut status = self.send(self.get("/account/status"))?;
        let account = status["account"].take();
        if let Some(uid) = account["uid"].as_u64() {
            *self.uid.lock() = Some(uid);
        }
        Ok(account)
    }

    fn uid(&self) -> anyhow::Result<u64> {
        if let Some(uid) = *self.uid.lock() {
            return Ok(uid);
        }
        self.account_status()?["uid"]
            .as_u64()
            .ok_or_else(|| anyhow!("Yandex: token is invalid or expired"))
    }

    /// Full track objects for `ids` (`"<id>"` or `"<id>:<albumId>"`), in API order.
    fn tracks(&self, ids: &[String]) -> anyhow::Result<Vec<Value>> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(TRACKS_BATCH) {
            let body = format!("track-ids={}&with-positions=False", form_encode(&chunk.join(",")));
            let rb = self
                .api(reqwest::Method::POST, "/tracks", ANDROID_CLIENT)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body);
            match self.send(rb)? {
                Value::Array(items) => out.extend(items),
                other => bail!("Yandex: unexpected /tracks response: {other}"),
            }
        }
        Ok(out)
    }

    /// Classic flow: download-info → XML → md5-signed `get-mp3` link.
    fn stream_classic(&self, track_id: &str) -> anyhow::Result<StreamSource> {
        let infos = self.send(self.get(&format!("/tracks/{track_id}/download-info")))?;
        let best = pick_download_info(&infos).ok_or_else(|| anyhow!("Yandex: no playable mp3 for this track"))?;
        let info_url = best["downloadInfoUrl"].as_str().ok_or_else(|| anyhow!("Yandex: no downloadInfoUrl"))?;
        let xml = self
            .http()
            .get(info_url)
            .header("X-Yandex-Music-Client", ANDROID_CLIENT)
            .send()?
            .error_for_status()?
            .text()?;
        let url = mp3_url_from_xml(&xml).ok_or_else(|| anyhow!("Yandex: malformed download-info XML"))?;
        Ok(StreamSource::Http { url, headers: Vec::new(), mime: Some("audio/mpeg".into()) })
    }

    /// Newer signed endpoint used by the desktop app. Fallback only: the classic flow is
    /// verified to work and needs no client-specific key.
    fn stream_file_info(&self, track_id: &str) -> anyhow::Result<StreamSource> {
        let (quality, codecs, transport) = ("hq", ["mp3"], "raw");
        let ts = unix_now();
        let sign = file_info_sign(ts, track_id, quality, &codecs, transport);
        let rb = self.api(reqwest::Method::GET, "/get-file-info", DESKTOP_CLIENT).query(&[
            ("ts", ts.to_string()),
            ("trackId", track_id.to_string()),
            ("quality", quality.to_string()),
            ("codecs", codecs.join(",")),
            ("transports", transport.to_string()),
            ("sign", sign),
        ]);
        let info = self.send(rb)?;
        let info = &info["downloadInfo"];
        if info["transport"].as_str().is_some_and(|t| t != "raw") {
            bail!("Yandex: get-file-info returned an encrypted stream");
        }
        let url = info["url"].as_str().ok_or_else(|| anyhow!("Yandex: get-file-info without url"))?;
        let mime = match info["codec"].as_str().unwrap_or("mp3") {
            "mp3" => "audio/mpeg",
            "aac" | "he-aac" => "audio/aac",
            "flac" => "audio/flac",
            _ => "audio/mp4",
        };
        Ok(StreamSource::Http { url: url.to_string(), headers: Vec::new(), mime: Some(mime.into()) })
    }

    fn fetch_text(&self, url: &str) -> anyhow::Result<String> {
        Ok(self.http().get(url).send()?.error_for_status()?.text()?)
    }
}

impl Provider for Yandex {
    fn id(&self) -> &'static str {
        "yandex"
    }

    fn authorized(&self) -> bool {
        self.token.is_some()
    }

    fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<RemoteTrack>> {
        let query = query.trim();
        let mut out = Vec::new();
        if query.is_empty() || limit == 0 {
            return Ok(out);
        }
        for page in 0..MAX_SEARCH_PAGES {
            let rb = self.get("/search").query(&[
                ("text", query.to_string()),
                ("type", "track".into()),
                ("page", page.to_string()),
                ("nocorrect", "false".into()),
            ]);
            let result = self.send(rb)?;
            let results = result["tracks"]["results"].as_array().map(Vec::as_slice).unwrap_or_default();
            out.extend(results.iter().filter_map(map_track));
            let total = result["tracks"]["total"].as_u64().unwrap_or(0) as usize;
            if results.is_empty() || out.len() >= limit || out.len() >= total {
                break;
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    fn liked(&self) -> anyhow::Result<Vec<RemoteTrack>> {
        let uid = self.uid()?;
        let rb = self
            .get(&format!("/users/{uid}/likes/tracks"))
            .query(&[("if-modified-since-revision", "0")]);
        let likes = self.send(rb)?;
        let mut items: Vec<(String, Option<String>, String)> = likes["library"]["tracks"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|t| {
                let id = id_string(&t["id"])?;
                let album = id_string(&t["albumId"]);
                let ts = t["timestamp"].as_str().unwrap_or_default().to_string();
                Some((id, album, ts))
            })
            .collect();
        // The API already lists newest first; the stable sort only guards against that changing.
        items.sort_by(|a, b| b.2.cmp(&a.2));

        let keys: Vec<String> = items
            .iter()
            .map(|(id, album, _)| match album {
                Some(a) => format!("{id}:{a}"),
                None => id.clone(),
            })
            .collect();
        let mut by_id: HashMap<String, RemoteTrack> =
            self.tracks(&keys)?.iter().filter_map(map_track).map(|t| (t.remote_id.clone(), t)).collect();
        Ok(items.iter().filter_map(|(id, _, _)| by_id.remove(id)).collect())
    }

    fn stream(&self, remote_id: &str) -> anyhow::Result<StreamSource> {
        let track_id = remote_id.split(':').next().unwrap_or(remote_id);
        match self.stream_classic(track_id) {
            Ok(s) => Ok(s),
            Err(e) if self.authorized() => self
                .stream_file_info(track_id)
                .map_err(|e2| anyhow!("{e:#}; get-file-info fallback: {e2:#}")),
            Err(e) => Err(e),
        }
    }

    fn lyrics(&self, remote_id: &str) -> anyhow::Result<Option<String>> {
        // User-uploaded tracks have non-numeric ids and never have lyrics.
        let Some(track_id) = track_number(remote_id) else { return Ok(None) };
        if self.token.is_none() {
            bail!("Yandex: lyrics require login");
        }
        for format in ["LRC", "TEXT"] {
            let ts = unix_now();
            let rb = self.get(&format!("/tracks/{track_id}/lyrics")).query(&[
                ("format", format.to_string()),
                ("timeStamp", ts.to_string()),
                ("sign", lyrics_sign(track_id, ts)),
            ]);
            let (status, body) = self.send_raw(rb)?;
            if status == StatusCode::NOT_FOUND {
                continue;
            }
            if !status.is_success() {
                bail!(api_error(status, &body));
            }
            let Some(url) = body["downloadUrl"].as_str() else { continue };
            let text = self.fetch_text(url)?;
            if !text.trim().is_empty() {
                return Ok(Some(text));
            }
        }
        Ok(None)
    }
}

/// Extracts `access_token` from an OAuth redirect (`...#access_token=XXX&token_type=bearer...`).
/// Also accepts the token in the query string.
pub fn token_from_redirect(url: &str) -> Option<String> {
    let params = match url.split_once('#') {
        Some((_, frag)) => frag,
        None => url.split_once('?')?.1,
    };
    params
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "access_token")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn api_error(status: StatusCode, body: &Value) -> anyhow::Error {
    let err = &body["error"];
    let msg = err["message"]
        .as_str()
        .or_else(|| err["name"].as_str())
        .or_else(|| err.as_str())
        .or_else(|| body["errorDescription"].as_str())
        .unwrap_or("");
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            anyhow!("Yandex: unauthorized ({status}) {msg}")
        }
        _ => anyhow!("Yandex: HTTP {status} {msg}"),
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `"123"` / `"123:456"` → 123. `None` for non-numeric (user-uploaded) ids.
fn track_number(id: &str) -> Option<u64> {
    id.split(':').next()?.trim().parse().ok()
}

/// Ids come as JSON numbers or strings depending on the endpoint.
fn id_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Percent-encodes a form value (everything but unreserved characters).
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// md5 signature of a `get-mp3` link: `md5(salt + path[1:] + s)`.
fn mp3_sign(path: &str, s: &str) -> String {
    let mut h = Md5::new();
    h.update(MP3_SIGN_SALT.as_bytes());
    h.update(path.strip_prefix('/').unwrap_or(path).as_bytes());
    h.update(s.as_bytes());
    hex(&h.finalize())
}

fn hmac_b64(key: &str, msg: &str) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key");
    mac.update(msg.as_bytes());
    BASE64.encode(mac.finalize().into_bytes())
}

/// Lyrics signature: `base64(HMAC-SHA256(key, "<trackId><timestamp>"))`.
fn lyrics_sign(track_id: u64, ts: u64) -> String {
    hmac_b64(LYRICS_SIGN_KEY, &format!("{track_id}{ts}"))
}

/// get-file-info signature: base64 HMAC of `"<ts><trackId><quality><codecs…><transport>"`
/// with the trailing `=` padding char dropped.
fn file_info_sign(ts: u64, track_id: &str, quality: &str, codecs: &[&str], transport: &str) -> String {
    let mut s = hmac_b64(FILE_INFO_SIGN_KEY, &format!("{ts}{track_id}{quality}{}{transport}", codecs.concat()));
    s.pop();
    s
}

fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].trim())
}

fn mp3_url_from_xml(xml: &str) -> Option<String> {
    let host = xml_tag(xml, "host")?;
    let path = xml_tag(xml, "path")?;
    let ts = xml_tag(xml, "ts")?;
    let s = xml_tag(xml, "s")?;
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("https://{host}/get-mp3/{}/{ts}{path}", mp3_sign(path, s)))
}

/// Best mp3 entry of a download-info list: full tracks before previews, then highest bitrate.
fn pick_download_info(infos: &Value) -> Option<&Value> {
    infos
        .as_array()?
        .iter()
        .filter(|i| i["codec"].as_str() == Some("mp3") && i["downloadInfoUrl"].is_string())
        .max_by_key(|i| (!i["preview"].as_bool().unwrap_or(false), i["bitrateInKbps"].as_u64().unwrap_or(0)))
}

/// `avatars.yandex.net/get-music-content/.../%%` → `https://avatars.yandex.net/.../1000x1000`.
fn cover_url(uri: &str) -> Option<String> {
    let uri = uri.trim();
    if uri.is_empty() {
        return None;
    }
    let uri = uri.replace("%%", COVER_SIZE);
    Some(if uri.starts_with("http://") || uri.starts_with("https://") {
        uri
    } else {
        format!("https://{}", uri.trim_start_matches('/'))
    })
}

/// Maps a Yandex track object to a [`RemoteTrack`]. `None` when it has no id or title.
fn map_track(t: &Value) -> Option<RemoteTrack> {
    let remote_id = id_string(&t["id"])?;
    let mut title = t["title"].as_str()?.trim().to_string();
    if let Some(version) = t["version"].as_str().map(str::trim).filter(|v| !v.is_empty()) {
        title = format!("{title} ({version})");
    }
    let artist = t["artists"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let album = &t["albums"][0];
    let cover_url = [&t["coverUri"], &album["coverUri"], &t["ogImage"]]
        .into_iter()
        .find_map(|v| v.as_str().and_then(cover_url));
    Some(RemoteTrack {
        source: "yandex".into(),
        remote_id,
        title,
        artist,
        album: album["title"].as_str().unwrap_or_default().to_string(),
        duration_ms: t["durationMs"].as_u64().unwrap_or(0),
        cover_url,
        isrc: t["isrc"].as_str().map(str::to_string),
        year: album["year"].as_u64().and_then(|y| u32::try_from(y).ok()).filter(|y| *y > 0),
        genre: album["genre"].as_str().filter(|g| !g.is_empty()).map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_token_from_redirect() {
        let url = "https://music.yandex.ru/#access_token=y0_AgAAAAB&token_type=bearer&expires_in=31536000";
        assert_eq!(token_from_redirect(url).as_deref(), Some("y0_AgAAAAB"));
        let url = "https://oauth.yandex.ru/verification_code#token_type=bearer&access_token=abc-123";
        assert_eq!(token_from_redirect(url).as_deref(), Some("abc-123"));
        assert_eq!(token_from_redirect("https://music.yandex.ru/?access_token=q").as_deref(), Some("q"));
        assert_eq!(token_from_redirect("https://music.yandex.ru/#error=access_denied"), None);
        assert_eq!(token_from_redirect("https://music.yandex.ru/#access_token="), None);
        assert_eq!(token_from_redirect("https://music.yandex.ru/"), None);
    }

    // Expected values below were computed independently with .NET's MD5 / HMACSHA256.
    #[test]
    fn mp3_sign_matches_reference() {
        assert_eq!(mp3_sign("/abc/def", "secret123"), "8ba8b4c6cee41a3b4639b266865754d0");
        let xml = "<download-info><host>h.example</host><path>/abc/def</path><ts>1a2b</ts>\
                   <region>-1</region><s>secret123</s></download-info>";
        assert_eq!(
            mp3_url_from_xml(xml).as_deref(),
            Some("https://h.example/get-mp3/8ba8b4c6cee41a3b4639b266865754d0/1a2b/abc/def")
        );
        assert_eq!(mp3_url_from_xml("<download-info></download-info>"), None);
    }

    #[test]
    fn lyrics_sign_matches_reference() {
        assert_eq!(lyrics_sign(330765, 1_700_000_000), "BYmnvXdrAYns6KKqtOrfv2JRpT/YgJg82OlnJBMhZEg=");
    }

    #[test]
    fn file_info_sign_matches_reference() {
        // Full base64 is "mmhqiaKg1Gy4TINx+82nAOj7f2SnVsi5RyCjXbL7b5w="; the API wants it without the last char.
        assert_eq!(
            file_info_sign(1_700_000_000, "330765", "nq", &["mp3"], "raw"),
            "mmhqiaKg1Gy4TINx+82nAOj7f2SnVsi5RyCjXbL7b5w"
        );
    }

    #[test]
    fn builds_cover_url() {
        assert_eq!(
            cover_url("avatars.yandex.net/get-music-content/10703962/da0bc4c9.a.3389005-2/%%").as_deref(),
            Some("https://avatars.yandex.net/get-music-content/10703962/da0bc4c9.a.3389005-2/1000x1000")
        );
        assert_eq!(cover_url("https://x.net/a/%%").as_deref(), Some("https://x.net/a/1000x1000"));
        assert_eq!(cover_url(""), None);
    }

    #[test]
    fn picks_best_download_info() {
        let infos = json!([
            {"codec": "mp3", "preview": true, "bitrateInKbps": 320, "downloadInfoUrl": "p320"},
            {"codec": "aac", "preview": false, "bitrateInKbps": 256, "downloadInfoUrl": "a256"},
            {"codec": "mp3", "preview": false, "bitrateInKbps": 192, "downloadInfoUrl": "f192"},
            {"codec": "mp3", "preview": false, "bitrateInKbps": 320, "downloadInfoUrl": "f320"},
        ]);
        assert_eq!(pick_download_info(&infos).unwrap()["downloadInfoUrl"], "f320");
        let previews = json!([{"codec": "mp3", "preview": true, "bitrateInKbps": 128, "downloadInfoUrl": "p"}]);
        assert_eq!(pick_download_info(&previews).unwrap()["downloadInfoUrl"], "p");
        assert!(pick_download_info(&json!([])).is_none());
    }

    #[test]
    fn misc_helpers() {
        assert_eq!(track_number("330765"), Some(330765));
        assert_eq!(track_number("330765:3389005"), Some(330765));
        assert_eq!(track_number("f1e2-uuid"), None);
        assert_eq!(form_encode("1:2,3"), "1%3A2%2C3");
        assert_eq!(id_string(&json!(5)).as_deref(), Some("5"));
        assert_eq!(id_string(&json!("5")).as_deref(), Some("5"));
        assert_eq!(id_string(&json!(null)), None);
    }

    #[test]
    fn maps_track_json() {
        let t = json!({
            "id": 330765,
            "title": "Creep",
            "version": "Explicit",
            "durationMs": 237370,
            "artists": [{"id": 36825, "name": "Radiohead"}, {"id": 1, "name": "Guest"}],
            "albums": [{
                "id": 3389005, "title": "Pablo Honey", "year": 1993, "genre": "alternative",
                "coverUri": "avatars.yandex.net/get-music-content/1/album/%%"
            }],
            "coverUri": "avatars.yandex.net/get-music-content/10703962/da0bc4c9.a.3389005-2/%%"
        });
        assert_eq!(
            map_track(&t).unwrap(),
            RemoteTrack {
                source: "yandex".into(),
                remote_id: "330765".into(),
                title: "Creep (Explicit)".into(),
                artist: "Radiohead, Guest".into(),
                album: "Pablo Honey".into(),
                duration_ms: 237370,
                cover_url: Some(
                    "https://avatars.yandex.net/get-music-content/10703962/da0bc4c9.a.3389005-2/1000x1000".into()
                ),
                isrc: None,
                year: Some(1993),
                genre: Some("alternative".into()),
            }
        );

        // String id, no album, cover only on the album-less track's ogImage.
        let ugc = json!({"id": "abc-def", "title": "Demo", "artists": [], "ogImage": "avatars.yandex.net/x/%%"});
        let m = map_track(&ugc).unwrap();
        assert_eq!(m.remote_id, "abc-def");
        assert_eq!(m.album, "");
        assert_eq!(m.year, None);
        assert_eq!(m.cover_url.as_deref(), Some("https://avatars.yandex.net/x/1000x1000"));

        assert!(map_track(&json!({"title": "no id"})).is_none());
    }

    #[test]
    fn provider_basics() {
        let y = Yandex::new(Some("  ".into()));
        assert!(!y.authorized());
        assert_eq!(y.id(), "yandex");
        assert!(Yandex::new(Some("tok".into())).authorized());
    }

    /// Hits the real API anonymously: search and the (preview) stream flow.
    #[test]
    #[ignore = "network"]
    fn live_anonymous_search_and_stream() {
        let y = Yandex::new(None);
        let tracks = y.search("radiohead creep", 25).expect("search");
        assert!(tracks.len() > 20, "expected paging past 20 results, got {}", tracks.len());
        assert!(tracks.len() <= 25);
        for t in &tracks {
            assert_eq!(t.source, "yandex");
            assert!(!t.remote_id.is_empty() && !t.title.is_empty());
        }
        let first = &tracks[0];
        eprintln!("{first:#?}");
        assert!(first.artist.contains("Radiohead"));
        assert!(first.duration_ms > 0);
        assert!(first.cover_url.as_deref().is_some_and(|c| c.starts_with("https://") && c.ends_with(COVER_SIZE)));

        let StreamSource::Http { url, mime, .. } = y.stream(&first.remote_id).expect("stream") else {
            panic!("expected Http stream");
        };
        assert_eq!(mime.as_deref(), Some("audio/mpeg"));
        let resp = y.http().get(&url).header("Range", "bytes=0-1023").send().expect("fetch audio");
        assert!(resp.status().is_success(), "audio fetch status {}", resp.status());
        assert!(!resp.bytes().unwrap().is_empty());

        assert!(y.lyrics(&first.remote_id).is_err(), "lyrics need a token");
    }
}
