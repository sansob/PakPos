//! OAuth 2.0 token acquisition: client credentials, resource-owner password,
//! and authorization code (with PKCE) through a local redirect listener.
//!
//! Tokens never go into collection files. They live in a [`TokenStore`]
//! (`tokens.json` in the workspace) keyed by [`OAuth2Config::cache_key`], and
//! [`crate::http::prepare`] reads them from [`VarScope::oauth_tokens`].

use crate::http::HttpSettings;
use crate::model::{Auth, ClientAuthMethod, OAuth2Config, OAuth2Grant};
use crate::vars::VarScope;
use base64::Engine;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("OAuth 2.0: {0} is required")]
    Missing(&'static str),
    #[error("token request failed: {0}")]
    Transport(String),
    #[error("token endpoint returned {status}: {body}")]
    Server { status: u16, body: String },
    #[error("authorization failed: {0}")]
    Denied(String),
    #[error("redirect URI must be http://127.0.0.1:<port>/… or http://localhost:<port>/…, got `{0}`")]
    BadRedirect(String),
    #[error("timed out waiting for the browser login")]
    Timeout,
    #[error("authorization code login needs a browser; get the token in the PakPos app first")]
    NeedsInteractive,
}

/// An access token as stored by PakPos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(default = "bearer")]
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub obtained_at: DateTime<Utc>,
}

fn bearer() -> String {
    "Bearer".into()
}

impl OAuthToken {
    /// Still usable for at least 30 more seconds (tokens without expiry never expire).
    pub fn is_fresh(&self) -> bool {
        self.expires_at.is_none_or(|t| t > Utc::now() + ChronoDuration::seconds(30))
    }

    /// Seconds until expiry, if known.
    pub fn expires_in(&self) -> Option<i64> {
        self.expires_at.map(|t| (t - Utc::now()).num_seconds())
    }
}

/// Tokens by cache key.
pub type TokenStore = HashMap<String, OAuthToken>;

/// Puts every stored access token into the scope so `prepare` can use them.
pub fn load_into_scope(store: &TokenStore, scope: &mut VarScope) {
    for (k, t) in store {
        scope.oauth_tokens.insert(k.clone(), t.access_token.clone());
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    scope: Option<String>,
}

fn parse_token_response(body: &[u8]) -> Result<OAuthToken, OAuthError> {
    // Most servers answer JSON; some (e.g. older GitHub) answer form-encoded.
    let parsed: TokenResponse = match serde_json::from_slice(body) {
        Ok(t) => t,
        Err(_) => {
            let map: HashMap<String, String> = url::form_urlencoded::parse(body).into_owned().collect();
            if let Some(err) = map.get("error") {
                return Err(OAuthError::Denied(err.clone()));
            }
            TokenResponse {
                access_token: map
                    .get("access_token")
                    .cloned()
                    .ok_or(OAuthError::Server { status: 200, body: String::from_utf8_lossy(body).into_owned() })?,
                token_type: map.get("token_type").cloned(),
                refresh_token: map.get("refresh_token").cloned(),
                expires_in: map.get("expires_in").map(|v| serde_json::Value::String(v.clone())),
                scope: map.get("scope").cloned(),
            }
        }
    };
    let expires_in = parsed.expires_in.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    });
    let now = Utc::now();
    Ok(OAuthToken {
        access_token: parsed.access_token,
        token_type: parsed.token_type.unwrap_or_else(bearer),
        refresh_token: parsed.refresh_token,
        expires_at: expires_in.map(|s| now + ChronoDuration::seconds(s)),
        scope: parsed.scope,
        obtained_at: now,
    })
}

