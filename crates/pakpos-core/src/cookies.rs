//! A persistent cookie jar (RFC 6265 matching), stored as `cookies.json` in
//! the workspace and shared by every request.

use crate::http::{PreparedRequest, Response};
use serde::{Deserialize, Serialize};

/// One stored cookie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    /// Lowercase, without a leading dot.
    pub domain: String,
    #[serde(default = "root_path")]
    pub path: String,
    /// Set without a `Domain` attribute: only sent to exactly `domain`.
    #[serde(default)]
    pub host_only: bool,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    /// Unix seconds; `None` for a session cookie (kept until deleted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<i64>,
}

fn root_path() -> String {
    "/".into()
}

impl StoredCookie {
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|e| e <= now)
    }

    fn same_key(&self, other: &StoredCookie) -> bool {
        self.name == other.name && self.domain == other.domain && self.path == other.path
    }

    /// Does this cookie go with a request to `url`?
    pub fn matches(&self, url: &url::Url) -> bool {
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return false };
        let domain_ok = if self.host_only {
            host == self.domain
        } else {
            host == self.domain || host.ends_with(&format!(".{}", self.domain))
        };
        domain_ok && path_matches(&self.path, url.path()) && (!self.secure || url.scheme() == "https")
    }
}

/// RFC 6265 section 5.1.4.
fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    let request_path = if request_path.is_empty() { "/" } else { request_path };
    request_path == cookie_path
        || (request_path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || request_path[cookie_path.len()..].starts_with('/')))
}

/// RFC 6265 section 5.1.4 default path.
fn default_path(url: &url::Url) -> String {
    let path = url.path();
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// All cookies, in insertion order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CookieJar {
    #[serde(default)]
    pub cookies: Vec<StoredCookie>,
}

impl CookieJar {
    /// `Cookie` header value for `url`, longest paths first (RFC 6265 5.4).
    pub fn header_for(&self, url: &str) -> Option<String> {
        let url = url::Url::parse(url).ok()?;
        let now = now();
        let mut matching: Vec<&StoredCookie> =
            self.cookies.iter().filter(|c| !c.is_expired(now) && c.matches(&url)).collect();
        matching.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        let header = matching.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; ");
        (!header.is_empty()).then_some(header)
    }

    /// Merges the jar's cookies into a `Cookie` header set by hand (the
    /// hand-written ones win on equal names). Without such a header nothing
    /// changes: the client's cookie provider ([`SharedJar`]) sends them,
    /// also on redirects.
    pub fn merge_into_manual(&self, prepared: &mut PreparedRequest) {
        let Some(stored) = self.header_for(&prepared.url) else { return };
        let Some((_, existing)) = prepared.headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("cookie")) else {
            return;
        };
        let manual: Vec<String> =
            existing.split(';').filter_map(|p| p.split_once('=')).map(|(k, _)| k.trim().to_string()).collect();
        let extra: Vec<&str> = stored
            .split("; ")
            .filter(|p| p.split_once('=').is_some_and(|(k, _)| !manual.iter().any(|m| m == k)))
            .collect();
        if !extra.is_empty() {
            existing.push_str("; ");
            existing.push_str(&extra.join("; "));
        }
    }

    /// Stores every `Set-Cookie` of a response. Returns how many changed.
    pub fn store_response(&mut self, response: &Response) -> usize {
        let url = response.url.clone();
        response
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| self.set_cookie(&url, v))
            .filter(|changed| *changed)
            .count()
    }

    /// Parses one `Set-Cookie` header received from `url`.
    pub fn set_cookie(&mut self, url: &str, header: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else { return false };
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return false };
        let Ok(parsed) = cookie::Cookie::parse(header.to_string()) else { return false };

        let (domain, host_only) = match parsed.domain().map(|d| d.trim_start_matches('.').to_ascii_lowercase()) {
            Some(d) if !d.is_empty() => {
                // A server may only set cookies for its own domain or a parent of it.
                if host != d && !host.ends_with(&format!(".{d}")) {
                    return false;
                }
                (d, false)
            }
            _ => (host, true),
        };
        let path = match parsed.path() {
            Some(p) if p.starts_with('/') => p.to_string(),
            _ => default_path(&url),
        };
        let expires = match (parsed.max_age(), parsed.expires_datetime()) {
            (Some(age), _) => Some(now() + age.whole_seconds()),
            (None, Some(at)) => Some(at.unix_timestamp()),
            _ => None,
        };
        let cookie = StoredCookie {
            name: parsed.name().to_string(),
            value: parsed.value().to_string(),
            domain,
            path,
            host_only,
            secure: parsed.secure().unwrap_or(false),
            http_only: parsed.http_only().unwrap_or(false),
            expires,
        };
        self.upsert(cookie)
    }

    /// Inserts or replaces a cookie; an expired one deletes its match.
    pub fn upsert(&mut self, cookie: StoredCookie) -> bool {
        let existing = self.cookies.iter().position(|c| c.same_key(&cookie));
        if cookie.is_expired(now()) {
            return existing.map(|i| self.cookies.remove(i)).is_some();
        }
        match existing {
            Some(i) if self.cookies[i] == cookie => false,
            Some(i) => {
                self.cookies[i] = cookie;
                true
            }
            None => {
                self.cookies.push(cookie);
                true
            }
        }
    }

    pub fn remove_expired(&mut self) -> usize {
        let now = now();
        let before = self.cookies.len();
        self.cookies.retain(|c| !c.is_expired(now));
        before - self.cookies.len()
    }

    /// Distinct domains, sorted.
    pub fn domains(&self) -> Vec<String> {
        let mut d: Vec<String> = self.cookies.iter().map(|c| c.domain.clone()).collect();
        d.sort();
        d.dedup();
        d
    }
}

