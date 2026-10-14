//! MCP client over Streamable HTTP against a small fake server.

use pakpos_core::http::HttpSettings;
use pakpos_core::mcp_client::{self, McpClient};
use pakpos_core::model::{Assertion, Item, McpConfig, McpOperation, RequestKind};
use pakpos_core::runner::{run_collection, RunOptions};
use pakpos_core::{Body, Collection, HttpMethod, Request};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

fn server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let mut stream = stream;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let (mut len, mut session, mut auth) = (0usize, String::new(), String::new());
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    let h = h.trim_end().to_string();
                    if h.is_empty() {
                        break;
                    }
                    let (k, v) = h.split_once(':').unwrap();
                    match k.to_ascii_lowercase().as_str() {
                        "content-length" => len = v.trim().parse().unwrap(),
                        "mcp-session-id" => session = v.trim().to_string(),
                        "authorization" => auth = v.trim().to_string(),
                        _ => {}
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let reply = |stream: &mut std::net::TcpStream, status: &str, ctype: &str, extra: &str, body: &str| {
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                };
                if line.starts_with("DELETE") {
                    return reply(&mut stream, "200 OK", "text/plain", "", "");
                }
                let msg: Value = serde_json::from_slice(&body).unwrap();
                let id = msg.get("id").cloned();
                let method = msg["method"].as_str().unwrap_or("");
                if auth != "Bearer s3cret" {
                    return reply(&mut stream, "401 Unauthorized", "text/plain", "", "no token");
                }
                if method != "initialize" && session != "sess-42" {
                    return reply(&mut stream, "400 Bad Request", "text/plain", "", "missing session");
                }
                match method {
                    "initialize" => {
                        let r = json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"9"}}});
                        reply(&mut stream, "200 OK", "application/json", "Mcp-Session-Id: sess-42\r\n", &r.to_string());
                    }
                    "notifications/initialized" => reply(&mut stream, "202 Accepted", "text/plain", "", ""),
                    "tools/list" => {
                        let r = json!({"jsonrpc":"2.0","id":id,"result":{"tools":[{"name":"weather","inputSchema":{"type":"object","properties":{"city":{"type":"string"}}}}]}});
                        let sse = format!(
                            "event: message\ndata: {}\n\nevent: message\ndata: {r}\n\n",
                            json!({"jsonrpc":"2.0","method":"notifications/message","params":{}})
                        );
                        reply(&mut stream, "200 OK", "text/event-stream", "", &sse);
                    }
                    "tools/call" => {
                        let city = msg["params"]["arguments"]["city"].as_str().unwrap_or("?").to_string();
                        let r = if city == "Atlantis" {
                            json!({"jsonrpc":"2.0","id":id,"result":{"isError":true,"content":[{"type":"text","text":"unknown city"}]}})
                        } else {
                            json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":format!("{city}: 31°C")}]}})
                        };
                        reply(&mut stream, "200 OK", "application/json", "", &r.to_string());
                    }
                    _ => {
                        let r = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"nope"}});
                        reply(&mut stream, "200 OK", "application/json", "", &r.to_string());
                    }
                }
            });
        }
    });
    format!("http://{addr}/mcp")
}

#[test]
fn streamable_http_session_sse_and_errors() {
    let url = server();
    let headers = vec![("Authorization".to_string(), "Bearer s3cret".to_string())];
    let mut c = McpClient::connect(&url, &headers, &[], &HttpSettings::default()).unwrap();
    assert_eq!(c.server.name, "fake");
    let tools = c.list("tools").unwrap();
    assert_eq!(tools[0]["name"], "weather");
    let r = c.call_tool("weather", json!({"city": "Jakarta"})).unwrap();
    assert_eq!(r["content"][0]["text"], "Jakarta: 31°C");
    assert!(matches!(c.request("nope/nope", json!({})), Err(mcp_client::McpError::Rpc { code: -32601, .. })));
    assert!(McpClient::connect(&url, &[], &[], &HttpSettings::default()).is_err(), "no token → 401");
}

#[test]
fn mcp_requests_run_with_tests() {
    let url = server();
    let mut c = Collection::new("MCP");
    for (city, expected) in [("Jakarta", "200"), ("Atlantis", "500")] {
        let mut req = Request::new(HttpMethod::Post, url.clone());
        req.kind = RequestKind::Mcp;
        req.headers.push(pakpos_core::KeyValue::new("Authorization", "Bearer {{token}}"));
        req.mcp = Some(McpConfig { operation: McpOperation::CallTool, target: "weather".into(), env: vec![] });
        req.body = Body::Raw { content: format!(r#"{{"city": "{city}"}}"#), language: Default::default() };
        req.assertions = vec![Assertion {
            enabled: true,
            target: "status".into(),
            op: pakpos_core::assertions::Operator::Equals,
            expected: expected.into(),
        }];
        c.items.push(Item::Request(req));
    }
    let options = RunOptions { overrides: vec![("token".into(), "s3cret".into())], ..Default::default() };
    let report = run_collection(&c, None, &options, |_| {});
    assert!(report.passed(), "{:?}", report.requests);
    assert_eq!(report.requests[0].method, "MCP");

    // A stdio server needs trust; an http one the allow-list.
    let mut stdio = Collection::new("stdio");
    let mut req = Request::new(HttpMethod::Post, "echo hi");
    req.kind = RequestKind::Mcp;
    stdio.items.push(Item::Request(req));
    let r = run_collection(&stdio, None, &RunOptions::default(), |_| {});
    assert!(r.requests[0].error.as_deref().unwrap_or("").contains("trust"), "{:?}", r.requests[0]);
    let r = run_collection(&c, None, &RunOptions { allowed_hosts: vec!["example.com".into()], ..options }, |_| {});
    assert!(r.requests[0].error.as_deref().unwrap_or("").contains("allow-list"));
}