/// POSTs a token request with the configured client authentication.
fn token_request(
    cfg: &OAuth2Config,
    mut form: Vec<(&str, String)>,
    settings: &HttpSettings,
) -> Result<OAuthToken, OAuthError> {
    if cfg.token_url.trim().is_empty() {
        return Err(OAuthError::Missing("token URL"));
    }
    if cfg.client_id.trim().is_empty() {
        return Err(OAuthError::Missing("client ID"));
    }
    let client =
        crate::http::client_for(settings, cfg.token_url.trim()).map_err(|e| OAuthError::Transport(e.to_string()))?;
    if !cfg.scope.trim().is_empty() && !form.iter().any(|(k, _)| *k == "scope") {
        form.push(("scope", cfg.scope.clone()));
    }
    if !cfg.audience.trim().is_empty() {
        form.push(("audience", cfg.audience.clone()));
    }
    let mut builder = client.post(cfg.token_url.trim()).header("Accept", "application/json");
    match cfg.client_auth {
        ClientAuthMethod::Header => {
            // RFC 6749 §2.3.1: form-urlencode id and secret before Basic encoding.
            let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
            let creds = format!("{}:{}", enc(&cfg.client_id), enc(&cfg.client_secret));
            let b64 = base64::engine::general_purpose::STANDARD.encode(creds);
            builder = builder.header("Authorization", format!("Basic {b64}"));
            // Public clients (no secret) must still identify themselves.
            if cfg.client_secret.is_empty() {
                form.push(("client_id", cfg.client_id.clone()));
            }
        }
        ClientAuthMethod::Body => {
            form.push(("client_id", cfg.client_id.clone()));
            if !cfg.client_secret.is_empty() {
                form.push(("client_secret", cfg.client_secret.clone()));
            }
        }
    }
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form.iter()).finish();
    let resp = builder
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|e| OAuthError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let bytes = resp.bytes().map_err(|e| OAuthError::Transport(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(OAuthError::Server { status, body: String::from_utf8_lossy(&bytes).chars().take(500).collect() });
    }
    parse_token_response(&bytes)
}

/// Fetches a token for the non-interactive grants. `cfg` must be resolved.
pub fn fetch_token(cfg: &OAuth2Config, settings: &HttpSettings) -> Result<OAuthToken, OAuthError> {
    match cfg.grant {
        OAuth2Grant::ClientCredentials => {
            token_request(cfg, vec![("grant_type", "client_credentials".into())], settings)
        }
        OAuth2Grant::Password => {
            if cfg.username.is_empty() {
                return Err(OAuthError::Missing("username"));
            }
            token_request(
                cfg,
                vec![
                    ("grant_type", "password".into()),
                    ("username", cfg.username.clone()),
                    ("password", cfg.password.clone()),
                ],
                settings,
            )
        }
        OAuth2Grant::AuthorizationCode => Err(OAuthError::NeedsInteractive),
    }
}

/// Exchanges a refresh token. Keeps the old refresh token if none is returned.
pub fn refresh_token(cfg: &OAuth2Config, refresh: &str, settings: &HttpSettings) -> Result<OAuthToken, OAuthError> {
    let mut t = token_request(
        cfg,
        vec![("grant_type", "refresh_token".into()), ("refresh_token", refresh.to_string())],
        settings,
    )?;
    if t.refresh_token.is_none() {
        t.refresh_token = Some(refresh.to_string());
    }
    Ok(t)
}

/// Random alphanumeric string: URL-safe without encoding (also valid for PKCE).
fn random_string(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..len).map(|_| CHARS[rng.random_range(0..CHARS.len())] as char).collect()
}

/// PKCE S256 challenge for a verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// A pending authorization-code login.
pub struct AuthorizationRequest {
    /// Open this URL in the user's browser.
    pub url: String,
    state: String,
    verifier: Option<String>,
    listener: TcpListener,
    path: String,
}

