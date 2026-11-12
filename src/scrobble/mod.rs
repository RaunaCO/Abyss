//! Last.fm scrobbling: request signing, API client, local queue and worker.
//!
//! Last.fm does not use OAuth 1.0a. Calls are signed with its own scheme
//! (`api_sig`, an MD5 of the sorted parameters plus the application secret) and
//! a user authorises Abyss on last.fm, which returns a permanent session key.
//! The key is stored with DPAPI like the TIDAL tokens (see `config.rs`).

pub mod queue;
pub mod tracker;
pub mod worker;

use std::fmt;
use std::time::Duration;

use md5::{Digest, Md5};
use serde_json::Value;

pub use worker::Scrobbler;

/// Application credentials registered for Abyss at last.fm/api. They identify
/// the application, not a user, so they are not confidential (any open source
/// client ships them). Set at build time with `ABYSS_LASTFM_API_KEY` and
/// `ABYSS_LASTFM_API_SECRET`, or replace the empty defaults here.
pub const API_KEY: &str = match option_env!("ABYSS_LASTFM_API_KEY") {
    Some(v) => v,
    None => "",
};
const API_SECRET: &str = match option_env!("ABYSS_LASTFM_API_SECRET") {
    Some(v) => v,
    None => "",
};

const ENDPOINT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_PAGE: &str = "https://www.last.fm/api/auth/";

/// True when this build carries Last.fm application credentials.
pub fn configured() -> bool {
    !API_KEY.is_empty() && !API_SECRET.is_empty()
}

/// What Last.fm needs to know about a track.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Meta {
    pub artist: String,
    pub track: String,
    pub album: String,
    /// Seconds, 0 when unknown.
    pub duration: u32,
}

/// One listen to send: `id` is stable so a retry never duplicates it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// When the listen started (Unix seconds, UTC).
    pub ts: u64,
    pub meta: Meta,
}

