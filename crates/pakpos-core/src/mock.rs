//! Mock server: serves the saved examples of a collection over HTTP.
//!
//! Routing uses each request's method and URL path. The host part
//! (`https://api.example.com`, `{{base_url}}`) is ignored, and path segments
//! written as `:id`, `{id}` or `{{id}}` match anything.
//!
//! When a route has several examples, the client can pick one with the
//! `x-mock-response-name` or `x-mock-response-code` header; otherwise the first
//! 2xx example is used (or the first one).

use crate::model::{Collection, Example, HttpMethod, Item, KeyValue};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// One route: a request that has examples.
#[derive(Debug, Clone)]
pub struct Route {
    pub method: HttpMethod,
    pub segments: Vec<Segment>,
    pub request_name: String,
    pub examples: Vec<Example>,
    /// Display form, e.g. `GET /users/:id`.
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Param(String),
}

/// Extracts the path of a request URL, dropping scheme, host and a leading
/// `{{base_url}}`-style variable.
pub fn url_path(url: &str) -> String {
    let mut u = url.trim();
    if let Some(i) = u.find("://") {
        u = &u[i + 3..];
        u = u.find('/').map(|j| &u[j..]).unwrap_or("/");
    } else if u.starts_with("{{") {
        if let Some(end) = u.find("}}") {
            u = &u[end + 2..];
        }
    } else if !u.starts_with('/') {
        // host:port/path without scheme
        u = u.find('/').map(|j| &u[j..]).unwrap_or("/");
    }
    let path = u.split(['?', '#']).next().unwrap_or("/");
    if path.is_empty() {
        "/".into()
    } else {
        path.to_string()
    }
}

fn parse_segments(path: &str) -> Vec<Segment> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(name) = s.strip_prefix(':') {
                Segment::Param(name.to_string())
            } else if s.starts_with("{{") && s.ends_with("}}") {
                Segment::Param(s[2..s.len() - 2].trim().to_string())
            } else if s.starts_with('{') && s.ends_with('}') {
                Segment::Param(s[1..s.len() - 1].to_string())
            } else {
                Segment::Literal(s.to_string())
            }
        })
        .collect()
}

/// Builds the routing table of a collection.
pub fn routes(collection: &Collection) -> Vec<Route> {
    fn walk(items: &[Item], out: &mut Vec<Route>) {
        for item in items {
            match item {
                Item::Folder(f) => walk(&f.items, out),
                Item::Request(r) if !r.examples.is_empty() && r.kind.is_http() => {
                    let path = url_path(&r.url);
                    out.push(Route {
                        method: r.method,
                        segments: parse_segments(&path),
                        request_name: r.name.clone(),
                        examples: r.examples.clone(),
                        pattern: format!("{} {}", r.method, path),
                    })
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&collection.items, &mut out);
    // Prefer routes with more literal segments (`/users/me` before `/users/:id`).
    out.sort_by_key(|r| std::cmp::Reverse(r.segments.iter().filter(|s| matches!(s, Segment::Literal(_))).count()));
    out
}

fn matches(route: &Route, method: &str, path: &str) -> bool {
    if !route.method.as_str().eq_ignore_ascii_case(method) {
        return false;
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    parts.len() == route.segments.len()
        && route.segments.iter().zip(&parts).all(|(seg, part)| match seg {
            Segment::Literal(l) => l == part,
            Segment::Param(_) => true,
        })
}

/// Picks the example for a request, honouring the selection headers.
pub fn choose<'a>(route: &'a Route, headers: &[(String, String)]) -> &'a Example {
    let header = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim());
    if let Some(name) = header("x-mock-response-name") {
        if let Some(e) = route.examples.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
            return e;
        }
    }
    if let Some(code) = header("x-mock-response-code").and_then(|c| c.parse::<u16>().ok()) {
        if let Some(e) = route.examples.iter().find(|e| e.status == code) {
            return e;
        }
    }
    route.examples.iter().find(|e| (200..300).contains(&e.status)).unwrap_or(&route.examples[0])
}

