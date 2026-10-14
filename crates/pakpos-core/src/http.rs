//! Turns a [`Request`] into a concrete HTTP call and executes it.

use crate::auth::{digest, ntlm, sigv4, ChallengeAuth};
use crate::model::{ApiKeyLocation, Auth, Body, KeyValue, Request};
use crate::vars::VarScope;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Errors that can happen while preparing or sending a request.
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("URL is empty")]
    EmptyUrl,
    #[error("invalid URL `{0}`: {1}")]
    InvalidUrl(String, String),
    #[error("cannot read file `{0}`: {1}")]
    File(String, String),
    #[error("request failed: {0}")]
    Transport(String),
    #[error("invalid body: {0}")]
    Body(String),
}

/// Client-side settings applied to every request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpSettings {
    pub timeout_secs: u64,
    pub follow_redirects: bool,
    pub max_redirects: usize,
    pub verify_tls: bool,
    /// Optional proxy URL (e.g. `http://127.0.0.1:8080`).
    #[serde(default)]
    pub proxy: String,
    pub user_agent: String,
    /// Client certificates (mTLS), picked by host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_certs: Vec<ClientCert>,
}

/// A client certificate used for hosts matching `host`
/// (`api.example.com`, `*.example.com`, `localhost:8443`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClientCert {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub host: String,
    /// PEM file with the certificate (and optionally the chain and key).
    /// May be empty when the entry only adds a CA.
    #[serde(default)]
    pub cert_path: String,
    /// PEM file with the private key; empty when it is inside `cert_path`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key_path: String,
    /// Extra CA (PEM) to trust for this host, for servers with a private CA.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ca_path: String,
}

fn default_true() -> bool {
    true
}

impl HttpSettings {
    /// The first enabled client certificate whose host pattern matches `url`.
    pub fn client_cert_for(&self, url: &str) -> Option<&ClientCert> {
        self.client_certs.iter().filter(|c| c.enabled && !c.host.trim().is_empty()).find(|c| {
            host_allowed(url, std::slice::from_ref(&c.host))
                || url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.eq_ignore_ascii_case(c.host.trim())))
                    == Some(true)
        })
    }
}

impl ClientCert {
    /// Reads the PEM data: certificate chain followed by the key.
    pub fn pem(&self) -> Result<Vec<u8>, HttpError> {
        let read = |p: &str| std::fs::read(p).map_err(|e| HttpError::File(p.to_string(), e.to_string()));
        let mut pem = read(self.cert_path.trim())?;
        if !self.key_path.trim().is_empty() {
            pem.push(b'\n');
            pem.extend(read(self.key_path.trim())?);
        }
        Ok(pem)
    }

    /// The extra CA, if one is configured.
    pub fn ca_pem(&self) -> Result<Option<Vec<u8>>, HttpError> {
        let path = self.ca_path.trim();
        if path.is_empty() {
            return Ok(None);
        }
        std::fs::read(path).map(Some).map_err(|e| HttpError::File(path.to_string(), e.to_string()))
    }
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            follow_redirects: true,
            max_redirects: 10,
            verify_tls: true,
            proxy: String::new(),
            user_agent: format!("PakPos/{}", env!("CARGO_PKG_VERSION")),
            client_certs: Vec::new(),
        }
    }
}

/// Body after variable resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedBody {
    None,
    Bytes(Vec<u8>),
    Text(String),
    Form(Vec<(String, String)>),
    /// (key, value, is_file)
    Multipart(Vec<(String, String, bool)>),
}

/// A request with every variable resolved and auth applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: PreparedBody,
    /// Digest / NTLM credentials, answered in [`send`] after the server's challenge.
    pub challenge: Option<ChallengeAuth>,
}