/// Why a call failed, grouped by what the caller should do about it.
#[derive(Clone, Debug, PartialEq)]
pub enum ApiError {
    /// The session key is no longer valid: connect again.
    InvalidSession,
    /// The user has not allowed Abyss on last.fm yet.
    Pending,
    /// Network trouble or a temporary Last.fm failure: try again later.
    Retry(String),
    /// The application key is invalid or suspended: stop until reconfigured.
    Fatal(String),
    /// This request will never succeed.
    Permanent(i64, String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSession => f.write_str("session expired"),
            Self::Pending => f.write_str("not authorised yet"),
            Self::Retry(m) | Self::Fatal(m) => f.write_str(m),
            Self::Permanent(code, m) => write!(f, "{m} ({code})"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Outcome of a `track.scrobble` call.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sent {
    pub accepted: u32,
    pub ignored: u32,
}

/// The calls the worker makes. A trait so the queue logic is tested without a network.
pub trait Api {
    fn auth_token(&self) -> Result<String, ApiError>;
    fn auth_url(&self, token: &str) -> String;
    /// Returns the user name and the session key.
    fn session(&self, token: &str) -> Result<(String, String), ApiError>;
    fn now_playing(&self, session: &str, meta: &Meta) -> Result<(), ApiError>;
    fn scrobble(&self, session: &str, items: &[Item]) -> Result<Sent, ApiError>;
}

/// `api_sig`: parameter names and values sorted by name and joined, then the
/// secret, hashed with MD5 (lowercase hex). `format` and `callback` are not signed.
pub fn sign(params: &[(String, String)], secret: &str) -> String {
    let mut signed: Vec<&(String, String)> = params.iter().filter(|(k, _)| k != "format" && k != "callback").collect();
    signed.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Md5::new();
    for (key, value) in signed {
        hasher.update(key.as_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update(secret.as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Turn an HTTP status and body into the JSON result or a classified error.
/// Last.fm reports failures as `{"error": code, "message": text}`, sometimes
/// with an HTTP error status, so the body is read first.
pub fn parse_response(status: u16, body: &str) -> Result<Value, ApiError> {
    let value: Option<Value> = serde_json::from_str(body).ok();
    if let Some(code) = value.as_ref().and_then(|v| v.get("error")).and_then(Value::as_i64) {
        let message = value.as_ref().and_then(|v| v.get("message")).and_then(Value::as_str).unwrap_or("Last.fm error").to_string();
        return Err(match code {
            9 => ApiError::InvalidSession,
            14 => ApiError::Pending,
            10 | 26 => ApiError::Fatal(message),
            8 | 11 | 16 | 29 => ApiError::Retry(message),
            _ => ApiError::Permanent(code, message),
        });
    }
    if status >= 500 || status == 429 {
        return Err(ApiError::Retry(format!("HTTP {status}")));
    }
    match value {
        Some(value) if status < 400 => Ok(value),
        _ if status >= 400 => Err(ApiError::Permanent(i64::from(status), format!("HTTP {status}"))),
        _ => Err(ApiError::Retry("unreadable response".into())),
    }
}

fn parse_session(value: &Value) -> Result<(String, String), ApiError> {
    let session = value.get("session");
    let name = session.and_then(|s| s.get("name")).and_then(Value::as_str);
    let key = session.and_then(|s| s.get("key")).and_then(Value::as_str);
    match (name, key) {
        (Some(name), Some(key)) if !key.is_empty() => Ok((name.to_string(), key.to_string())),
        _ => Err(ApiError::Permanent(0, "no session in the response".into())),
    }
}

fn parse_sent(value: &Value) -> Sent {
    let attr = value.get("scrobbles").and_then(|s| s.get("@attr"));
    let number = |name: &str| {
        attr.and_then(|a| a.get(name)).and_then(|n| n.as_u64().or_else(|| n.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0) as u32
    };
    Sent { accepted: number("accepted"), ignored: number("ignored") }
}

/// Parameters of a `track.scrobble` call for up to 50 items (`artist[0]`, ...).
fn scrobble_params(items: &[Item]) -> Vec<(String, String)> {
    let mut params = Vec::new();
    for (i, item) in items.iter().enumerate() {
        params.push((format!("artist[{i}]"), item.meta.artist.clone()));
        params.push((format!("track[{i}]"), item.meta.track.clone()));
        params.push((format!("timestamp[{i}]"), item.ts.to_string()));
        if !item.meta.album.is_empty() {
            params.push((format!("album[{i}]"), item.meta.album.clone()));
        }
        if item.meta.duration > 0 {
            params.push((format!("duration[{i}]"), item.meta.duration.to_string()));
        }
    }
    params
}

/// The real client. Blocking: call it from the worker thread.
pub struct Client {
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("Abyss/", env!("CARGO_PKG_VERSION"), " (https://github.com/RaunaCO/Abyss)"))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self { http }
    }

    fn call(&self, post: bool, method: &str, mut params: Vec<(String, String)>) -> Result<Value, ApiError> {
        params.push(("method".into(), method.into()));
        params.push(("api_key".into(), API_KEY.into()));
        let signature = sign(&params, API_SECRET);
        params.push(("api_sig".into(), signature));
        params.push(("format".into(), "json".into()));
        let request = if post { self.http.post(ENDPOINT).form(&params) } else { self.http.get(ENDPOINT).query(&params) };
        let response = request.send().map_err(|e| ApiError::Retry(e.without_url().to_string()))?;
        let status = response.status().as_u16();
        let body = response.text().map_err(|e| ApiError::Retry(e.without_url().to_string()))?;
        parse_response(status, &body)
    }
}

impl Api for Client {
    fn auth_token(&self) -> Result<String, ApiError> {
        let value = self.call(false, "auth.getToken", Vec::new())?;
        value
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ApiError::Permanent(0, "no token in the response".into()))
    }

    fn auth_url(&self, token: &str) -> String {
        format!("{AUTH_PAGE}?api_key={API_KEY}&token={token}")
    }

    fn session(&self, token: &str) -> Result<(String, String), ApiError> {
        parse_session(&self.call(false, "auth.getSession", vec![("token".into(), token.into())])?)
    }

    fn now_playing(&self, session: &str, meta: &Meta) -> Result<(), ApiError> {
        let mut params =
            vec![("artist".to_string(), meta.artist.clone()), ("track".into(), meta.track.clone()), ("sk".into(), session.into())];
        if !meta.album.is_empty() {
            params.push(("album".into(), meta.album.clone()));
        }
        if meta.duration > 0 {
            params.push(("duration".into(), meta.duration.to_string()));
        }
        self.call(true, "track.updateNowPlaying", params).map(|_| ())
    }

    fn scrobble(&self, session: &str, items: &[Item]) -> Result<Sent, ApiError> {
        let mut params = scrobble_params(items);
        params.push(("sk".into(), session.into()));
        self.call(true, "track.scrobble", params).map(|value| parse_sent(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn signature_sorts_by_name_and_appends_the_secret() {
        // md5("api_keyxxxxxxxxmethodauth.getSessiontokenyyyyyyyymysecret"), computed independently.
        let params = p(&[("token", "yyyyyyyy"), ("method", "auth.getSession"), ("api_key", "xxxxxxxx")]);
        assert_eq!(sign(&params, "mysecret"), "3e356846fa994eec8e629f135724085b");
    }

    #[test]
    fn signature_signs_raw_unicode_values_and_indexed_names() {
        let params = p(&[
            ("track[0]", "Svefn-g-englar"),
            ("sk", "S"),
            ("method", "track.scrobble"),
            ("artist[0]", "Sigur Rós & Co"),
            ("timestamp[0]", "1700000000"),
            ("api_key", "k"),
        ]);
        assert_eq!(sign(&params, "sec"), "bba40e1de5e752286f5cbd6e1fe09702");
    }

    #[test]
    fn format_and_callback_are_not_signed() {
        let base = p(&[("a", "1")]);
        let mut extra = base.clone();
        extra.push(("format".into(), "json".into()));
        extra.push(("callback".into(), "cb".into()));
        assert_eq!(sign(&base, "s"), sign(&extra, "s"));
    }

    #[test]
    fn error_codes_map_to_actions() {
        let err = |code: i64| parse_response(200, &format!(r#"{{"error":{code},"message":"m"}}"#)).unwrap_err();
        assert_eq!(err(9), ApiError::InvalidSession);
        assert_eq!(err(14), ApiError::Pending);
        assert_eq!(err(10), ApiError::Fatal("m".into()));
        assert_eq!(err(26), ApiError::Fatal("m".into()));
        for code in [8, 11, 16, 29] {
            assert_eq!(err(code), ApiError::Retry("m".into()), "code {code}");
        }
        assert_eq!(err(6), ApiError::Permanent(6, "m".into()));
        // Last.fm sometimes pairs an error body with an HTTP error status.
        assert_eq!(parse_response(403, r#"{"error":9,"message":"Invalid session key"}"#).unwrap_err(), ApiError::InvalidSession);
    }

    #[test]
    fn http_failures_without_a_lastfm_error() {
        assert!(matches!(parse_response(503, "<html>"), Err(ApiError::Retry(_))));
        assert!(matches!(parse_response(429, ""), Err(ApiError::Retry(_))));
        assert!(matches!(parse_response(404, "nope"), Err(ApiError::Permanent(404, _))));
        assert!(matches!(parse_response(200, "not json"), Err(ApiError::Retry(_))));
    }

    #[test]
    fn parses_token_and_session_responses() {
        let token = parse_response(200, r#"{"token":"abc123"}"#).unwrap();
        assert_eq!(token.get("token").and_then(Value::as_str), Some("abc123"));
        let session = parse_response(200, r#"{"session":{"name":"rauna","key":"sk1","subscriber":0}}"#).unwrap();
        assert_eq!(parse_session(&session).unwrap(), ("rauna".to_string(), "sk1".to_string()));
        let empty = parse_response(200, r#"{"session":{"name":"rauna","key":""}}"#).unwrap();
        assert!(parse_session(&empty).is_err());
    }

    #[test]
    fn parses_scrobble_counts_as_numbers_or_strings() {
        let a = parse_response(200, r#"{"scrobbles":{"@attr":{"accepted":2,"ignored":1},"scrobble":[]}}"#).unwrap();
        assert_eq!(parse_sent(&a), Sent { accepted: 2, ignored: 1 });
        let b = parse_response(200, r#"{"scrobbles":{"@attr":{"accepted":"3","ignored":"0"}}}"#).unwrap();
        assert_eq!(parse_sent(&b), Sent { accepted: 3, ignored: 0 });
    }

    #[test]
    fn batch_parameters_are_indexed_and_skip_empty_fields() {
        let meta = |album: &str, duration| Meta { artist: "A".into(), track: "T".into(), album: album.into(), duration };
        let items = [Item { id: "1".into(), ts: 10, meta: meta("Al", 200) }, Item { id: "2".into(), ts: 20, meta: meta("", 0) }];
        let params = scrobble_params(&items);
        let names: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["artist[0]", "track[0]", "timestamp[0]", "album[0]", "duration[0]", "artist[1]", "track[1]", "timestamp[1]"]);
    }
}
