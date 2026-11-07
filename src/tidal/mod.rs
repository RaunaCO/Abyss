//! TIDAL API client (blocking; call it from background threads).

pub mod dash;
pub mod lyrics;
pub mod models;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use parking_lot::Mutex;
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;

use crate::config::{Quality, Settings};
pub use models::*;

const AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2";
const API_URL: &str = "https://api.tidal.com/v1";
const SCOPE: &str = "r_usr w_usr w_sub";

/// TIDAL's Android client used with the web (PKCE) login. Sessions from it get
/// every track in FLAC: 16-bit lossless and up to 24-bit/192 kHz hi-res.
pub const PKCE_CLIENT_ID: &str = "6BDSRdpK9hqEBTgU";
pub const PKCE_CLIENT_SECRET: &str = "xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=";
const PKCE_REDIRECT: &str = "https://tidal.com/android/login/auth";

#[derive(Debug, Clone)]
pub struct PkceLogin {
    pub url: String,
    verifier: String,
    unique_key: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub enum PollResult {
    Pending,
    Done,
}

#[derive(Clone)]
pub struct Tidal {
    http: Client,
    pub settings: Arc<Mutex<Settings>>,
    /// Held while refreshing the session so parallel requests that all find
    /// the token expired refresh it once, not once each.
    refresh_lock: Arc<Mutex<()>>,
}

impl Tidal {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        let http = Client::builder()
            .user_agent(concat!("TIDE/", env!("CARGO_PKG_VERSION"), " (https://github.com/RaunaCO/TIDE)"))
            .gzip(true)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(8)
            .build()
            .expect("http client");
        Self { http, settings, refresh_lock: Arc::new(Mutex::new(())) }
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    // ------------------------------------------------------------- login

    pub fn start_device_login(&self) -> Result<DeviceCode> {
        let client_id = crate::config::DEVICE_CLIENT_ID.to_string();
        let v: Value = self
            .http
            .post(format!("{AUTH_URL}/device_authorization"))
            .form(&[("client_id", client_id.as_str()), ("scope", SCOPE)])
            .send()?
            .error_for_status()?
            .json()?;
        let uri = v
            .get("verificationUriComplete")
            .or_else(|| v.get("verificationUri"))
            .and_then(Value::as_str)
            .unwrap_or("link.tidal.com")
            .to_string();
        Ok(DeviceCode {
            device_code: v["deviceCode"].as_str().unwrap_or_default().into(),
            user_code: v["userCode"].as_str().unwrap_or_default().into(),
            verification_uri: if uri.starts_with("http") { uri } else { format!("https://{uri}") },
            expires_in: v["expiresIn"].as_u64().unwrap_or(300),
            interval: v["interval"].as_u64().unwrap_or(2).max(1),
        })
    }

    /// Web login: open `url`, the user signs in and pastes back the address
    /// TIDAL redirects to (it carries the authorization code).
    pub fn start_pkce_login(&self) -> PkceLogin {
        use rand::RngCore;
        use sha2::Digest;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let verifier = b64.encode(raw);
        let challenge = b64.encode(sha2::Sha256::digest(verifier.as_bytes()));
        let mut key = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut key);
        let unique_key: String = key.iter().map(|b| format!("{b:02x}")).collect();
        let url = reqwest::Url::parse_with_params(
            "https://login.tidal.com/authorize",
            &[
                ("response_type", "code"),
                ("redirect_uri", PKCE_REDIRECT),
                ("client_id", PKCE_CLIENT_ID),
                ("lang", "es"),
                ("appMode", "android"),
                ("client_unique_key", unique_key.as_str()),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("restrict_signup", "true"),
            ],
        )
        .map(|u| u.to_string())
        .unwrap_or_default();
        PkceLogin { url, verifier, unique_key }
    }