impl PreparedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A received HTTP response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub elapsed_ms: u128,
    pub url: String,
    pub http_version: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }

    pub fn is_json(&self) -> bool {
        self.header("content-type").map(|c| c.contains("json")).unwrap_or(false) || self.json().is_some()
    }

    /// Body pretty-printed if it is JSON, otherwise as text.
    pub fn pretty_body(&self) -> String {
        match self.json() {
            Some(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| self.text()),
            None => self.text(),
        }
    }

    pub fn size_bytes(&self) -> usize {
        self.body.len()
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Human readable byte size (`1.2 KB`).
pub fn format_size(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn enabled(kvs: &[KeyValue], scope: &VarScope) -> Vec<(String, String)> {
    kvs.iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (scope.resolve(&kv.key), scope.resolve(&kv.value)))
        .collect()
}

/// Resolves variables, appends query params and applies auth.
///
/// `fallback_auth` is used when the request's auth is [`Auth::Inherit`]
/// (normally the effective auth of the parent folder / collection).
pub fn prepare(req: &Request, scope: &VarScope, fallback_auth: Option<&Auth>) -> Result<PreparedRequest, HttpError> {
    let raw_url = scope.resolve(req.url.trim());
    if raw_url.is_empty() {
        return Err(HttpError::EmptyUrl);
    }
    // Like Postman, accept URLs without a scheme.
    let with_scheme = if raw_url.contains("://") { raw_url.clone() } else { format!("http://{raw_url}") };
    let mut url = url::Url::parse(&with_scheme).map_err(|e| HttpError::InvalidUrl(raw_url.clone(), e.to_string()))?;

    let params = enabled(&req.params, scope);
    if !params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in &params {
            pairs.append_pair(k, v);
        }
    }

    let mut headers = enabled(&req.headers, scope);
    let has_header =
        |headers: &[(String, String)], name: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));

    let auth = match &req.auth {
        Auth::Inherit => fallback_auth.cloned().unwrap_or(Auth::None),
        other => other.clone(),
    };
    let mut challenge = None;
    let mut aws = None;
    match auth {
        Auth::Inherit | Auth::None => {}
        Auth::Digest { username, password } => {
            challenge =
                Some(ChallengeAuth::Digest { username: scope.resolve(&username), password: scope.resolve(&password) });
        }
        Auth::Ntlm { username, password, domain, workstation } => {
            challenge = Some(ChallengeAuth::Ntlm {
                username: scope.resolve(&username),
                password: scope.resolve(&password),
                domain: scope.resolve(&domain),
                workstation: scope.resolve(&workstation),
            });
        }
        Auth::AwsSigV4(cfg) => {
            let r = |s: &str| scope.resolve(s).trim().to_string();
            aws = Some(sigv4::Credentials {
                access_key: r(&cfg.access_key),
                secret_key: r(&cfg.secret_key),
                session_token: r(&cfg.session_token),
                region: r(&cfg.region),
                service: r(&cfg.service),
            });
        }
        Auth::Basic { username, password } => {
            let creds = format!("{}:{}", scope.resolve(&username), scope.resolve(&password));
            let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
            headers.push(("Authorization".into(), format!("Basic {encoded}")));
        }
        Auth::Bearer { token } => {
            headers.push(("Authorization".into(), format!("Bearer {}", scope.resolve(&token))));
        }
        Auth::OAuth2(cfg) => {
            let cfg = cfg.resolved(scope);
            if let Some(token) = scope.oauth_tokens.get(&cfg.cache_key()) {
                let prefix = cfg.header_prefix.trim();
                let value = if prefix.is_empty() { token.clone() } else { format!("{prefix} {token}") };
                headers.push(("Authorization".into(), value));
            }
        }
        Auth::ApiKey { key, value, location } => {
            let (k, v) = (scope.resolve(&key), scope.resolve(&value));
            match location {
                ApiKeyLocation::Header => headers.push((k, v)),
                ApiKeyLocation::Query => {
                    url.query_pairs_mut().append_pair(&k, &v);
                }
            }
        }
    }

    let body = match &req.body {
        Body::None => PreparedBody::None,
        Body::Raw { content, language } => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), language.content_type().into()));
            }
            PreparedBody::Text(scope.resolve(content))
        }
        Body::FormUrlEncoded { fields } => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            }
            PreparedBody::Form(enabled(fields, scope))
        }
        Body::Multipart { fields } => PreparedBody::Multipart(
            fields
                .iter()
                .filter(|f| f.enabled && !f.key.trim().is_empty())
                .map(|f| (scope.resolve(&f.key), scope.resolve(&f.value), f.is_file))
                .collect(),
        ),
        Body::Binary { path } => {
            let path = scope.resolve(path);
            if path.is_empty() {
                PreparedBody::None
            } else {
                let bytes = std::fs::read(&path).map_err(|e| HttpError::File(path, e.to_string()))?;
                if !has_header(&headers, "content-type") {
                    headers.push(("Content-Type".into(), "application/octet-stream".into()));
                }
                PreparedBody::Bytes(bytes)
            }
        }
        Body::GraphQl { query, variables, operation_name } => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            let mut payload = serde_json::Map::new();
            payload.insert("query".into(), serde_json::Value::String(scope.resolve(query)));
            let vars = scope.resolve(variables);
            if !vars.trim().is_empty() {
                let v: serde_json::Value = serde_json::from_str(&vars)
                    .map_err(|e| HttpError::Body(format!("GraphQL variables are not valid JSON: {e}")))?;
                payload.insert("variables".into(), v);
            }
            if !operation_name.trim().is_empty() {
                payload.insert("operationName".into(), serde_json::Value::String(scope.resolve(operation_name.trim())));
            }
            PreparedBody::Text(serde_json::Value::Object(payload).to_string())
        }
    };

    let method = req.method.as_str().to_string();
    if let Some(creds) = aws {
        let form_bytes;
        let payload: Option<&[u8]> = match &body {
            PreparedBody::None => Some(b""),
            PreparedBody::Bytes(b) => Some(b),
            PreparedBody::Text(t) => Some(t.as_bytes()),
            PreparedBody::Form(fields) => {
                form_bytes = encode_form(fields);
                Some(form_bytes.as_bytes())
            }
            PreparedBody::Multipart(_) => None,
        };
        let input = sigv4::SignInput { method: &method, url: &url, headers: &headers, body: payload };
        let signed = sigv4::sign(&input, &creds, &sigv4::now_amz_date());
        headers.extend(signed);
    }

    Ok(PreparedRequest { method, url: url.to_string(), headers, body, challenge })
}

