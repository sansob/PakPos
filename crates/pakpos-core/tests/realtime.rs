//! SSE, Socket.IO and MQTT clients against small in-process servers.

use pakpos_core::http::{HttpSettings, PreparedBody, PreparedRequest};
use pakpos_core::ws::{Direction, WsCommand, WsEvent, WsHandle};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn prepared(url: &str) -> PreparedRequest {
    PreparedRequest {
        method: "GET".into(),
        url: url.into(),
        headers: vec![("Authorization".into(), "Bearer t0k".into())],
        body: PreparedBody::None,
        challenge: None,
    }
}

fn next(h: &WsHandle) -> WsEvent {
    h.rx.recv_timeout(Duration::from_secs(5)).expect("event within 5 s")
}

/// Next received message as (label, text), skipping other events.
fn received(h: &WsHandle) -> (Option<String>, String) {
    loop {
        match next(h) {
            WsEvent::Message { direction: Direction::Received, label, text, .. } => return (label, text),
            WsEvent::Closed { reason } => panic!("closed early: {reason}"),
            _ => {}
        }
    }
}

// ---- SSE ----------------------------------------------------------------------------

#[test]
fn sse_events_with_types_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(s.try_clone().unwrap());
        let mut auth = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            if line.to_ascii_lowercase().starts_with("authorization:") {
                auth = line.split_once(':').unwrap().1.trim().to_string();
            }
        }
        write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
        write!(s, ": hi\n\ndata: {auth}\n\n").unwrap();
        s.flush().unwrap();
        thread::sleep(Duration::from_millis(100));
        write!(s, "event: price\ndata: 1\ndata: 2\n\n").unwrap();
    });
    let h = pakpos_core::sse::connect(&prepared(&format!("http://{addr}/events")), &HttpSettings::default(), || {});
    assert!(matches!(next(&h), WsEvent::Connected { status: 200, .. }));
    assert_eq!(received(&h), (Some("message".into()), "Bearer t0k".into()));
    assert_eq!(received(&h), (Some("price".into()), "1\n2".into()));
    loop {
        if let WsEvent::Closed { reason } = next(&h) {
            assert!(reason.contains("ended"));
            break;
        }
    }
}

// ---- Socket.IO --------------------------------------------------------------------