    pub fn finish_pkce_login(&self, login: &PkceLogin, pasted: &str) -> Result<()> {
        let pasted = pasted.trim();
        let code = reqwest::Url::parse(pasted)
            .ok()
            .and_then(|u| u.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned()))
            .or_else(|| (!pasted.is_empty() && !pasted.contains(' ') && !pasted.contains('/')).then(|| pasted.to_string()))
            .ok_or_else(|| anyhow!("no encuentro el código en esa dirección; copia la URL completa de la barra del navegador"))?;
        let resp = self
            .http
            .post(format!("{AUTH_URL}/token"))
            .form(&[
                ("code", code.as_str()),
                ("client_id", PKCE_CLIENT_ID),
                ("grant_type", "authorization_code"),
                ("redirect_uri", PKCE_REDIRECT),
                ("scope", SCOPE),
                ("code_verifier", login.verifier.as_str()),
                ("client_unique_key", login.unique_key.as_str()),
            ])
            .send()?;
        let status = resp.status();
        let v: Value = resp.json().unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = v.get("error_description").or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("");
            bail!("TIDAL rechazó el inicio de sesión ({status}) {msg}");
        }
        {
            let mut s = self.settings.lock();
            s.client_id = PKCE_CLIENT_ID.into();
            s.client_secret = PKCE_CLIENT_SECRET.into();
        }
        self.store_tokens(&v);
        self.load_session()?;
        Ok(())
    }

    /// Whether the session gets FLAC for every track (web login).
    pub fn full_lossless(&self) -> bool {
        self.settings.lock().client_id == PKCE_CLIENT_ID
    }

    pub fn poll_device_login(&self, code: &DeviceCode) -> Result<PollResult> {
        let (id, secret) = (crate::config::DEVICE_CLIENT_ID, crate::config::DEVICE_CLIENT_SECRET);
        let resp = self
            .http
            .post(format!("{AUTH_URL}/token"))
            .form(&[
                ("client_id", id),
                ("client_secret", secret),
                ("device_code", code.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("scope", SCOPE),
            ])
            .send()?;
        let status = resp.status();
        let v: Value = resp.json().unwrap_or(Value::Null);
        if status.is_success() {
            {
                let mut s = self.settings.lock();
                s.client_id = id.into();
                s.client_secret = secret.into();
            }
            self.store_tokens(&v);
            self.load_session()?;
            return Ok(PollResult::Done);
        }
        match v.get("error").and_then(Value::as_str) {
            Some("authorization_pending") | Some("slow_down") => Ok(PollResult::Pending),
            Some(e) => bail!("login failed: {e}"),
            None => bail!("login failed ({status})"),
        }
    }

    fn store_tokens(&self, v: &Value) {
        let mut s = self.settings.lock();
        if let Some(t) = v.get("access_token").and_then(Value::as_str) {
            s.access_token = Some(t.into());
        }
        if let Some(t) = v.get("refresh_token").and_then(Value::as_str) {
            s.refresh_token = Some(t.into());
        }
        s.expires_at = now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
        if let Some(user) = v.get("user") {
            if let Some(id) = user.get("userId").and_then(Value::as_u64) {
                s.user_id = Some(id);
            }
            if let Some(cc) = user.get("countryCode").and_then(Value::as_str) {
                s.country_code = cc.into();
            }
            if let Some(name) = user.get("username").and_then(Value::as_str) {
                s.username = Some(name.into());
            }
        }
        s.save();
    }

    fn refresh(&self) -> Result<()> {
        let (id, secret, refresh) = {
            let s = self.settings.lock();
            (s.client_id.clone(), s.client_secret.clone(), s.refresh_token.clone())
        };
        let refresh = refresh.ok_or_else(|| anyhow!("not signed in"))?;
        let v: Value = self
            .http
            .post(format!("{AUTH_URL}/token"))
            .form(&[
                ("client_id", id.as_str()),
                ("client_secret", secret.as_str()),
                ("refresh_token", refresh.as_str()),
                ("grant_type", "refresh_token"),
                ("scope", SCOPE),
            ])
            .send()?
            .error_for_status()
            .context("refreshing session")?
            .json()?;
        self.store_tokens(&v);
        Ok(())
    }

    fn token(&self) -> Result<String> {
        let expired = |s: &Settings| s.access_token.is_none() || s.expires_at <= now() + 60;
        if expired(&self.settings.lock()) {
            let _g = self.refresh_lock.lock();
            // Another thread may have refreshed while we waited.
            if expired(&self.settings.lock()) {
                self.refresh()?;
            }
        }
        self.settings.lock().access_token.clone().ok_or_else(|| anyhow!("not signed in"))
    }

    /// The server rejected `used`: refresh, unless another thread already did.
    fn refresh_rejected(&self, used: &str) -> Result<()> {
        let _g = self.refresh_lock.lock();
        if self.settings.lock().access_token.as_deref() == Some(used) {
            self.refresh()?;
        }
        Ok(())
    }

    /// Fill user id, country and name from the current session.
    pub fn load_session(&self) -> Result<()> {
        let v = self.get_json(&format!("{API_URL}/sessions"), &[])?;
        {
            let mut s = self.settings.lock();
            if let Some(id) = v.get("userId").and_then(Value::as_u64) {
                s.user_id = Some(id);
            }
            if let Some(cc) = v.get("countryCode").and_then(Value::as_str) {
                s.country_code = cc.into();
            }
        }
        if let Ok(uid) = self.user_id() {
            if let Ok(u) = self.get(&format!("/users/{uid}"), &[]) {
                let mut s = self.settings.lock();
                let first = u.get("firstName").and_then(Value::as_str).unwrap_or("");
                let name =
                    if first.is_empty() { u.get("username").and_then(Value::as_str).unwrap_or("").to_string() } else { first.to_string() };
                if !name.is_empty() {
                    s.username = Some(name);
                }
                s.user_picture = u.get("picture").and_then(Value::as_str).map(str::to_string);
            }
        }
        self.settings.lock().save();
        Ok(())
    }

    pub fn sign_out(&self) {
        let mut s = self.settings.lock();
        s.access_token = None;
        s.refresh_token = None;
        s.user_id = None;
        s.username = None;
        s.user_picture = None;
        s.save();
    }

    fn user_id(&self) -> Result<u64> {
        self.settings.lock().user_id.ok_or_else(|| anyhow!("not signed in"))
    }

    fn country(&self) -> String {
        self.settings.lock().country_code.clone()
    }

    // ------------------------------------------------------------ requests

    fn send(&self, build: impl Fn(&str) -> RequestBuilder) -> Result<Value> {
        self.send_full(build).map(|(v, _)| v)
    }

    /// Like `send`, also returning the response's ETag (needed to edit playlists).
    fn send_full(&self, build: impl Fn(&str) -> RequestBuilder) -> Result<(Value, Option<String>)> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let token = self.token()?;
            let resp = build(&token).send().context("network error")?;
            let status = resp.status();
            if status.as_u16() == 401 && attempt == 1 {
                // Token revoked or expired early: refresh once and retry.
                self.refresh_rejected(&token)?;
                continue;
            }
            if status.as_u16() == 429 && attempt < 4 {
                std::thread::sleep(Duration::from_millis(600 * attempt));
                continue;
            }
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
            let text = resp.text().unwrap_or_default();
            if !status.is_success() {
                let msg = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("userMessage").or_else(|| v.get("description")).and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_else(|| text.chars().take(200).collect());
                bail!("TIDAL {status}: {msg}");
            }
            if text.trim().is_empty() {
                return Ok((Value::Null, etag));
            }
            return Ok((serde_json::from_str(&text).context("invalid JSON from TIDAL")?, etag));
        }
    }

    fn get_json(&self, url: &str, query: &[(&str, String)]) -> Result<Value> {
        let cc = self.country();
        self.send(|token| {
            self.http
                .get(url)
                .bearer_auth(token)
                .query(&[("countryCode", cc.as_str()), ("locale", "es_ES"), ("deviceType", "BROWSER")])
                .query(query)
        })
    }

    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.get_json(&format!("{API_URL}{path}"), query)
    }

    fn post_form(&self, path: &str, form: &[(&str, String)]) -> Result<Value> {
        let cc = self.country();
        let url = format!("{API_URL}{path}");
        self.send(|token| self.http.post(&url).bearer_auth(token).query(&[("countryCode", cc.as_str())]).form(form))
    }

    fn delete(&self, path: &str) -> Result<Value> {
        let cc = self.country();
        let url = format!("{API_URL}{path}");
        self.send(|token| self.http.delete(&url).bearer_auth(token).query(&[("countryCode", cc.as_str())]))
    }

    /// Fetch every page of a paged list (up to `max` items).
    fn paged(&self, path: &str, extra: &[(&str, String)], max: usize) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        // Some collection endpoints refuse pages larger than 50.
        let page = if path.contains("playlistsAndFavoritePlaylists") { 50usize } else { 100usize };
        loop {
            let mut q: Vec<(&str, String)> = extra.to_vec();
            q.push(("limit", page.to_string()));
            q.push(("offset", out.len().to_string()));
            let v = self.get(path, &q)?;
            let items = v.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
            let total = v.get("totalNumberOfItems").and_then(Value::as_u64).unwrap_or(0) as usize;
            let n = items.len();
            out.extend(items);
            if n == 0 || out.len() >= total || out.len() >= max {
                break;
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------- catalogue

    pub fn search(&self, query: &str) -> Result<SearchResults> {
        let v = self.get(
            "/search",
            &[
                ("query", query.to_string()),
                ("types", "ARTISTS,ALBUMS,TRACKS,PLAYLISTS".into()),
                ("limit", "25".into()),
                ("includeContributors", "true".into()),
            ],
        )?;
        let list = |k: &str| -> Vec<Value> { v.get(k).and_then(|x| x.get("items")).and_then(Value::as_array).cloned().unwrap_or_default() };
        let top = v.get("topHit").and_then(|t| {
            let kind = t.get("type")?.as_str()?;
            let kind = kind.trim_end_matches('S');
            Item::parse_typed(kind, t.get("value")?)
        });
        Ok(SearchResults {
            top,
            tracks: list("tracks").iter().filter_map(Track::parse).collect(),
            albums: list("albums").iter().filter_map(Album::parse).collect(),
            artists: list("artists").iter().filter_map(Artist::parse).collect(),
            playlists: list("playlists").iter().filter_map(Playlist::parse).collect(),
        })
    }

    pub fn album(&self, id: u64) -> Result<(Album, Vec<Track>)> {
        let album = Album::parse(&self.get(&format!("/albums/{id}"), &[])?).ok_or_else(|| anyhow!("album not found"))?;
        let items = self.paged(&format!("/albums/{id}/items"), &[], 1000)?;
        let tracks = items
            .iter()
            .filter(|i| i.get("type").and_then(Value::as_str) == Some("track"))
            .filter_map(|i| Track::parse(i.get("item")?))
            .collect();
        Ok((album, tracks))
    }

    pub fn playlist(&self, uuid: &str) -> Result<(Playlist, Vec<Track>)> {
        let pl = Playlist::parse(&self.get(&format!("/playlists/{uuid}"), &[])?).ok_or_else(|| anyhow!("playlist not found"))?;
        // `/tracks` (unlike `/items`) carries the date each track was added.
        let items = self.paged(&format!("/playlists/{uuid}/tracks"), &[], 5000)?;
        let tracks = items.iter().filter_map(|i| Track::parse(i).map(|t| t.with_added(i))).collect();
        Ok((pl, tracks))
    }

    pub fn artist(&self, id: u64) -> Result<ArtistPage> {
        let artist = Artist::parse(&self.get(&format!("/artists/{id}"), &[])?).ok_or_else(|| anyhow!("artist not found"))?;
        let top = self
            .get(&format!("/artists/{id}/toptracks"), &[("limit", "10".into())])?
            .get("items")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Track::parse).collect())
            .unwrap_or_default();
        let albums = |filter: Option<&str>| -> Vec<Album> {
            let mut q = vec![("limit", "50".to_string())];
            if let Some(f) = filter {
                q.push(("filter", f.to_string()));
            }
            self.get(&format!("/artists/{id}/albums"), &q)
                .ok()
                .and_then(|v| v.get("items").and_then(Value::as_array).cloned())
                .map(|a| a.iter().filter_map(Album::parse).collect())
                .unwrap_or_default()
        };
        let bio = self.get(&format!("/artists/{id}/bio"), &[]).ok().and_then(|v| v.get("text").and_then(Value::as_str).map(clean_bio));
        let similar = self
            .get(&format!("/artists/{id}/similar"), &[("limit", "12".into())])
            .ok()
            .and_then(|v| v.get("items").and_then(Value::as_array).cloned())
            .map(|a| a.iter().filter_map(Artist::parse).collect())
            .unwrap_or_default();
        Ok(ArtistPage {
            artist,
            top_tracks: top,
            albums: albums(None),
            singles: albums(Some("EPSANDSINGLES")),
            appears_on: albums(Some("COMPILATIONS")),
            similar,
            bio,
        })
    }

    pub fn mix(&self, id: &str) -> Result<(Mix, Vec<Track>)> {
        let v = self.get("/pages/mix", &[("mixId", id.to_string())])?;
        let mut mix = Mix { id: id.into(), title: v["title"].as_str().unwrap_or("Mix").into(), subtitle: String::new(), image: None };
        let mut tracks = Vec::new();
        for row in v.get("rows").and_then(Value::as_array).into_iter().flatten() {
            for module in row.get("modules").and_then(Value::as_array).into_iter().flatten() {
                match module.get("type").and_then(Value::as_str) {
                    Some("MIX_HEADER") => {
                        if let Some(m) = module.get("mix").and_then(Mix::parse) {
                            mix = m;
                        }
                    }
                    Some("TRACK_LIST") => {
                        let items =
                            module.get("pagedList").and_then(|p| p.get("items")).and_then(Value::as_array).cloned().unwrap_or_default();
                        tracks.extend(items.iter().filter_map(Track::parse));
                    }
                    _ => {}
                }
            }
        }
        Ok((mix, tracks))
    }

    /// Diagnostics: module types, titles and item shapes of an editorial page.
    pub fn page_modules(&self, name: &str) -> Vec<String> {
        let Ok(v) = self.get(&format!("/pages/{name}"), &[]) else { return vec!["ERR".into()] };
        let mut out = Vec::new();
        for row in v.get("rows").and_then(Value::as_array).into_iter().flatten() {
            for m in row.get("modules").and_then(Value::as_array).into_iter().flatten() {
                let kind = m.get("type").and_then(Value::as_str).unwrap_or("?");
                let title = m.get("title").and_then(Value::as_str).unwrap_or("");
                let items = m
                    .get("pagedList")
                    .and_then(|p| p.get("items"))
                    .or_else(|| m.get("items"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let shape = items
                    .first()
                    .and_then(Value::as_object)
                    .map(|o| o.keys().take(8).cloned().collect::<Vec<_>>().join(","))
                    .unwrap_or_default();
                out.push(format!("{kind:<26} | {title:<28} | {:>3} items | {shape}", items.len()));
                if let Some(first) = items.first() {
                    for k in ["imageId", "apiPath", "type", "artifactId", "icon"] {
                        if let Some(v) = first.get(k).and_then(Value::as_str) {
                            out.push(format!("      first.{k} = {v}"));
                        }
                    }
                }
            }
        }
        out
    }

    /// The editorial home page as titled rows of cards and tracks.
    /// The home feed ("For you", "Staff Picks", "Uploads") the official app shows.
    /// `vibe` is `static`, `editorial` or `uploads`.
    pub fn home_feed(&self, vibe: &str) -> Result<Vec<Row>> {
        let cc = self.country();
        let uid = self.user_id()?;
        let url = format!("https://api.tidal.com/v2/home/feed/{vibe}");
        let v = self.send(|token| {
            self.http
                .get(&url)
                .bearer_auth(token)
                .query(&[
                    ("locale", "es_ES"),
                    ("deviceType", "DESKTOP"),
                    ("platform", "DESKTOP"),
                    ("countryCode", cc.as_str()),
                    ("timeOffset", "+00:00"),
                    ("limit", "30"),
                ])
                // Required by this endpoint (400 without it). It is the version
                // string of TIDAL's own desktop app: see docs/LEGAL.md (R2).
                // No subscription tier is asserted: that header was not needed.
                .header("x-tidal-client-version", "2.47.0")
                .header("x-tidal-authenticated-userid", uid.to_string())
        })?;
        let parse = |x: &Value| -> Option<Item> {
            let kind = x.get("type")?.as_str()?;
            let data = x.get("data")?;
            match kind {
                "ALBUM" => Album::parse(data).map(Item::Album),
                "PLAYLIST" => Playlist::parse(data).map(Item::Playlist),
                "ARTIST" => Artist::parse(data).map(Item::Artist),
                "MIX" => Mix::parse_feed(data).map(Item::Mix),
                "TRACK" => Track::parse(data).map(Item::Track),
                "DEEP_LINK" => {
                    let url = data.get("url")?.as_str()?;
                    let page = url.strip_prefix("tidal://my-collection/")?;
                    Some(Item::Link(Link {
                        title: data.get("title")?.as_str()?.to_string(),
                        path: format!("@{page}"),
                        icon: String::new(),
                    }))
                }
                _ => None,
            }
        };
        let mut rows = Vec::new();
        for module in v.get("items").and_then(Value::as_array).into_iter().flatten() {
            let items: Vec<Item> = module.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(parse).collect();
            if items.is_empty() {
                continue;
            }
            let mut title = module.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            // "Because you listened to" + the album it is about.
            if let Some(about) = module.get("header").and_then(|h| h.get("data")).and_then(|d| d.get("title")).and_then(Value::as_str) {
                title = format!("{title} {about}");
            }
            let kind =
                if module.get("type").and_then(Value::as_str) == Some("SHORTCUT_LIST") { RowKind::Shortcuts } else { RowKind::Cards };
            rows.push(Row { title, items, kind });
        }
        Ok(rows)
    }

    pub fn page(&self, name: &str) -> Result<Vec<Row>> {
        let v = self.get(&format!("/pages/{name}"), &[])?;
        let mut rows = Vec::new();
        for row in v.get("rows").and_then(Value::as_array).into_iter().flatten() {
            for module in row.get("modules").and_then(Value::as_array).into_iter().flatten() {
                let kind = module.get("type").and_then(Value::as_str).unwrap_or("");
                let title = module.get("title").and_then(Value::as_str).unwrap_or("").to_string();
                let items: Vec<Value> =
                    module.get("pagedList").and_then(|p| p.get("items")).and_then(Value::as_array).cloned().unwrap_or_default();
                let promos = || -> Vec<Item> {
                    module
                        .get("items")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|p| {
                            let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                            Item::Promo(Promo {
                                // `header` is the category ("LISTEN: NEW SINGLE"), `shortHeader` the name.
                                label: s("header"),
                                title: s("shortHeader"),
                                subtitle: s("shortSubHeader"),
                                image: p.get("imageId").and_then(Value::as_str).map(str::to_string),
                                kind: s("type"),
                                target: s("artifactId"),
                            })
                        })
                        .collect()
                };
                let links = || -> Vec<Item> {
                    items
                        .iter()
                        .filter_map(|l| {
                            Some(Item::Link(Link {
                                title: l.get("title")?.as_str()?.to_string(),
                                path: l.get("apiPath")?.as_str()?.trim_start_matches("pages/").to_string(),
                                icon: l.get("icon").and_then(Value::as_str).unwrap_or("").to_string(),
                            }))
                        })
                        .collect()
                };
                let (row_kind, parsed): (RowKind, Vec<Item>) = match kind {
                    "PAGE_LINKS_CLOUD" => (RowKind::Chips, links()),
                    "PAGE_LINKS" => (RowKind::Links, links()),
                    "FEATURED_PROMOTIONS" if name == "explore" => (RowKind::Featured, promos()),
                    _ => (
                        RowKind::Cards,
                        match kind {
                            "MIXED_TYPES_LIST" | "HIGHLIGHT_MODULE" => items
                                .iter()
                                .filter_map(|i| {
                                    let i = i.get("item").map(|inner| (i, inner)).unwrap_or((i, i));
                                    Item::parse_typed(i.0.get("type")?.as_str()?, i.1)
                                })
                                .collect(),
                            "FEATURED_PROMOTIONS" | "MULTIPLE_TOP_PROMOTIONS" => {
                                module.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(promotion_item).collect()
                            }
                            other => items.iter().filter_map(|i| Item::parse_typed(other, i)).collect(),
                        },
                    ),
                };
                if !parsed.is_empty() {
                    rows.push(Row { title, items: parsed, kind: row_kind });
                }
            }
        }
        Ok(rows)
    }

    /// More albums by the artist and related artists, for the bottom of an album page.
    pub fn album_extras(&self, artist_id: u64, album_id: u64) -> Result<(Vec<Album>, Vec<Artist>)> {
        let albums = self
            .get(&format!("/artists/{artist_id}/albums"), &[("limit", "30".into())])
            .ok()
            .and_then(|v| v.get("items").and_then(Value::as_array).cloned())
            .map(|a| a.iter().filter_map(Album::parse).filter(|a| a.id != album_id).collect())
            .unwrap_or_default();
        let similar = self
            .get(&format!("/artists/{artist_id}/similar"), &[("limit", "14".into())])
            .ok()
            .and_then(|v| v.get("items").and_then(Value::as_array).cloned())
            .map(|a| a.iter().filter_map(Artist::parse).collect())
            .unwrap_or_default();
        Ok((albums, similar))
    }

    // ------------------------------------------------------------- library

    pub fn favorite_tracks(&self) -> Result<Vec<Track>> {
        let uid = self.user_id()?;
        let items =
            self.paged(&format!("/users/{uid}/favorites/tracks"), &[("order", "DATE".into()), ("orderDirection", "DESC".into())], 10_000)?;
        Ok(items.iter().filter_map(|i| Track::parse(i.get("item")?).map(|t| t.with_added(i))).collect())
    }

    pub fn favorite_albums(&self) -> Result<Vec<Album>> {
        let uid = self.user_id()?;
        let items =
            self.paged(&format!("/users/{uid}/favorites/albums"), &[("order", "DATE".into()), ("orderDirection", "DESC".into())], 5000)?;
        Ok(items.iter().filter_map(|i| Album::parse(i.get("item")?)).collect())
    }

    pub fn favorite_artists(&self) -> Result<Vec<Artist>> {
        let uid = self.user_id()?;
        let items =
            self.paged(&format!("/users/{uid}/favorites/artists"), &[("order", "DATE".into()), ("orderDirection", "DESC".into())], 5000)?;
        Ok(items.iter().filter_map(|i| Artist::parse(i.get("item")?)).collect())
    }

    pub fn my_playlists(&self) -> Result<Vec<Playlist>> {
        let uid = self.user_id()?;
        let items = self.paged(
            &format!("/users/{uid}/playlistsAndFavoritePlaylists"),
            &[("order", "DATE".into()), ("orderDirection", "DESC".into())],
            2000,
        )?;
        Ok(items.iter().filter_map(|i| Playlist::parse(i.get("playlist")?)).collect())
    }

    pub fn my_mixes(&self) -> Result<Vec<Mix>> {
        let rows = self.page("my_collection_my_mixes")?;
        Ok(rows.into_iter().flat_map(|r| r.items).filter_map(|i| if let Item::Mix(m) = i { Some(m) } else { None }).collect())
    }

    /// Ids of favourite tracks and followed artists.
    pub fn favorite_ids(&self) -> Result<(Vec<u64>, Vec<u64>)> {
        let uid = self.user_id()?;
        let v = self.get(&format!("/users/{uid}/favorites/ids"), &[])?;
        let ids = |k: &str| -> Vec<u64> {
            v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str()?.parse().ok()).collect()).unwrap_or_default()
        };
        Ok((ids("TRACK"), ids("ARTIST")))
    }

    pub fn set_favorite_track(&self, id: u64, fav: bool) -> Result<()> {
        let uid = self.user_id()?;
        if fav {
            self.post_form(&format!("/users/{uid}/favorites/tracks"), &[("trackIds", id.to_string())])?;
        } else {
            self.delete(&format!("/users/{uid}/favorites/tracks/{id}"))?;
        }
        Ok(())
    }

    pub fn set_favorite_album(&self, id: u64, fav: bool) -> Result<()> {
        let uid = self.user_id()?;
        if fav {
            self.post_form(&format!("/users/{uid}/favorites/albums"), &[("albumIds", id.to_string())])?;
        } else {
            self.delete(&format!("/users/{uid}/favorites/albums/{id}"))?;
        }
        Ok(())
    }

    // ---------------------------------------------------- playlist editing

    fn playlist_etag(&self, uuid: &str) -> Result<String> {
        let cc = self.country();
        let url = format!("{API_URL}/playlists/{uuid}");
        let (_, etag) = self.send_full(|token| self.http.get(&url).bearer_auth(token).query(&[("countryCode", cc.as_str())]))?;
        etag.ok_or_else(|| anyhow!("TIDAL did not return a playlist version"))
    }

    /// Send a write to a playlist with its current version (If-None-Match).
    fn playlist_write(&self, uuid: &str, method: reqwest::Method, path: &str, form: &[(&str, String)]) -> Result<Value> {
        let etag = self.playlist_etag(uuid)?;
        let cc = self.country();
        let url = format!("{API_URL}{path}");
        self.send(|token| {
            self.http
                .request(method.clone(), &url)
                .bearer_auth(token)
                .header("If-None-Match", etag.as_str())
                .query(&[("countryCode", cc.as_str())])
                .form(form)
        })
    }

    pub fn create_playlist(&self, title: &str, description: &str) -> Result<Playlist> {
        let uid = self.user_id()?;
        let v =
            self.post_form(&format!("/users/{uid}/playlists"), &[("title", title.to_string()), ("description", description.to_string())])?;
        Playlist::parse(&v).ok_or_else(|| anyhow!("TIDAL did not return the new playlist"))
    }

    pub fn edit_playlist(&self, uuid: &str, title: &str, description: &str) -> Result<()> {
        self.playlist_write(
            uuid,
            reqwest::Method::POST,
            &format!("/playlists/{uuid}"),
            &[("title", title.to_string()), ("description", description.to_string())],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, uuid: &str) -> Result<()> {
        self.delete(&format!("/playlists/{uuid}"))?;
        Ok(())
    }

    pub fn add_to_playlist(&self, uuid: &str, track_ids: &[u64]) -> Result<()> {
        let ids = track_ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        self.playlist_write(
            uuid,
            reqwest::Method::POST,
            &format!("/playlists/{uuid}/items"),
            &[("trackIds", ids), ("onArtifactNotFound", "SKIP".into()), ("onDupes", "ADD".into())],
        )?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, uuid: &str, index: usize) -> Result<()> {
        self.playlist_write(uuid, reqwest::Method::DELETE, &format!("/playlists/{uuid}/items/{index}"), &[])?;
        Ok(())
    }

    pub fn move_in_playlist(&self, uuid: &str, from: usize, to: usize) -> Result<()> {
        self.playlist_write(uuid, reqwest::Method::POST, &format!("/playlists/{uuid}/items/{from}"), &[("toIndex", to.to_string())])?;
        Ok(())
    }

    pub fn set_favorite_playlist(&self, uuid: &str, fav: bool) -> Result<()> {
        let uid = self.user_id()?;
        if fav {
            self.post_form(&format!("/users/{uid}/favorites/playlists"), &[("uuids", uuid.to_string())])?;
        } else {
            self.delete(&format!("/users/{uid}/favorites/playlists/{uuid}"))?;
        }
        Ok(())
    }

    pub fn set_favorite_artist(&self, id: u64, fav: bool) -> Result<()> {
        let uid = self.user_id()?;
        if fav {
            self.post_form(&format!("/users/{uid}/favorites/artists"), &[("artistIds", id.to_string())])?;
        } else {
            self.delete(&format!("/users/{uid}/favorites/artists/{id}"))?;
        }
        Ok(())
    }

    /// Credits of a track: (role, names).
    pub fn credits(&self, track_id: u64) -> Result<Vec<(String, Vec<String>)>> {
        let v = self.get(&format!("/tracks/{track_id}/credits"), &[("includeContributors", "true".into())])?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let role = c.get("type")?.as_str()?.to_string();
                let names: Vec<String> =
                    c.get("contributors")?.as_array()?.iter().filter_map(|p| p.get("name")?.as_str().map(str::to_string)).collect();
                (!names.is_empty()).then_some((role, names))
            })
            .collect())
    }

    pub fn lyrics(&self, track_id: u64) -> Result<Lyrics> {
        let v = self.get(&format!("/tracks/{track_id}/lyrics"), &[])?;
        let synced = v.get("subtitles").and_then(Value::as_str).map(parse_lrc).unwrap_or_default();
        Ok(Lyrics {
            roman: Vec::new(),
            text: v.get("lyrics").and_then(Value::as_str).unwrap_or("").to_string(),
            synced,
            provider: v.get("lyricsProvider").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    }

    pub fn track_radio(&self, track_id: u64) -> Result<Vec<Track>> {
        let v = self.get(&format!("/tracks/{track_id}/radio"), &[("limit", "50".into())])?;
        Ok(v.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Track::parse).collect())
    }

    // ------------------------------------------------------------- playback

    /// Resolve the audio stream for a track in the best quality allowed by
    /// `quality`. Hi-res comes as DASH, which the player handles natively.
    pub fn stream(&self, track_id: u64, quality: Quality) -> Result<StreamInfo> {
        let mut last = anyhow!("no stream available");
        for q in quality.api_values() {
            match self.stream_at(track_id, q) {
                Ok(info) => return Ok(info),
                Err(e) => {
                    log::debug!("{q} unavailable for {track_id}: {e:#}");
                    last = e;
                }
            }
        }
        Err(last)
    }

    /// Diagnostics: what each endpoint/quality combination returns.
    pub fn probe(&self, track_id: u64) -> Vec<String> {
        let mut out = Vec::new();
        let cc = self.country();
        match self.get(
            &format!("/tracks/{track_id}/playbackinfo"),
            &[("audioquality", "HI_RES_LOSSLESS".into()), ("playbackmode", "STREAM".into()), ("assetpresentation", "FULL".into())],
        ) {
            Ok(v) => {
                let m = v["manifest"].as_str().unwrap_or("");
                let xml = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(m).unwrap_or_default()).unwrap_or_default();
                // Hide signed URLs/tokens.
                let shown: String = xml.split('"').map(|p| if p.starts_with("http") { "<url>" } else { p }).collect::<Vec<_>>().join("\"");
                out.push(format!("MPD:\n{shown}"));
                match dash::parse(&xml) {
                    Ok(mpd) => out.push(format!(
                        "parsed: {} urls, codec {}, {:?} bit, {:?} Hz",
                        mpd.urls.len(),
                        mpd.codec,
                        mpd.bit_depth,
                        mpd.sample_rate
                    )),
                    Err(e) => out.push(format!("PARSE ERROR: {e:#}")),
                }
            }
            Err(e) => out.push(format!("ERR {e:#}")),
        }
        if let Ok(v) = self.get_json(&format!("{API_URL}/sessions"), &[]) {
            out.push(format!("session client: {}", v.get("client").map(|c| c.to_string()).unwrap_or_default()));
        }
        for ep in ["playbackinfo"] {
            for device in [None] {
                for q in ["HI_RES", "LOSSLESS", "HI_RES_LOSSLESS", "HIGH"] {
                    let url = format!("{API_URL}/tracks/{track_id}/{ep}");
                    let r = self.send(|token| {
                        let mut req = self.http.get(&url).bearer_auth(token);
                        if let Some(d) = device {
                            req = req.header("x-tidal-client-version", d);
                        }
                        req.query(&[
                            ("countryCode", cc.as_str()),
                            ("audioquality", q),
                            ("playbackmode", "STREAM"),
                            ("assetpresentation", "FULL"),
                        ])
                    });
                    let line = match r {
                        Ok(v) => format!(
                            "{} {} {}",
                            v["audioQuality"].as_str().unwrap_or("?"),
                            v["bitDepth"],
                            v["manifestMimeType"].as_str().unwrap_or("?")
                        ),
                        Err(e) => format!("ERR {e:#}"),
                    };
                    out.push(format!("{ep:<26} clientver={:<10} ask={q:<16} -> {line}", device.unwrap_or("-")));
                }
            }
        }
        out
    }

    fn stream_at(&self, track_id: u64, quality: &str) -> Result<StreamInfo> {
        let v = self.get(
            &format!("/tracks/{track_id}/playbackinfo"),
            &[("audioquality", quality.into()), ("playbackmode", "STREAM".into()), ("assetpresentation", "FULL".into())],
        )?;
        let mime = v.get("manifestMimeType").and_then(Value::as_str).unwrap_or("");
        let manifest = v.get("manifest").and_then(Value::as_str).ok_or_else(|| anyhow!("no manifest"))?;
        let manifest = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(manifest)?)?;
        let audio_quality = v.get("audioQuality").and_then(Value::as_str).unwrap_or(quality).to_string();
        let bit_depth = v.get("bitDepth").and_then(Value::as_u64).map(|x| x as u32);
        let sample_rate = v.get("sampleRate").and_then(Value::as_u64).map(|x| x as u32);
        let replay_gain = v.get("trackReplayGain").and_then(Value::as_f64).map(|x| x as f32);
        let peak = v.get("trackPeakAmplitude").and_then(Value::as_f64).map(|x| x as f32);

        if mime.contains("dash") || manifest.trim_start().starts_with('<') {
            let mpd = dash::parse(&manifest)?;
            return Ok(StreamInfo {
                source: StreamSource::Segments(mpd.urls),
                seg_secs: mpd.seg_secs,
                codec: mpd.codec,
                audio_quality,
                bit_depth: bit_depth.or(mpd.bit_depth),
                sample_rate: sample_rate.or(mpd.sample_rate),
                replay_gain,
                peak,
            });
        }
        let m: Value = serde_json::from_str(&manifest).context("parsing BTS manifest")?;
        if let Some(enc) = m.get("encryptionType").and_then(Value::as_str) {
            if enc != "NONE" {
                bail!("encrypted stream ({enc}) is not supported");
            }
        }
        let url = m
            .get("urls")
            .and_then(Value::as_array)
            .and_then(|u| u.first())
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("no URL in manifest"))?
            .to_string();
        if !url.starts_with("https://") {
            bail!("stream URL is not HTTPS");
        }
        let codec = m.get("codecs").and_then(Value::as_str).unwrap_or("flac").to_string();
        Ok(StreamInfo {
            source: StreamSource::Single(url),
            seg_secs: Vec::new(),
            codec: if codec.starts_with("mp4a") { "AAC".into() } else { codec },
            audio_quality,
            bit_depth,
            sample_rate,
            replay_gain,
            peak,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ArtistPage {
    pub artist: Artist,
    pub top_tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub singles: Vec<Album>,
    pub appears_on: Vec<Album>,
    pub similar: Vec<Artist>,
    pub bio: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub text: String,
    /// (seconds, line)
    pub synced: Vec<(f64, String)>,
    pub provider: String,
    /// Latin pronunciation of each synced line (romaji, pinyin, Korean romanization);
    /// empty when the lyrics need none.
    pub roman: Vec<Option<String>>,
}

fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix('[')?;
            let (stamp, line) = rest.split_once(']')?;
            let (m, sec) = stamp.split_once(':')?;
            let t = m.parse::<f64>().ok()? * 60.0 + sec.parse::<f64>().ok()?;
            Some((t, line.trim().to_string()))
        })
        .collect()
}