fn encode_form(fields: &[(String, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields.iter()).finish()
}

/// Builds a blocking client for the given settings, without a client certificate.
pub fn build_client(settings: &HttpSettings) -> Result<reqwest::blocking::Client, HttpError> {
    build_client_with(settings, None)
}

/// Builds a client presenting the certificate configured for `url`'s host, if any.
pub fn client_for(settings: &HttpSettings, url: &str) -> Result<reqwest::blocking::Client, HttpError> {
    build_client_with(settings, settings.client_cert_for(url))
}

/// Clients per client certificate, for callers sending many requests.
pub struct Clients {
    settings: HttpSettings,
    cache: std::collections::HashMap<Option<usize>, reqwest::blocking::Client>,
}

impl Clients {
    pub fn new(settings: HttpSettings) -> Self {
        Self { settings, cache: Default::default() }
    }

    pub fn for_url(&mut self, url: &str) -> Result<&reqwest::blocking::Client, HttpError> {
        let cert = self.settings.client_cert_for(url);
        let key = cert.and_then(|c| self.settings.client_certs.iter().position(|x| x == c));
        if !self.cache.contains_key(&key) {
            let client = build_client_with(&self.settings, cert)?;
            self.cache.insert(key, client);
        }
        Ok(&self.cache[&key])
    }
}

fn build_client_with(
    settings: &HttpSettings,
    cert: Option<&ClientCert>,
) -> Result<reqwest::blocking::Client, HttpError> {
    let redirect = if settings.follow_redirects {
        reqwest::redirect::Policy::limited(settings.max_redirects)
    } else {
        reqwest::redirect::Policy::none()
    };
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(settings.timeout_secs.max(1)))
        .redirect(redirect)
        .user_agent(settings.user_agent.clone())
        .cookie_store(true)
        .tls_danger_accept_invalid_certs(!settings.verify_tls);
    if !settings.proxy.trim().is_empty() {
        let proxy = reqwest::Proxy::all(settings.proxy.trim())
            .map_err(|e| HttpError::InvalidUrl(settings.proxy.clone(), e.to_string()))?;
        builder = builder.proxy(proxy);
    }
    if let Some(cert) = cert {
        if !cert.cert_path.trim().is_empty() {
            let identity = reqwest::Identity::from_pem(&cert.pem()?).map_err(|e| {
                HttpError::Transport(format!("client certificate `{}`: {}", cert.cert_path, describe_error(&e)))
            })?;
            builder = builder.identity(identity);
        }
        if let Some(ca) = cert.ca_pem()? {
            let certs = reqwest::Certificate::from_pem_bundle(&ca)
                .map_err(|e| HttpError::Transport(format!("CA `{}`: {}", cert.ca_path, describe_error(&e))))?;
            for c in certs {
                builder = builder.add_root_certificate(c);
            }
        }
    }
    builder.build().map_err(|e| HttpError::Transport(e.to_string()))
}