/// Starts an authorization-code login: binds the redirect listener and
/// builds the authorization URL. `cfg` must be resolved.
pub fn begin_authorization(cfg: &OAuth2Config) -> Result<AuthorizationRequest, OAuthError> {
    if cfg.auth_url.trim().is_empty() {
        return Err(OAuthError::Missing("authorization URL"));
    }
    if cfg.client_id.trim().is_empty() {
        return Err(OAuthError::Missing("client ID"));
    }
    let redirect =
        url::Url::parse(cfg.redirect_uri.trim()).map_err(|_| OAuthError::BadRedirect(cfg.redirect_uri.clone()))?;
    let host = redirect.host_str().unwrap_or("");
    if redirect.scheme() != "http" || !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return Err(OAuthError::BadRedirect(cfg.redirect_uri.clone()));
    }
    let port = redirect.port().filter(|p| *p != 0).ok_or_else(|| OAuthError::BadRedirect(cfg.redirect_uri.clone()))?;
    let bind = if host == "[::1]" { format!("[::1]:{port}") } else { format!("127.0.0.1:{port}") };
    let listener =
        TcpListener::bind(&bind).map_err(|e| OAuthError::Transport(format!("cannot listen on {bind}: {e}")))?;

    let state = random_string(24);
    let verifier = cfg.use_pkce.then(|| random_string(64));
    let mut url = url::Url::parse(cfg.auth_url.trim()).map_err(|e| OAuthError::Transport(e.to_string()))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &cfg.client_id);
        q.append_pair("redirect_uri", cfg.redirect_uri.trim());
        q.append_pair("state", &state);
        if !cfg.scope.trim().is_empty() {
            q.append_pair("scope", cfg.scope.trim());
        }
        if !cfg.audience.trim().is_empty() {
            q.append_pair("audience", cfg.audience.trim());
        }
        if let Some(v) = &verifier {
            q.append_pair("code_challenge", &pkce_challenge(v));
            q.append_pair("code_challenge_method", "S256");
        }
    }
    Ok(AuthorizationRequest { url: url.to_string(), state, verifier, listener, path: redirect.path().to_string() })
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>PakPos</title><body style=\"font-family:sans-serif;background:#0f1b3f;color:#fff;display:grid;place-items:center;height:100vh;margin:0\"><div><h2 style=\"color:#f7941d\">PakPos</h2><p>%MSG%</p><p>You can close this tab.</p></div>";

impl AuthorizationRequest {
    /// Waits for the browser redirect, then exchanges the code for a token.
    pub fn finish(
        self,
        cfg: &OAuth2Config,
        settings: &HttpSettings,
        timeout: Duration,
    ) -> Result<OAuthToken, OAuthError> {
        let deadline = Instant::now() + timeout;
        self.listener.set_nonblocking(true).map_err(|e| OAuthError::Transport(e.to_string()))?;
        loop {
            if Instant::now() > deadline {
                return Err(OAuthError::Timeout);
            }
            let (stream, _) = match self.listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => return Err(OAuthError::Transport(e.to_string())),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else { continue };
            let reply = |mut s: &std::net::TcpStream, msg: &str| {
                let page = DONE_PAGE.replace("%MSG%", msg);
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
            };
            if url.path() != self.path {
                reply(&stream, "Waiting for the login redirect…");
                continue;
            }
            let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
            if let Some(err) = params.get("error") {
                let desc = params.get("error_description").cloned().unwrap_or_default();
                reply(&stream, "Login failed. Return to PakPos for details.");
                return Err(OAuthError::Denied(format!("{err} {desc}").trim().to_string()));
            }
            if params.get("state") != Some(&self.state) {
                reply(&stream, "State mismatch, login ignored.");
                return Err(OAuthError::Denied("state mismatch (possible CSRF)".into()));
            }
            let Some(code) = params.get("code") else {
                reply(&stream, "No authorization code received.");
                return Err(OAuthError::Denied("no code in redirect".into()));
            };
            reply(&stream, "Login complete ✔");
            let mut form = vec![
                ("grant_type", "authorization_code".to_string()),
                ("code", code.clone()),
                ("redirect_uri", cfg.redirect_uri.trim().to_string()),
            ];
            if let Some(v) = &self.verifier {
                form.push(("code_verifier", v.clone()));
            }
            return token_request(cfg, form, settings);
        }
    }
}

