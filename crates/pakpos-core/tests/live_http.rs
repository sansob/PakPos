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

#[test]
fn runner_executes_trusted_scripts_only() {
    use pakpos_core::model::Scripts;
    let base = echo_server();
    let mut env = Environment::new("e");
    env.set("base", base);
    let mut c = Collection::new("scripted");
    c.scripts = Scripts { pre_request: "pm.variables.set('tok', 's1');".into(), test: String::new() };
    let mut r = Request::new(HttpMethod::Get, "{{base}}/me");
    r.name = "Me".into();
    r.scripts = Scripts {
        pre_request: "pm.request.headers.add({ key: 'Authorization', value: 'Bearer ' + pm.variables.get('tok') });"
            .into(),
        test: r#"
            pm.test("server saw the header", () => pm.expect(pm.response.json().auth).to.equal("Bearer s1"));
            pm.environment.set("seen", "yes");
            console.log("done");
        "#
        .into(),
    };
    c.items = vec![Item::Request(r)];

    // Untrusted: scripts are skipped and the run says so.
    let report = run_collection(&c, Some(&mut env), &RunOptions::default(), |_| {});
    assert!(report.requests[0].assertions.is_empty());
    assert!(report.requests[0].logs[0].contains("not trusted"));
    assert_eq!(env.get("seen"), None);

    // Trusted.
    let opts = RunOptions { allow_scripts: true, ..Default::default() };
    let report = run_collection(&c, Some(&mut env), &opts, |_| {});
    let r = &report.requests[0];
    assert!(r.passed(), "{:?} {:?}", r.assertions, r.logs);
    assert_eq!(r.assertions.len(), 1);
    assert_eq!(r.logs, vec!["done"]);
    assert_eq!(env.get("seen"), Some("yes"));
}

#[test]
fn data_driven_run_uses_each_row() {
    let base = echo_server();
    let mut c = Collection::new("Data");
    let mut req = Request::new(HttpMethod::Get, format!("{base}/users/{{{{user}}}}"));
    req.name = "Get user".into();
    req.assertions = vec![Assertion {
        enabled: true,
        target: "json.path".into(),
        op: Operator::Equals,
        expected: "/users/{{user}}".into(),
    }];
    c.items.push(Item::Request(req));
    let data = pakpos_core::data::parse("user,city\nbudi,Jakarta\nsiti,Bandung\nagus,Medan\n").unwrap();
    let options = RunOptions { data, ..Default::default() };
    let report = run_collection(&c, None, &options, |_| {});
    assert_eq!(report.iterations, 3);
    let paths: Vec<_> = report.requests.iter().map(|r| r.url.rsplit('/').next().unwrap().to_string()).collect();
    assert_eq!(paths, ["budi", "siti", "agus"]);
    assert!(report.passed(), "{:?}", report.requests);
    assert!(report.to_junit().contains("Get user #3"));

    // Explicit iteration count repeats the rows.
    let options = RunOptions { iterations: 4, ..options };
    let report = run_collection(&c, None, &options, |_| {});
    assert_eq!(report.requests.len(), 4);
    assert!(report.requests[3].url.ends_with("/budi"));
}

/// `/login` answers 302 + Set-Cookie, `/home` and `/me` echo the Cookie header.
fn cookie_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut cookie = String::new();
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    let h = h.trim_end().to_string();
                    if h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.strip_prefix("cookie: ").or_else(|| h.strip_prefix("Cookie: ")) {
                        cookie = v.to_string();
                    }
                }
                let resp = if path == "/login" {
                    "HTTP/1.1 302 Found\r\nSet-Cookie: session=s3cr3t; Path=/; HttpOnly\r\nLocation: /home\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    let body = serde_json::json!({ "cookie": cookie }).to_string();
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                };
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn cookies_from_redirects_are_kept_for_the_next_request() {
    let base = cookie_server();
    let mut c = Collection::new("Cookies");
    let mut login = Request::new(HttpMethod::Post, format!("{base}/login"));
    login.assertions = vec![Assertion {
        enabled: true,
        target: "json.cookie".into(),
        op: Operator::Equals,
        expected: "session=s3cr3t".into(),
    }];
    let mut me = Request::new(HttpMethod::Get, format!("{base}/me"));
    me.headers.push(KeyValue::new("Cookie", "lang=id"));
    me.assertions = vec![Assertion {
        enabled: true,
        target: "json.cookie".into(),
        op: Operator::Equals,
        expected: "lang=id; session=s3cr3t".into(),
    }];
    c.items.push(Item::Request(login));
    c.items.push(Item::Request(me));
    let report = run_collection(&c, None, &RunOptions::default(), |_| {});
    assert!(report.passed(), "{:?}", report.requests);
    assert_eq!(report.cookies.cookies.len(), 1);
    assert!(report.cookies.cookies[0].http_only);
}

#[test]
fn pre_request_script_fetches_a_token_with_send_request() {
    let base = echo_server();
    let mut c = Collection::new("Token");
    let mut req = Request::new(HttpMethod::Get, format!("{base}/api"));
    req.headers.push(KeyValue::new("Authorization", "Bearer {{token}}"));
    req.scripts.pre_request = format!(
        r#"pm.sendRequest({{ url: "{base}/login", method: "POST", body: {{ mode: "raw", raw: "u=budi" }} }}, (err, res) => {{
            pm.environment.set("token", res.json().token);
        }});"#
    );
    req.assertions = vec![Assertion {
        enabled: true,
        target: "json.auth".into(),
        op: Operator::Equals,
        expected: "Bearer tok-123".into(),
    }];
    c.items.push(Item::Request(req));
    let mut env = Environment::new("Local");

    let without =
        run_collection(&c, Some(&mut env.clone()), &RunOptions { allow_scripts: true, ..Default::default() }, |_| {});
    assert!(!without.passed());

    let options = RunOptions { allow_scripts: true, allow_network: true, ..Default::default() };
    let report = run_collection(&c, Some(&mut env), &options, |_| {});
    assert!(report.passed(), "{:?}", report.requests);
    assert_eq!(env.get("token"), Some("tok-123"));
}