/// Sends a prepared request with an existing client, answering a Digest or
/// NTLM challenge when the request carries those credentials.
pub fn send(client: &reqwest::blocking::Client, prepared: &PreparedRequest) -> Result<Response, HttpError> {
    let started = Instant::now();
    let mut response = match &prepared.challenge {
        None => send_once(client, prepared, None)?,
        Some(ChallengeAuth::Digest { username, password }) => {
            let first = send_once(client, prepared, None)?;
            match (first.status, digest::find_challenge(header_values(&first, "www-authenticate"))) {
                (401, Some(ch)) => {
                    let uri = request_target(&prepared.url);
                    let cnonce = crate::auth::hex(&crate::auth::random_bytes::<8>());
                    let body = body_bytes(&prepared.body);
                    let header = digest::authorization(
                        &ch,
                        username,
                        password,
                        &prepared.method,
                        &uri,
                        body.as_deref().unwrap_or_default(),
                        &cnonce,
                        1,
                    );
                    send_once(client, prepared, Some(header))?
                }
                _ => first,
            }
        }
        Some(ChallengeAuth::Ntlm { username, password, domain, workstation }) => {
            let first = send_once(client, prepared, Some(ntlm::negotiate_header()))?;
            match (first.status, ntlm::find_challenge(header_values(&first, "www-authenticate"))) {
                (401, Some(ch)) => {
                    let header = ntlm::authenticate_header(&ch, username, password, domain, workstation);
                    send_once(client, prepared, Some(header))?
                }
                _ => first,
            }
        }
    };
    response.elapsed_ms = started.elapsed().as_millis();
    Ok(response)
}

fn header_values<'a>(resp: &'a Response, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    resp.headers.iter().filter(move |(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Path and query of a URL, as used in the HTTP request line.
fn request_target(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        },
        Err(_) => url.to_string(),
    }
}

fn body_bytes(body: &PreparedBody) -> Option<Vec<u8>> {
    match body {
        PreparedBody::None => Some(Vec::new()),
        PreparedBody::Bytes(b) => Some(b.clone()),
        PreparedBody::Text(t) => Some(t.as_bytes().to_vec()),
        PreparedBody::Form(f) => Some(encode_form(f).into_bytes()),
        PreparedBody::Multipart(_) => None,
    }
}

fn send_once(
    client: &reqwest::blocking::Client,
    prepared: &PreparedRequest,
    authorization: Option<String>,
) -> Result<Response, HttpError> {
    let method =
        reqwest::Method::from_bytes(prepared.method.as_bytes()).map_err(|e| HttpError::Transport(e.to_string()))?;
    let mut builder = client.request(method, &prepared.url);
    for (k, v) in &prepared.headers {
        if authorization.is_some() && k.eq_ignore_ascii_case("authorization") {
            continue;
        }
        builder = builder.header(k, v);
    }
    if let Some(a) = authorization {
        builder = builder.header("Authorization", a);
    }
    builder = match &prepared.body {
        PreparedBody::None => builder,
        PreparedBody::Bytes(b) => builder.body(b.clone()),
        PreparedBody::Text(t) => builder.body(t.clone()),
        PreparedBody::Form(fields) => builder.body(encode_form(fields)),
        PreparedBody::Multipart(fields) => {
            let mut form = reqwest::blocking::multipart::Form::new();
            for (k, v, is_file) in fields {
                if *is_file {
                    form = form.file(k.clone(), v).map_err(|e| HttpError::File(v.clone(), e.to_string()))?;
                } else {
                    form = form.text(k.clone(), v.clone());
                }
            }
            builder.multipart(form)
        }
    };

    let started = Instant::now();
    let resp = builder.send().map_err(|e| HttpError::Transport(describe_error(&e)))?;
    let status = resp.status();
    let url = resp.url().to_string();
    let http_version = format!("{:?}", resp.version());
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let body = resp.bytes().map_err(|e| HttpError::Transport(describe_error(&e)))?.to_vec();
    let elapsed_ms = started.elapsed().as_millis();

    Ok(Response {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers,
        body,
        elapsed_ms,
        url,
        http_version,
    })
}

/// Convenience: prepare + build client + send.
pub fn execute(
    req: &Request,
    scope: &VarScope,
    fallback_auth: Option<&Auth>,
    settings: &HttpSettings,
) -> Result<(PreparedRequest, Response), HttpError> {
    let prepared = prepare(req, scope, fallback_auth)?;
    let client = client_for(settings, &prepared.url)?;
    let response = send(&client, &prepared)?;
    Ok((prepared, response))
}

/// Checks the host of `url` against allow-list patterns (`api.example.com`,
/// `*.example.com`, `localhost:8080`, `*`). An empty list allows everything.
pub fn host_allowed(url: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let with_port = parsed.port_or_known_default().map(|p| format!("{host}:{p}")).unwrap_or_else(|| host.clone());
    patterns.iter().any(|p| {
        let p = p.trim().to_ascii_lowercase();
        crate::assertions::glob_match(&p, &host) || crate::assertions::glob_match(&p, &with_port)
    })
}