/// Makes sure `auth` (if it is OAuth 2.0) has a usable token in `scope`:
/// uses the store, refreshes expired tokens, or fetches a new one for the
/// non-interactive grants. Returns the new token (to persist) if one was obtained.
pub fn ensure_token(
    auth: &Auth,
    scope: &mut VarScope,
    store: &mut TokenStore,
    settings: &HttpSettings,
) -> Result<Option<(String, OAuthToken)>, OAuthError> {
    let Auth::OAuth2(cfg) = auth else { return Ok(None) };
    let cfg = cfg.resolved(scope);
    let key = cfg.cache_key();
    if let Some(t) = store.get(&key) {
        if t.is_fresh() {
            scope.oauth_tokens.insert(key, t.access_token.clone());
            return Ok(None);
        }
        if let Some(rt) = t.refresh_token.clone() {
            if let Ok(new) = refresh_token(&cfg, &rt, settings) {
                store.insert(key.clone(), new.clone());
                scope.oauth_tokens.insert(key.clone(), new.access_token.clone());
                return Ok(Some((key, new)));
            }
        }
    }
    if cfg.grant.is_interactive() {
        // Keep using an expired token rather than failing silently; the
        // server's 401 tells the user to log in again.
        if let Some(t) = store.get(&key) {
            scope.oauth_tokens.insert(key, t.access_token.clone());
            return Ok(None);
        }
        return Err(OAuthError::NeedsInteractive);
    }
    let new = fetch_token(&cfg, settings)?;
    store.insert(key.clone(), new.clone());
    scope.oauth_tokens.insert(key.clone(), new.access_token.clone());
    Ok(Some((key, new)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_base64url_sha256() {
        // Expected value computed independently with:
        // printf '<verifier>' | openssl dgst -sha256 -binary | base64 | tr '+/' '-_' | tr -d '='
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r7wW1gFWFOEjXk"),
            "bwWFMyPfdG9qreDhH2lmftFx_dFeLDalzcT1gb_j68g"
        );
    }

    #[test]
    fn parses_json_and_form_responses() {
        let t = parse_token_response(
            br#"{"access_token":"abc","token_type":"bearer","expires_in":3600,"refresh_token":"r"}"#,
        )
        .unwrap();
        assert_eq!(t.access_token, "abc");
        assert!(t.is_fresh());
        assert!(t.expires_in().unwrap() > 3500);
        let t = parse_token_response(b"access_token=xyz&scope=repo&token_type=bearer").unwrap();
        assert_eq!(t.access_token, "xyz");
        assert!(t.expires_at.is_none());
        assert!(matches!(parse_token_response(b"error=bad_verification_code"), Err(OAuthError::Denied(_))));
    }

    #[test]
    fn authorization_url_contains_pkce_and_state() {
        let cfg = OAuth2Config {
            grant: OAuth2Grant::AuthorizationCode,
            auth_url: "https://id.example.com/authorize".into(),
            client_id: "app".into(),
            scope: "openid profile".into(),
            redirect_uri: "http://127.0.0.1:0/callback".into(),
            ..Default::default()
        };
        // Port 0 is rejected: the redirect must name a fixed port.
        assert!(matches!(begin_authorization(&cfg), Err(OAuthError::BadRedirect(_))));
        let cfg = OAuth2Config { redirect_uri: "http://127.0.0.1:47811/cb".into(), ..cfg };
        let req = begin_authorization(&cfg).unwrap();
        assert!(req.url.starts_with("https://id.example.com/authorize?response_type=code&client_id=app"));
        assert!(req.url.contains("code_challenge_method=S256"));
        assert!(req.url.contains("scope=openid+profile"));
        assert!(req.url.contains(&format!("state={}", req.state)));
        let bad = OAuth2Config { redirect_uri: "https://evil.example/cb".into(), ..cfg };
        assert!(matches!(begin_authorization(&bad), Err(OAuthError::BadRedirect(_))));
    }

    #[test]
    fn interactive_grant_without_token_needs_login() {
        let auth = Auth::OAuth2(OAuth2Config { grant: OAuth2Grant::AuthorizationCode, ..Default::default() });
        let r = ensure_token(&auth, &mut VarScope::new(), &mut TokenStore::new(), &HttpSettings::default());
        assert!(matches!(r, Err(OAuthError::NeedsInteractive)));
    }
}
