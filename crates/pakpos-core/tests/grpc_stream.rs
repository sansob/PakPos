//! Interactive client / bidirectional streams against a real tonic server.

#[path = "common/chat.rs"]
mod chat;

use pakpos_core::grpc::{self, StreamEvent};
use pakpos_core::http::HttpSettings;
use pakpos_core::ws::Direction;
use std::time::Duration;

fn start() -> (String, prost_reflect::DescriptorPool) {
    let pool = chat::pool();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let server = chat::ChatServer::new(pool.clone());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move { tonic::transport::Server::builder().add_service(server).serve(addr).await.unwrap() });
    });
    for _ in 0..50 {
        if std::net::TcpStream::connect(addr).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (format!("grpc://{addr}"), pool)
}

fn next(h: &grpc::StreamHandle) -> StreamEvent {
    h.rx.recv_timeout(Duration::from_secs(5)).expect("event within 5 s")
}

fn next_message(h: &grpc::StreamHandle, dir: Direction) -> String {
    loop {
        match next(h) {
            StreamEvent::Message { direction, text, .. } if direction == dir => return text,
            StreamEvent::Closed { message, .. } => panic!("closed early: {message}"),
            _ => {}
        }
    }
}

#[test]
fn bidirectional_talk_answers_each_message() {
    let (url, pool) = start();
    let h = grpc::open_stream(&url, &pool, "pakpos.demo.Chat/Talk", &[], &HttpSettings::default(), || {}).unwrap();
    h.send(r#"{"user": "budi", "text": "pagi"}"#);
    assert!(next_message(&h, Direction::Sent).contains("pagi"));
    assert!(next_message(&h, Direction::Received).contains("#1 halo budi: pagi"));
    // Invalid input is reported without closing the stream.
    h.send(r#"{"nope": 1}"#);
    assert!(matches!(next(&h), StreamEvent::Error(_)));
    h.send(r#"{"user": "budi", "text": "lagi"}"#);
    assert!(next_message(&h, Direction::Received).contains("#2"));
    h.end();
    loop {
        if let StreamEvent::Closed { code, .. } = next(&h) {
            assert_eq!(code, 0);
            break;
        }
    }
}

#[test]
fn client_stream_returns_one_reply_after_end() {
    let (url, pool) = start();
    let h = grpc::open_stream(&url, &pool, "pakpos.demo.Chat/Sum", &[], &HttpSettings::default(), || {}).unwrap();
    for v in [5, 10, 27] {
        h.send(format!(r#"{{"value": {v}}}"#));
    }
    h.end();
    let total = next_message(&h, Direction::Received);
    assert!(total.contains("\"sum\": \"42\"") || total.contains("\"sum\": 42"), "{total}");
    assert!(total.contains("\"count\": 3"), "{total}");
}

#[test]
fn cancel_and_unary_methods() {
    let (url, pool) = start();
    let h = grpc::open_stream(&url, &pool, "pakpos.demo.Chat/Talk", &[], &HttpSettings::default(), || {}).unwrap();
    h.cancel();
    loop {
        if let StreamEvent::Closed { code_name, .. } = next(&h) {
            assert_eq!(code_name, "Cancelled");
            break;
        }
    }
    // The one-shot call still works for streams: inputs sent up front.
    let r = grpc::call(
        &url,
        &pool,
        "pakpos.demo.Chat/Sum",
        r#"[{"value": 1}, {"value": 2}]"#,
        &[],
        &HttpSettings::default(),
        10,
    )
    .unwrap();
    assert!(r.is_ok(), "{r:?}");
    assert!(grpc::open_stream(
        "grpc://127.0.0.1:1",
        &pool,
        "pakpos.demo.Chat/Nope",
        &[],
        &HttpSettings::default(),
        || {}
    )
    .is_err());
}