/// Strip TIDAL's `[wimpLink ...]text[/wimpLink]` markup from bios.
fn clean_bio(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '[' => in_tag = true,
            ']' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("<br/>", "\n").replace("<br />", "\n")
}

fn promotion_item(v: &Value) -> Option<Item> {
    let kind = v.get("type")?.as_str()?;
    let id = v.get("artifactId")?.as_str()?;
    let title = v.get("header").and_then(Value::as_str).unwrap_or("").to_string();
    let image = v.get("imageId").and_then(Value::as_str).map(str::to_string);
    match kind {
        "PLAYLIST" => Some(Item::Playlist(Playlist {
            uuid: id.into(),
            title,
            description: v.get("shortSubHeader").and_then(Value::as_str).unwrap_or("").into(),
            image,
            square_image: None,
            number_of_tracks: 0,
            duration: 0,
            creator: "TIDAL".into(),
            creator_id: 0,
            kind: "EDITORIAL".into(),
        })),
        "ALBUM" => Some(Item::Album(Album {
            id: id.parse().ok()?,
            title,
            cover: image,
            artists: vec![ArtistRef { id: 0, name: v.get("shortHeader").and_then(Value::as_str).unwrap_or("").into() }],
            release_date: String::new(),
            number_of_tracks: 0,
            duration: 0,
            kind: "ALBUM".into(),
            explicit: false,
            tags: vec![],
            copyright: String::new(),
        })),
        _ => None,
    }
}