/// A jar shared between threads and plugged into the HTTP client, so cookies
/// set on redirects are stored too.
#[derive(Debug, Default)]
pub struct SharedJar(std::sync::Mutex<CookieJar>);

impl SharedJar {
    pub fn new(jar: CookieJar) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::Mutex::new(jar)))
    }

    pub fn snapshot(&self) -> CookieJar {
        self.0.lock().map(|j| j.clone()).unwrap_or_default()
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut CookieJar) -> R) -> R {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }
}

impl reqwest::cookie::CookieStore for SharedJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &reqwest::header::HeaderValue>, url: &url::Url) {
        let headers: Vec<String> = cookie_headers.filter_map(|h| h.to_str().ok().map(str::to_string)).collect();
        self.with(|jar| {
            for h in &headers {
                jar.set_cookie(url.as_str(), h);
            }
        });
    }

    fn cookies(&self, url: &url::Url) -> Option<reqwest::header::HeaderValue> {
        self.with(|jar| jar.header_for(url.as_str())).and_then(|h| reqwest::header::HeaderValue::from_str(&h).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(url: &str, set_cookies: &[&str]) -> Response {
        Response {
            status: 200,
            status_text: "OK".into(),
            headers: set_cookies.iter().map(|c| ("Set-Cookie".to_string(), c.to_string())).collect(),
            body: Vec::new(),
            elapsed_ms: 1,
            url: url.into(),
            http_version: "HTTP/1.1".into(),
        }
    }

    #[test]
    fn stores_and_sends_by_domain_path_and_scheme() {
        let mut jar = CookieJar::default();
        let n = jar.store_response(&resp(
            "https://api.example.com/v1/login",
            &[
                "session=abc; Path=/; HttpOnly; Secure",
                "pref=dark; Domain=.example.com; Path=/",
                "scoped=1",
                "evil=1; Domain=other.com",
            ],
        ));
        assert_eq!(n, 3, "{:?}", jar.cookies);
        let scoped = jar.cookies.iter().find(|c| c.name == "scoped").unwrap();
        assert_eq!(scoped.path, "/v1");
        assert!(scoped.host_only);

        assert_eq!(
            jar.header_for("https://api.example.com/v1/users").as_deref(),
            Some("scoped=1; session=abc; pref=dark")
        );
        // Secure cookie not over http, host-only cookie not on a sibling host.
        assert_eq!(jar.header_for("http://www.example.com/").as_deref(), Some("pref=dark"));
        assert_eq!(jar.header_for("https://api.example.com/v10").as_deref(), Some("session=abc; pref=dark"));
        assert!(jar.header_for("https://example.org/").is_none());
    }

    #[test]
    fn max_age_zero_deletes_and_manual_header_wins() {
        let mut jar = CookieJar::default();
        jar.set_cookie("http://localhost:8080/", "a=1");
        jar.set_cookie("http://localhost:8080/", "b=2; Max-Age=3600");
        assert!(jar.cookies[1].expires.is_some());
        assert!(jar.set_cookie("http://localhost:8080/", "a=gone; Max-Age=0"));
        assert_eq!(jar.cookies.len(), 1);

        jar.set_cookie("http://localhost:8080/", "c=3");
        let mut p = PreparedRequest {
            method: "GET".into(),
            url: "http://localhost:8080/x".into(),
            headers: vec![("Cookie".into(), "b=manual".into())],
            body: crate::http::PreparedBody::None,
            challenge: None,
        };
        jar.merge_into_manual(&mut p);
        assert_eq!(p.header("cookie"), Some("b=manual; c=3"));
    }
}
