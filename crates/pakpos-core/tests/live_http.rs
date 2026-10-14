//! End-to-end tests against a tiny local HTTP server (no internet needed).

use pakpos_core::assertions::Operator;
use pakpos_core::http::{self, HttpSettings};
use pakpos_core::model::{Assertion, Extraction, Item};
use pakpos_core::runner::{run_collection, RunOptions};
use pakpos_core::vars::VarScope;
use pakpos_core::{Body, Collection, Environment, HttpMethod, KeyValue, Request};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

/// Starts a server that echoes method, path, a header and the body as JSON.
fn echo_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut len = 0usize;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    let h = h.trim_end().to_string();
                    if h.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        match k.to_ascii_lowercase().as_str() {
                            "content-length" => len = v.trim().parse().unwrap_or(0),
                            "authorization" => auth = v.trim().to_string(),
                            _ => {}
                        }
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let payload = serde_json::json!({
                    "method": method,
                    "path": path,
                    "auth": auth,
                    "body": String::from_utf8_lossy(&body),
                    "token": "tok-123",
                })
                .to_string();
                let status = if path.starts_with("/missing") { "404 Not Found" } else { "200 OK" };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn sends_request_and_reads_response() {
    let base = echo_server();
    let mut scope = VarScope::new();
    scope.environment.insert("base".into(), base);
    let mut req = Request::new(HttpMethod::Post, "{{base}}/items");
    req.params.push(KeyValue::new("q", "1"));
    req.body = Body::Raw { content: "hello".into(), language: pakpos_core::model::RawLanguage::Text };
    let (_, resp) = http::execute(&req, &scope, None, &HttpSettings::default()).unwrap();
    assert_eq!(resp.status, 200);
    let json = resp.json().unwrap();
    assert_eq!(json["method"], "POST");
    assert_eq!(json["path"], "/items?q=1");
    assert_eq!(json["body"], "hello");
}

#[test]
fn runner_chains_extracted_variables() {
    let base = echo_server();
    let mut env = Environment::new("test");
    env.set("base", base);

    let mut login = Request::new(HttpMethod::Post, "{{base}}/login");
    login.name = "Login".into();
    login.extractions.push(Extraction { enabled: true, variable: "token".into(), source: "json.token".into() });
    let mut me = Request::new(HttpMethod::Get, "{{base}}/me");
    me.name = "Me".into();
    me.auth = pakpos_core::Auth::Bearer { token: "{{token}}".into() };
    me.assertions.push(Assertion {
        enabled: true,
        target: "json.auth".into(),
        op: Operator::Equals,
        expected: "Bearer tok-123".into(),
    });
    let mut missing = Request::new(HttpMethod::Get, "{{base}}/missing");
    missing.name = "Missing".into();
    missing.assertions.push(Assertion::default());

    let mut c = Collection::new("chain");
    c.items = vec![Item::Request(login), Item::Request(me), Item::Request(missing)];

    let report = run_collection(&c, Some(&mut env), &RunOptions::default(), |_| {});
    assert_eq!(report.requests.len(), 3);
    assert!(report.requests[0].passed());
    assert!(report.requests[1].passed(), "{:?}", report.requests[1].assertions);
    assert!(!report.requests[2].passed());
    assert_eq!(env.get("token"), Some("tok-123"));
    assert_eq!(report.assertion_counts(), (1, 2));
}

#[test]
fn connection_error_is_reported() {
    let req = Request::new(HttpMethod::Get, "http://127.0.0.1:1/");
    let settings = HttpSettings { timeout_secs: 2, ..Default::default() };
    assert!(http::execute(&req, &VarScope::new(), None, &settings).is_err());
}
