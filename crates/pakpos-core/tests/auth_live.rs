//! Digest and NTLM handshakes against local servers.

use hmac::KeyInit;
use md5::{Digest, Md5};
use pakpos_core::auth::ntlm;
use pakpos_core::http::{self, HttpSettings};
use pakpos_core::model::{Auth, AwsSigV4Config};
use pakpos_core::vars::VarScope;
use pakpos_core::{HttpMethod, Request};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn read_request(reader: &mut impl BufRead) -> Option<Req> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Some(Req { method, path, headers })
}

fn respond(stream: &mut impl Write, status: &str, extra: &str, body: &str) {
    let resp = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(resp.as_bytes()).unwrap();
}

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn param<'a>(header: &'a str, key: &str) -> &'a str {
    let start = header.find(&format!("{key}=")).map(|i| i + key.len() + 1).unwrap_or(header.len());
    let rest = &header[start..];
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest.find(['"', ',']).unwrap_or(rest.len());
    &rest[..end]
}

/// A Digest server (qop=auth, MD5) for user `mufasa` / `circle of life`.
fn digest_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(req) = read_request(&mut reader) {
                    let ok = req.header("authorization").is_some_and(|a| {
                        let ha1 = md5_hex("mufasa:pakpos:circle of life");
                        let ha2 = md5_hex(&format!("{}:{}", req.method, param(a, "uri")));
                        let expected =
                            md5_hex(&format!("{ha1}:n0nce:{}:{}:auth:{ha2}", param(a, "nc"), param(a, "cnonce")));
                        param(a, "uri") == req.path && param(a, "response") == expected
                    });
                    if ok {
                        respond(&mut stream, "200 OK", "", "welcome");
                    } else {
                        respond(
                            &mut stream,
                            "401 Unauthorized",
                            "WWW-Authenticate: Digest realm=\"pakpos\", qop=\"auth\", nonce=\"n0nce\", opaque=\"op\"\r\n",
                            "",
                        );
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn digest_auth_answers_the_challenge() {
    let base = digest_server();
    let mut req = Request::new(HttpMethod::Get, format!("{base}/secret?x=1"));
    req.auth = Auth::Digest { username: "mufasa".into(), password: "circle of life".into() };
    let (_, resp) = http::execute(&req, &VarScope::new(), None, &HttpSettings::default()).unwrap();
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(resp.text(), "welcome");

    req.auth = Auth::Digest { username: "mufasa".into(), password: "wrong".into() };
    let (_, resp) = http::execute(&req, &VarScope::new(), None, &HttpSettings::default()).unwrap();
    assert_eq!(resp.status, 401);
}

/// An NTLM server that requires the whole handshake on one connection.
fn ntlm_server(connections: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            connections.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let server_challenge = [7u8; 8];
                let mut sent_challenge = false;
                while let Some(req) = read_request(&mut reader) {
                    let auth = req.header("authorization").unwrap_or("").to_string();
                    let Some(data) = auth.strip_prefix("NTLM ") else {
                        respond(&mut stream, "401 Unauthorized", "WWW-Authenticate: NTLM\r\n", "");
                        continue;
                    };
                    let msg = b64.decode(data).unwrap();
                    match msg[8] {
                        1 => {
                            let mut t2 = b"NTLMSSP\0".to_vec();
                            t2.extend_from_slice(&2u32.to_le_bytes());
                            t2.extend_from_slice(&[0u8; 8]);
                            t2.extend_from_slice(&0x0008_8205u32.to_le_bytes());
                            t2.extend_from_slice(&server_challenge);
                            t2.extend_from_slice(&[0u8; 16]);
                            sent_challenge = true;
                            let header = format!("WWW-Authenticate: NTLM {}\r\n", b64.encode(&t2));
                            respond(&mut stream, "401 Unauthorized", &header, "");
                        }
                        3 if sent_challenge => {
                            let field = |at: usize| {
                                let len = u16::from_le_bytes([msg[at], msg[at + 1]]) as usize;
                                let off = u32::from_le_bytes(msg[at + 4..at + 8].try_into().unwrap()) as usize;
                                msg[off..off + len].to_vec()
                            };
                            let nt = field(20);
                            let utf16 = |b: Vec<u8>| {
                                String::from_utf16(
                                    &b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
                                )
                                .unwrap()
                            };
                            let (domain, user) = (utf16(field(28)), utf16(field(36)));
                            let key = ntlm::ntowf_v2("s3cret", &user, &domain);
                            use hmac::Mac;
                            let mut mac = hmac::Hmac::<Md5>::new_from_slice(&key).unwrap();
                            mac.update(&server_challenge);
                            mac.update(&nt[16..]);
                            let ok =
                                user == "budi" && domain == "KANTOR" && mac.finalize().into_bytes()[..] == nt[..16];
                            if ok {
                                respond(&mut stream, "200 OK", "", "hello budi");
                            } else {
                                respond(&mut stream, "401 Unauthorized", "", "bad proof");
                            }
                        }
                        _ => respond(&mut stream, "400 Bad Request", "", "out of order"),
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn ntlm_handshake_on_one_connection() {
    let connections = Arc::new(AtomicUsize::new(0));
    let base = ntlm_server(connections.clone());
    let mut req = Request::new(HttpMethod::Get, format!("{base}/intranet"));
    req.auth = Auth::Ntlm {
        username: "KANTOR\\budi".into(),
        password: "s3cret".into(),
        domain: String::new(),
        workstation: "LAPTOP".into(),
    };
    let (_, resp) = http::execute(&req, &VarScope::new(), None, &HttpSettings::default()).unwrap();
    assert_eq!(resp.status, 200, "{}", resp.text());
    assert_eq!(resp.text(), "hello budi");
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}

#[test]
fn aws_sigv4_headers_are_added() {
    let mut req = Request::new(HttpMethod::Get, "https://s3.amazonaws.com/bucket/key?list-type=2");
    req.auth = Auth::AwsSigV4(AwsSigV4Config {
        access_key: "{{ak}}".into(),
        secret_key: "secret".into(),
        session_token: String::new(),
        region: "us-east-1".into(),
        service: "s3".into(),
    });
    let mut scope = VarScope::new();
    scope.environment.insert("ak".into(), "AKIDEXAMPLE".into());
    let p = http::prepare(&req, &scope, None).unwrap();
    let auth = p.header("authorization").unwrap();
    assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"), "{auth}");
    assert!(auth.contains("/us-east-1/s3/aws4_request"));
    assert!(p.header("x-amz-date").is_some());
    assert_eq!(
        p.header("x-amz-content-sha256"),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}