#[test]
#[allow(clippy::result_large_err)] // tungstenite's handshake callback signature
fn socketio_namespace_events_and_ping() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut path = String::new();
        let mut ws = tungstenite::accept_hdr(stream, |req: &tungstenite::handshake::server::Request, resp| {
            path = req.uri().to_string();
            Ok(resp)
        })
        .unwrap();
        assert!(path.starts_with("/socket.io/?EIO=4&transport=websocket"), "{path}");
        let send = |ws: &mut tungstenite::WebSocket<TcpStream>, t: &str| {
            ws.send(tungstenite::Message::Text(t.into())).unwrap()
        };
        send(&mut ws, r#"0{"sid":"e1","pingInterval":25000,"pingTimeout":20000}"#);
        let mut pong = false;
        loop {
            let msg = match ws.read() {
                Ok(tungstenite::Message::Text(t)) => t.to_string(),
                _ => break,
            };
            if msg == r#"40/admin,{"token":"x"}"# {
                send(&mut ws, r#"40/admin,{"sid":"s1"}"#);
                send(&mut ws, "2"); // Engine.IO ping
                send(&mut ws, r#"42["ignored","other namespace"]"#);
                send(&mut ws, r#"42/admin,["hello",{"n":1},"two"]"#);
            } else if msg == "3" {
                pong = true;
            } else if let Some(rest) = msg.strip_prefix("42/admin,") {
                assert!(pong, "the client answers pings");
                send(&mut ws, &format!("42/admin,[\"echo\",{rest}]"));
            } else if msg == "41/admin," {
                break;
            }
        }
    });
    let h = pakpos_core::socketio::connect(&prepared(&format!("http://{addr}/admin")), "", r#"{"token":"x"}"#, || {});
    assert!(matches!(next(&h), WsEvent::Connected { .. }));
    assert_eq!(received(&h), (Some("hello".into()), r#"[{"n":1},"two"]"#.into()));
    let _ = h.tx.send(WsCommand::Emit { event: "chat".into(), data: r#"{"text":"halo"}"#.into() });
    assert_eq!(received(&h).0.as_deref(), Some("echo"));
    h.close();
    loop {
        if let WsEvent::Closed { .. } = next(&h) {
            break;
        }
    }
}

// ---- MQTT -----------------------------------------------------------------------------

fn read_packet(s: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut first = [0u8; 1];
    s.read_exact(&mut first).ok()?;
    let (mut len, mut mult) = (0usize, 1usize);
    loop {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).ok()?;
        len += (b[0] & 0x7f) as usize * mult;
        mult *= 128;
        if b[0] & 0x80 == 0 {
            break;
        }
    }
    let mut body = vec![0; len];
    s.read_exact(&mut body).ok()?;
    Some((first[0], body))
}

/// One-client broker: checks CONNECT credentials, echoes PUBLISH back on the
/// same topic if subscribed, acks QoS 1 and 2.
fn broker() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut filters: Vec<String> = Vec::new();
        while let Some((first, body)) = read_packet(&mut s) {
            match first >> 4 {
                1 => {
                    let ok = body.windows(4).any(|w| w == b"budi") && body.windows(7).any(|w| w == b"rahasia");
                    s.write_all(&[0x20, 2, 0, if ok { 0 } else { 5 }]).unwrap();
                }
                8 => {
                    let tlen = u16::from_be_bytes([body[2], body[3]]) as usize;
                    filters.push(String::from_utf8(body[4..4 + tlen].to_vec()).unwrap());
                    s.write_all(&[0x90, 3, body[0], body[1], body[4 + tlen].min(1)]).unwrap();
                }
                3 => {
                    let qos = (first >> 1) & 3;
                    let tlen = u16::from_be_bytes([body[0], body[1]]) as usize;
                    let topic = String::from_utf8(body[2..2 + tlen].to_vec()).unwrap();
                    let mut at = 2 + tlen;
                    if qos > 0 {
                        s.write_all(&[if qos == 1 { 0x40 } else { 0x50 }, 2, body[at], body[at + 1]]).unwrap();
                        at += 2;
                    }
                    if filters.iter().any(|f| f == "#" || f == &topic) {
                        let mut out = vec![(tlen >> 8) as u8, tlen as u8];
                        out.extend_from_slice(topic.as_bytes());
                        out.extend_from_slice(&body[at..]);
                        let mut pkt = vec![0x30, out.len() as u8];
                        pkt.extend(out);
                        s.write_all(&pkt).unwrap();
                    }
                }
                6 => s.write_all(&[0x70, 2, body[0], body[1]]).unwrap(),
                12 => s.write_all(&[0xD0, 0]).unwrap(),
                14 => break,
                _ => {}
            }
        }
    });
    format!("mqtt://{addr}")
}

fn options(url: String, password: &str) -> pakpos_core::mqtt::MqttOptions {
    pakpos_core::mqtt::MqttOptions {
        url,
        client_id: "test".into(),
        username: Some("budi".into()),
        password: Some(password.into()),
        keep_alive_secs: 30,
        clean_session: true,
        subscriptions: vec![("#".into(), 1)],
        settings: HttpSettings::default(),
    }
}

#[test]
fn mqtt_subscribe_publish_qos() {
    let h = pakpos_core::mqtt::connect(options(broker(), "rahasia"), || {});
    assert!(matches!(next(&h), WsEvent::Connected { .. }));
    loop {
        if let WsEvent::Info(t) = next(&h) {
            assert!(t.contains("subscribed to #"), "{t}");
            break;
        }
    }
    for qos in [0u8, 1, 2] {
        let _ = h.tx.send(WsCommand::Publish {
            topic: "kantor/suhu".into(),
            payload: format!("q{qos}"),
            qos,
            retain: false,
        });
        assert_eq!(received(&h), (Some("kantor/suhu".into()), format!("q{qos}")));
    }
    h.close();
}

#[test]
fn mqtt_bad_credentials_are_reported() {
    let h = pakpos_core::mqtt::connect(options(broker(), "salah"), || {});
    match next(&h) {
        WsEvent::Error(e) => assert!(e.contains("not authorized"), "{e}"),
        other => panic!("{other:?}"),
    }
}
