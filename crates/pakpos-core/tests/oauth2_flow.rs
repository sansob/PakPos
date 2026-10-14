//! OAuth 2.0 against an in-process authorization server.

use pakpos_core::http::HttpSettings;
use pakpos_core::model::{Assertion, Item, OAuth2Config, OAuth2Grant};
use pakpos_core::oauth2::{self, TokenStore};
use pakpos_core::runner::{run_collection, RunOptions};
use pakpos_core::{Auth, Collection, HttpMethod, Request};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Default)]
struct Seen {
    forms: Vec<HashMap<String, String>>,
    auth_headers: Vec<String>,
}

/// `/token` issues tokens; `/me` requires `Bearer tok-<grant>`.
fn auth_server() -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let seen2 = seen.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let (mut len, mut auth) = (0usize, String::new());
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end().to_string();
                if h.is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    match k.to_ascii_lowercase().as_str() {
                        "content-length" => len = v.trim().parse().unwrap(),
                        "authorization" => auth = v.trim().to_string(),
                        _ => {}
                    }
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let (status, payload) = if path == "/token" {
                let form: HashMap<String, String> = url::form_urlencoded::parse(&body).into_owned().collect();
                let grant = form.get("grant_type").cloned().unwrap_or_default();
                let mut s = seen2.lock().unwrap();
                s.forms.push(form.clone());
                s.auth_headers.push(auth.clone());
                if grant == "authorization_code" && !form.contains_key("code_verifier") {
                    ("400 Bad Request", r#"{"error":"invalid_request"}"#.to_string())
                } else {
                    ("200 OK", format!(r#"{{"access_token":"tok-{grant}","token_type":"bearer","expires_in":3600}}"#))
                }
            } else if auth.starts_with("Bearer tok-") {
                ("200 OK", format!(r#"{{"ok":true,"auth":"{auth}"}}"#))
            } else {
                ("401 Unauthorized", r#"{"ok":false}"#.to_string())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
        }
    });
    (format!("http://{addr}"), seen)
}

#[test]
fn runner_fetches_client_credentials_token_once() {
    let (base, seen) = auth_server();
    let mut c = Collection::new("oauth");
    c.auth = Auth::OAuth2(OAuth2Config {
        grant: OAuth2Grant::ClientCredentials,
        token_url: format!("{base}/token"),
        client_id: "app".into(),
        client_secret: "s3cret".into(),
        scope: "read".into(),
        ..Default::default()
    });
    for name in ["one", "two"] {
        let mut r = Request::new(HttpMethod::Get, format!("{base}/me"));
        r.name = name.into();
        r.assertions.push(Assertion::default());
        c.items.push(Item::Request(r));
    }
    let report = run_collection(&c, None, &RunOptions::default(), |_| {});
    assert!(report.passed(), "{:?}", report.requests);
    let s = seen.lock().unwrap();
    assert_eq!(s.forms.len(), 1, "token must be cached between requests");
    assert_eq!(s.forms[0]["grant_type"], "client_credentials");
    assert_eq!(s.forms[0]["scope"], "read");
    assert!(s.auth_headers[0].starts_with("Basic "));
    assert_eq!(report.tokens.len(), 1);
}

#[test]
fn allow_list_also_covers_token_url() {
    let (base, seen) = auth_server();
    let mut r = Request::new(HttpMethod::Get, "http://allowed.example/me");
    r.auth =
        Auth::OAuth2(OAuth2Config { token_url: format!("{base}/token"), client_id: "a".into(), ..Default::default() });
    let mut c = Collection::new("c");
    c.items.push(Item::Request(r));
    let opts = RunOptions { allowed_hosts: vec!["allowed.example".into()], ..Default::default() };
    let report = run_collection(&c, None, &opts, |_| {});
    assert!(report.requests[0].error.as_deref().unwrap().contains("token URL"));
    assert!(seen.lock().unwrap().forms.is_empty());
}

#[test]
fn authorization_code_with_pkce_end_to_end() {
    let (base, seen) = auth_server();
    let cfg = OAuth2Config {
        grant: OAuth2Grant::AuthorizationCode,
        auth_url: format!("{base}/authorize"),
        token_url: format!("{base}/token"),
        client_id: "spa".into(),
        client_auth: pakpos_core::model::ClientAuthMethod::Body,
        redirect_uri: "http://127.0.0.1:47823/callback".into(),
        ..Default::default()
    };
    let pending = oauth2::begin_authorization(&cfg).unwrap();
    let auth_url = url::Url::parse(&pending.url).unwrap();
    let q: HashMap<String, String> = auth_url.query_pairs().into_owned().collect();
    let state = q["state"].clone();
    assert_eq!(q["code_challenge_method"], "S256");

    // Play the browser: follow the redirect with a code.
    let browser = thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut s = std::net::TcpStream::connect("127.0.0.1:47823").unwrap();
        write!(s, "GET /callback?code=abc&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        let mut page = String::new();
        let _ = s.read_to_string(&mut page);
        page
    });
    let token = pending.finish(&cfg, &HttpSettings::default(), std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(token.access_token, "tok-authorization_code");
    assert!(browser.join().unwrap().contains("Login complete"));
    let s = seen.lock().unwrap();
    let form = &s.forms[0];
    assert_eq!(form["code"], "abc");
    assert_eq!(form["client_id"], "spa");
    // The verifier must hash to the challenge sent to the browser.
    assert_eq!(oauth2::pkce_challenge(&form["code_verifier"]), q["code_challenge"]);

    // A stored token is then used by prepare().
    let mut store = TokenStore::new();
    store.insert(cfg.cache_key(), token);
    let mut scope = pakpos_core::vars::VarScope::new();
    let auth = Auth::OAuth2(cfg);
    oauth2::ensure_token(&auth, &mut scope, &mut store, &HttpSettings::default()).unwrap();
    let mut req = Request::new(HttpMethod::Get, format!("{base}/me"));
    req.auth = auth;
    let p = pakpos_core::http::prepare(&req, &scope, None).unwrap();
    assert_eq!(p.header("authorization"), Some("Bearer tok-authorization_code"));
}

#[test]
fn postman_oauth2_roundtrip() {
    let json = r#"{"info":{"name":"x"},"item":[{"name":"r","request":{"method":"GET","url":"http://h/",
      "auth":{"type":"oauth2","oauth2":[
        {"key":"grant_type","value":"password_credentials"},
        {"key":"accessTokenUrl","value":"{{idp}}/token"},
        {"key":"clientId","value":"cli"},{"key":"username","value":"budi"},{"key":"password","value":"pw"},
        {"key":"client_authentication","value":"body"}]}}}]}"#;
    let c = pakpos_core::postman::import_collection(json).unwrap().value;
    let r = &c.flatten()[0].1;
    let Auth::OAuth2(cfg) = &r.auth else { panic!("expected oauth2, got {:?}", r.auth) };
    assert_eq!(cfg.grant, OAuth2Grant::Password);
    assert_eq!(cfg.token_url, "{{idp}}/token");
    assert_eq!(cfg.client_auth, pakpos_core::model::ClientAuthMethod::Body);
    let back = pakpos_core::postman::import_collection(&pakpos_core::postman::export_collection(&c)).unwrap().value;
    assert_eq!(back.flatten()[0].1.auth, r.auth);
}
