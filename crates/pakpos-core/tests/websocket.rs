//! WebSocket client against an in-process echo server.
#![allow(clippy::result_large_err)] // tungstenite callback signature

use pakpos_core::http::prepare;
use pakpos_core::vars::VarScope;
use pakpos_core::ws::{self, Direction, WsEvent};
use pakpos_core::{Auth, HttpMethod, KeyValue, Request};
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn echo_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut seen_auth = String::new();
                let cb = |req: &tungstenite::handshake::server::Request, resp| {
                    seen_auth =
                        req.headers().get("authorization").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
                    Ok(resp)
                };
                let mut ws = tungstenite::accept_hdr(stream, cb).unwrap();
                ws.send(tungstenite::Message::Text(format!("hello {seen_auth}").into())).unwrap();
                while let Ok(msg) = ws.read() {
                    if msg.is_text() {
                        ws.send(tungstenite::Message::Text(format!("echo: {}", msg.to_text().unwrap()).into()))
                            .unwrap();
                    }
                    if msg.is_close() {
                        break;
                    }
                }
            });
        }
    });
    format!("ws://{addr}/socket")
}

fn next(handle: &ws::WsHandle) -> WsEvent {
    handle.rx.recv_timeout(Duration::from_secs(5)).expect("event")
}

#[test]
fn connects_sends_and_receives_with_auth_header() {
    let url = echo_server();
    let mut req = Request::new(HttpMethod::Get, url);
    req.kind = pakpos_core::model::RequestKind::WebSocket;
    req.params.push(KeyValue::new("room", "1"));
    req.auth = Auth::Bearer { token: "t0k".into() };
    let prepared = prepare(&req, &VarScope::new(), None).unwrap();
    let handle = ws::connect(&prepared, || {});
    assert!(matches!(next(&handle), WsEvent::Connected { status: 101, .. }));
    match next(&handle) {
        WsEvent::Message { direction: Direction::Received, text, .. } => assert_eq!(text, "hello Bearer t0k"),
        other => panic!("unexpected {other:?}"),
    }
    handle.send("ping?");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got = Vec::new();
    while got.len() < 2 && Instant::now() < deadline {
        if let WsEvent::Message { direction, text, .. } = next(&handle) {
            got.push((direction, text));
        }
    }
    assert_eq!(got, vec![(Direction::Sent, "ping?".to_string()), (Direction::Received, "echo: ping?".to_string())]);
    handle.close();
    assert!(matches!(next(&handle), WsEvent::Closed { .. }));
}

#[test]
fn reports_connection_errors() {
    let req = Request::new(HttpMethod::Get, "ws://127.0.0.1:1/");
    let handle = ws::connect(&prepare(&req, &VarScope::new(), None).unwrap(), || {});
    assert!(matches!(next(&handle), WsEvent::Error(_)));
    assert!(matches!(next(&handle), WsEvent::Closed { .. }));
}