/// A request served by the mock server, for logs.
#[derive(Debug, Clone)]
pub struct Hit {
    pub method: String,
    pub path: String,
    pub status: u16,
    /// `request name › example name`, or `None` when nothing matched.
    pub matched: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

/// Mock server options.
#[derive(Debug, Clone)]
pub struct MockOptions {
    pub addr: SocketAddr,
    /// Artificial latency added to every response.
    pub delay: Duration,
}

impl Default for MockOptions {
    fn default() -> Self {
        Self { addr: ([127, 0, 0, 1], 8000).into(), delay: Duration::ZERO }
    }
}

/// A running mock server. Dropping it stops the server.
pub struct MockServer {
    pub addr: SocketAddr,
    pub hits: Receiver<Hit>,
    routes: Arc<RwLock<Vec<Route>>>,
    stop: Arc<AtomicBool>,
}

impl MockServer {
    /// Replaces the routes (e.g. after the collection was edited).
    pub fn reload(&self, collection: &Collection) {
        if let Ok(mut r) = self.routes.write() {
            *r = routes(collection);
        }
    }

    pub fn routes(&self) -> Vec<Route> {
        self.routes.read().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts serving `collection` on `options.addr` (port 0 picks a free port).
pub fn start(collection: &Collection, options: MockOptions) -> std::io::Result<MockServer> {
    let listener = TcpListener::bind(options.addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let routes = Arc::new(RwLock::new(routes(collection)));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    {
        let routes = routes.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let routes = routes.clone();
                        let tx = tx.clone();
                        let delay = options.delay;
                        std::thread::spawn(move || {
                            let _ = handle(stream, &routes, &tx, delay);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(30))
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        });
    }
    Ok(MockServer { addr, hits: rx, routes, stop })
}

fn reason(code: u16) -> &'static str {
    reqwest::StatusCode::from_u16(code).ok().and_then(|c| c.canonical_reason()).unwrap_or("")
}

fn handle(stream: TcpStream, routes: &RwLock<Vec<Route>>, tx: &Sender<Hit>, delay: Duration) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    // Keep-alive: serve requests until the client closes.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("/").to_string());
        let mut headers = Vec::new();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h)? == 0 {
                return Ok(());
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap_or(0);
                }
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        // Drain (and ignore) the request body.
        let mut body = vec![0; len.min(16 * 1024 * 1024)];
        reader.read_exact(&mut body)?;
        let close =
            headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close"));
        let path = target.split(['?', '#']).next().unwrap_or("/").to_string();
        let started = Instant::now();