fn describe_error(e: &reqwest::Error) -> String {
    use std::error::Error;
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HttpMethod, RawLanguage};

    #[test]
    fn prepare_applies_vars_params_and_auth() {
        let mut scope = VarScope::new();
        scope.environment.insert("host".into(), "api.test".into());
        scope.environment.insert("tok".into(), "secret".into());
        let mut req = Request::new(HttpMethod::Post, "https://{{host}}/items?x=1");
        req.params.push(KeyValue::new("q", "a b"));
        let mut disabled = KeyValue::new("skip", "me");
        disabled.enabled = false;
        req.params.push(disabled);
        req.auth = Auth::Bearer { token: "{{tok}}".into() };
        req.body = Body::Raw { content: "{\"host\":\"{{host}}\"}".into(), language: RawLanguage::Json };
        let p = prepare(&req, &scope, None).unwrap();
        assert_eq!(p.url, "https://api.test/items?x=1&q=a+b");
        assert_eq!(p.header("authorization"), Some("Bearer secret"));
        assert_eq!(p.header("content-type"), Some("application/json"));
        assert_eq!(p.body, PreparedBody::Text("{\"host\":\"api.test\"}".into()));
    }

    #[test]
    fn prepare_basic_and_inherit() {
        let req = Request::new(HttpMethod::Get, "localhost:8080/");
        let auth = Auth::Basic { username: "u".into(), password: "p".into() };
        let p = prepare(&req, &VarScope::new(), Some(&auth)).unwrap();
        assert_eq!(p.url, "http://localhost:8080/");
        assert_eq!(p.header("Authorization"), Some("Basic dTpw"));
    }

    #[test]
    fn api_key_in_query() {
        let mut req = Request::new(HttpMethod::Get, "http://h/");
        req.auth = Auth::ApiKey { key: "k".into(), value: "v".into(), location: ApiKeyLocation::Query };
        let p = prepare(&req, &VarScope::new(), None).unwrap();
        assert_eq!(p.url, "http://h/?k=v");
    }

    #[test]
    fn empty_url_is_error() {
        let req = Request::new(HttpMethod::Get, "  ");
        assert!(matches!(prepare(&req, &VarScope::new(), None), Err(HttpError::EmptyUrl)));
    }

    #[test]
    fn graphql_body_is_json_envelope() {
        let mut scope = VarScope::new();
        scope.environment.insert("uid".into(), "42".into());
        let mut req = Request::new(HttpMethod::Post, "http://h/graphql");
        req.body = Body::GraphQl {
            query: "query($id: ID!) { user(id: $id) { name } }".into(),
            variables: r#"{"id": "{{uid}}"}"#.into(),
            operation_name: String::new(),
        };
        let p = prepare(&req, &scope, None).unwrap();
        assert_eq!(p.header("content-type"), Some("application/json"));
        let PreparedBody::Text(t) = p.body else { panic!() };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        assert_eq!(v["variables"]["id"], "42");
        assert!(v.get("operationName").is_none());
        req.body = Body::GraphQl { query: "{a}".into(), variables: "{oops".into(), operation_name: String::new() };
        assert!(matches!(prepare(&req, &scope, None), Err(HttpError::Body(_))));
    }

    #[test]
    fn host_allow_list() {
        let list = vec!["*.example.com".to_string(), "localhost:8080".to_string()];
        assert!(host_allowed("https://api.example.com/x", &list));
        assert!(host_allowed("http://localhost:8080/", &list));
        assert!(!host_allowed("http://localhost:9000/", &list));
        assert!(!host_allowed("http://169.254.169.254/latest", &list));
        assert!(host_allowed("http://anything/", &[]));
    }

    #[test]
    fn size_formatting() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn client_certificate_is_picked_by_host() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mtls");
        let cert = |host: &str| ClientCert {
            enabled: true,
            host: host.into(),
            cert_path: format!("{dir}/client.pem"),
            key_path: format!("{dir}/client.key"),
            ca_path: format!("{dir}/ca.pem"),
        };
        let settings = HttpSettings {
            client_certs: vec![
                ClientCert { enabled: false, ..cert("*") },
                cert("*.internal.example"),
                cert("localhost"),
            ],
            ..Default::default()
        };
        assert_eq!(settings.client_cert_for("https://api.internal.example/x").unwrap().host, "*.internal.example");
        assert_eq!(settings.client_cert_for("https://localhost:8443/").unwrap().host, "localhost");
        assert!(settings.client_cert_for("https://example.com/").is_none());
        let mut clients = Clients::new(settings);
        clients.for_url("https://localhost:8443/").expect("PEM identity and CA load");
        clients.for_url("https://example.com/").unwrap();
        assert_eq!(clients.cache.len(), 2);
    }
}