        let (status, resp_headers, resp_body, matched) = if method.eq_ignore_ascii_case("OPTIONS") {
            (204, Vec::new(), String::new(), Some("CORS preflight".to_string()))
        } else {
            let routes = routes.read().map(|r| r.clone()).unwrap_or_default();
            match routes.iter().find(|r| matches(r, &method, &path)) {
                Some(route) => {
                    let ex = choose(route, &headers);
                    (
                        ex.status,
                        ex.headers.clone(),
                        ex.body.clone(),
                        Some(format!("{} › {}", route.request_name, ex.name)),
                    )
                }
                None => {
                    let available: Vec<&str> = routes.iter().map(|r| r.pattern.as_str()).collect();
                    let body = serde_json::json!({
                        "error": "no mock example matches this request",
                        "request": format!("{method} {path}"),
                        "available": available,
                    });
                    (404, vec![KeyValue::new("Content-Type", "application/json")], body.to_string(), None)
                }
            }
        };
        if !delay.is_zero() {
            std::thread::sleep(delay.saturating_sub(started.elapsed()));
        }
        let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
        let has = |name: &str| resp_headers.iter().any(|h| h.enabled && h.key.eq_ignore_ascii_case(name));
        for h in resp_headers.iter().filter(|h| h.enabled) {
            out.push_str(&format!("{}: {}\r\n", h.key, h.value));
        }
        if !has("content-type") && !resp_body.is_empty() {
            let ct = if serde_json::from_str::<serde_json::Value>(&resp_body).is_ok() {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            out.push_str(&format!("Content-Type: {ct}\r\n"));
        }
        out.push_str(
            "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: *\r\n",
        );
        if let Some(m) = &matched {
            out.push_str(&format!("X-PakPos-Mock: {}\r\n", m.replace(['\r', '\n'], " ")));
        }
        out.push_str(&format!("Content-Length: {}\r\n", resp_body.len()));
        out.push_str(if close { "Connection: close\r\n\r\n" } else { "Connection: keep-alive\r\n\r\n" });
        writer.write_all(out.as_bytes())?;
        writer.write_all(resp_body.as_bytes())?;
        writer.flush()?;
        let _ = tx.send(Hit { method: method.clone(), path, status, matched, at: chrono::Utc::now() });
        if close {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Request;

    #[test]
    fn paths_and_segments() {
        assert_eq!(url_path("{{base_url}}/users/:id?x=1"), "/users/:id");
        assert_eq!(url_path("https://api.example.com/v1/items"), "/v1/items");
        assert_eq!(url_path("localhost:8080/health"), "/health");
        assert_eq!(url_path("https://api.example.com"), "/");
        assert_eq!(
            parse_segments("/users/{{user_id}}/posts/{post}"),
            vec![
                Segment::Literal("users".into()),
                Segment::Param("user_id".into()),
                Segment::Literal("posts".into()),
                Segment::Param("post".into())
            ]
        );
    }

    fn collection() -> Collection {
        let ex = |name: &str, status: u16, body: &str| Example {
            id: crate::model::new_id(),
            name: name.into(),
            status,
            headers: vec![],
            body: body.into(),
        };
        let mut by_id = Request::new(HttpMethod::Get, "{{base}}/users/:id");
        by_id.name = "Get user".into();
        by_id.examples = vec![ex("missing", 404, r#"{"error":"nope"}"#), ex("ok", 200, r#"{"id":1}"#)];
        let mut me = Request::new(HttpMethod::Get, "{{base}}/users/me");
        me.name = "Me".into();
        me.examples = vec![ex("me", 200, r#"{"id":"me"}"#)];
        let mut c = Collection::new("m");
        c.items = vec![Item::Request(by_id), Item::Request(me)];
        c
    }

    #[test]
    fn routing_prefers_literals_and_headers_select_examples() {
        let rs = routes(&collection());
        let find = |m: &str, p: &str| rs.iter().find(|r| matches(r, m, p)).map(|r| r.request_name.clone());
        assert_eq!(find("GET", "/users/me"), Some("Me".into()));
        assert_eq!(find("GET", "/users/42"), Some("Get user".into()));
        assert_eq!(find("POST", "/users/42"), None);
        assert_eq!(find("GET", "/users/42/x"), None);
        let route = rs.iter().find(|r| r.request_name == "Get user").unwrap();
        assert_eq!(choose(route, &[]).name, "ok", "first 2xx by default");
        assert_eq!(choose(route, &[("X-Mock-Response-Code".into(), "404".into())]).name, "missing");
        assert_eq!(choose(route, &[("x-mock-response-name".into(), "MISSING".into())]).name, "missing");
    }

    #[test]
    fn serves_over_http() {
        let server =
            start(&collection(), MockOptions { addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }).unwrap();
        let base = format!("http://{}", server.addr);
        let client = reqwest::blocking::Client::new();
        let r = client.get(format!("{base}/users/7")).send().unwrap();
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers()["x-pakpos-mock"], "Get user › ok");
        assert_eq!(r.text().unwrap(), r#"{"id":1}"#);
        let r = client.get(format!("{base}/users/7")).header("x-mock-response-code", "404").send().unwrap();
        assert_eq!(r.status().as_u16(), 404);
        let r = client.delete(format!("{base}/nothing")).send().unwrap();
        assert_eq!(r.status().as_u16(), 404);
        assert!(r.text().unwrap().contains("GET /users/:id"));
        // Hits are reported right after the response is written.
        std::thread::sleep(Duration::from_millis(150));
        let hits: Vec<Hit> = server.hits.try_iter().collect();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[2].matched, None);
    }
}
