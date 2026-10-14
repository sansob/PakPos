//! Socket.IO client (protocol v5 over Engine.IO v4, WebSocket transport),
//! with the same handle and events as [`crate::ws`].
//!
//! `http://host:3000/admin` connects to the namespace `/admin` through
//! `ws://host:3000/socket.io/?EIO=4&transport=websocket`, like the official
//! client. Events arrive as messages labelled with their name; `Emit` sends
//! one. Binary attachments and acknowledgements are not supported.

use crate::http::PreparedRequest;
use crate::ws::{self, Direction, WsCommand, WsEvent, WsHandle};
use chrono::Utc;
use std::sync::mpsc::{self, TryRecvError};
use tungstenite::Message;

/// Engine.IO URL and namespace for a Socket.IO URL.
pub fn endpoint(url: &str, path: &str) -> Result<(String, String), String> {
    let mut u = url::Url::parse(&ws::to_ws_url(url)).map_err(|e| format!("invalid URL `{url}`: {e}"))?;
    let namespace = match u.path() {
        "" | "/" => "/".to_string(),
        p => p.trim_end_matches('/').to_string(),
    };
    let path = if path.trim().is_empty() { "/socket.io/" } else { path.trim() };
    let path = format!("/{}/", path.trim_matches('/'));
    let query: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    u.set_path(&path);
    u.set_query(None);
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in &query {
            q.append_pair(k, v);
        }
        q.append_pair("EIO", "4");
        q.append_pair("transport", "websocket");
    }
    Ok((u.to_string(), namespace))
}

/// A Socket.IO packet inside an Engine.IO `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Connect { namespace: String, data: Option<String> },
    Disconnect { namespace: String },
    Event { namespace: String, id: Option<u64>, name: String, args: Vec<serde_json::Value> },
    Ack { namespace: String, id: u64, args: Vec<serde_json::Value> },
    ConnectError { namespace: String, data: String },
    Other(String),
}

/// Parses the Socket.IO part (after the Engine.IO `4`).
pub fn parse_packet(s: &str) -> Packet {
    let mut chars = s.chars();
    let kind = chars.next();
    let mut rest = chars.as_str();
    // Binary packets carry "<n>-" before the namespace.
    if matches!(kind, Some('5' | '6')) {
        return Packet::Other(s.to_string());
    }
    let namespace = if rest.starts_with('/') {
        let end = rest.find(',').unwrap_or(rest.len());
        let ns = rest[..end].to_string();
        rest = rest.get(end + 1..).unwrap_or("");
        ns
    } else {
        "/".to_string()
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let id = digits.parse::<u64>().ok();
    let payload = &rest[digits.len()..];
    let args = || serde_json::from_str::<Vec<serde_json::Value>>(payload).unwrap_or_default();
    match kind {
        Some('0') => Packet::Connect { namespace, data: (!payload.is_empty()).then(|| payload.to_string()) },
        Some('1') => Packet::Disconnect { namespace },
        Some('2') => {
            let mut a = args();
            if a.is_empty() {
                return Packet::Other(s.to_string());
            }
            let name = match a.remove(0) {
                serde_json::Value::String(n) => n,
                other => other.to_string(),
            };
            Packet::Event { namespace, id, name, args: a }
        }
        Some('3') => Packet::Ack { namespace, id: id.unwrap_or(0), args: args() },
        Some('4') => Packet::ConnectError { namespace, data: payload.to_string() },
        _ => Packet::Other(s.to_string()),
    }
}

/// The whole Engine.IO text frame for an event: `4` (message) + `2` (event),
/// `42/ns,["event",data]` (namespace prefix only when not `/`).
pub fn event_packet(namespace: &str, event: &str, data: &str) -> String {
    let data = data.trim();
    let arg = if data.is_empty() {
        None
    } else {
        Some(serde_json::from_str::<serde_json::Value>(data).unwrap_or_else(|_| serde_json::Value::String(data.into())))
    };
    let mut array = vec![serde_json::Value::String(event.to_string())];
    array.extend(arg);
    format!("42{}{}", ns_prefix(namespace), serde_json::Value::Array(array))
}

fn ns_prefix(namespace: &str) -> String {
    if namespace == "/" {
        String::new()
    } else {
        format!("{namespace},")
    }
}

/// Shows an event's arguments: one argument as itself, several as an array.
fn render_args(args: &[serde_json::Value]) -> String {
    match args {
        [] => String::new(),
        [one] => match one {
            serde_json::Value::String(s) => s.clone(),
            v => v.to_string(),
        },
        many => serde_json::Value::Array(many.to_vec()).to_string(),
    }
}

/// Connects. `auth` is the optional JSON sent with the namespace connect
/// (`io(url, { auth })`). `path` is the server path, default `/socket.io/`.
pub fn connect(prepared: &PreparedRequest, path: &str, auth: &str, on_event: impl Fn() + Send + 'static) -> WsHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>();
    let (ev_tx, ev_rx) = mpsc::channel::<WsEvent>();
    let target = endpoint(&prepared.url, path);
    let headers = prepared.headers.clone();
    let auth = auth.trim().to_string();
    std::thread::spawn(move || {
        let emit = |e: WsEvent| {
            let _ = ev_tx.send(e);
            on_event();
        };
        let (url, namespace) = match target {
            Ok(t) => t,
            Err(e) => {
                emit(WsEvent::Error(e));
                return emit(WsEvent::Closed { reason: "not connected".into() });
            }
        };
        let (mut socket, response) = match ws::open_socket(&url, &headers) {
            Ok(x) => x,
            Err(e) => {
                emit(WsEvent::Error(e));
                return emit(WsEvent::Closed { reason: "not connected".into() });
            }
        };
        let status = response.status().as_u16();
        let upgrade_headers = ws::response_headers(&response);
        let mut joined = false;
        let send = |socket: &mut ws::Socket, text: String| socket.send(Message::Text(text.into()));
        loop {
            if joined {
                loop {
                    match cmd_rx.try_recv() {
                        Ok(WsCommand::Emit { event, data }) => {
                            let packet = event_packet(&namespace, &event, &data);
                            match send(&mut socket, packet) {
                                Ok(()) => emit(WsEvent::Message {
                                    direction: Direction::Sent,
                                    text: data,
                                    binary: false,
                                    at: Utc::now(),
                                    label: Some(event),
                                }),
                                Err(e) => emit(WsEvent::Error(format!("send failed: {e}"))),
                            }
                        }
                        Ok(WsCommand::Send(text)) => {
                            // Raw packet, for anything the composer does not cover.
                            if let Err(e) = send(&mut socket, text.clone()) {
                                emit(WsEvent::Error(format!("send failed: {e}")));
                            } else {
                                emit(WsEvent::Message {
                                    direction: Direction::Sent,
                                    text,
                                    binary: false,
                                    at: Utc::now(),
                                    label: Some("raw".into()),
                                });
                            }
                        }
                        Ok(WsCommand::Close) | Err(TryRecvError::Disconnected) => {
                            let _ = send(&mut socket, format!("41{}", ns_prefix(&namespace)));
                            let _ = socket.close(None);
                            let _ = socket.flush();
                            return emit(WsEvent::Closed { reason: "disconnected by client".into() });
                        }
                        Ok(_) => emit(WsEvent::Error("not supported over Socket.IO".into())),
                        Err(TryRecvError::Empty) => break,
                    }
                }
            } else if matches!(cmd_rx.try_recv(), Ok(WsCommand::Close) | Err(TryRecvError::Disconnected)) {
                let _ = socket.close(None);
                return emit(WsEvent::Closed { reason: "closed by client".into() });
            }
            let text = match socket.read() {
                Ok(Message::Text(t)) => t.to_string(),
                Ok(Message::Close(_)) => return emit(WsEvent::Closed { reason: "closed by server".into() }),
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
                {
                    continue
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return emit(WsEvent::Closed { reason: "connection closed".into() });
                }
                Err(e) => {
                    emit(WsEvent::Error(e.to_string()));
                    return emit(WsEvent::Closed { reason: "error".into() });
                }
            };
            // Engine.IO packet types: 0 open, 1 close, 2 ping, 3 pong, 4 message.
            match text.chars().next() {
                Some('0') => {
                    let connect = if auth.is_empty() {
                        format!("40{}", ns_prefix(&namespace))
                    } else {
                        format!("40{}{auth}", ns_prefix(&namespace))
                    };
                    if let Err(e) = send(&mut socket, connect) {
                        emit(WsEvent::Error(e.to_string()));
                        return emit(WsEvent::Closed { reason: "error".into() });
                    }
                }
                Some('1') => return emit(WsEvent::Closed { reason: "closed by server".into() }),
                Some('2') => {
                    let _ = send(&mut socket, format!("3{}", &text[1..]));
                }
                Some('4') => match parse_packet(&text[1..]) {
                    Packet::Connect { namespace: ns, .. } if ns == namespace => {
                        joined = true;
                        emit(WsEvent::Connected { status, headers: upgrade_headers.clone() });
                    }
                    Packet::ConnectError { data, .. } => {
                        emit(WsEvent::Error(format!("connect refused: {data}")));
                        let _ = socket.close(None);
                        return emit(WsEvent::Closed { reason: "connect refused".into() });
                    }
                    Packet::Disconnect { namespace: ns } if ns == namespace => {
                        let _ = socket.close(None);
                        return emit(WsEvent::Closed { reason: "disconnected by server".into() });
                    }
                    Packet::Event { namespace: ns, name, args, .. } if ns == namespace => emit(WsEvent::Message {
                        direction: Direction::Received,
                        text: render_args(&args),
                        binary: false,
                        at: Utc::now(),
                        label: Some(name),
                    }),
                    Packet::Ack { id, args, .. } => emit(WsEvent::Message {
                        direction: Direction::Received,
                        text: render_args(&args),
                        binary: false,
                        at: Utc::now(),
                        label: Some(format!("ack {id}")),
                    }),
                    Packet::Other(raw) => emit(WsEvent::Message {
                        direction: Direction::Received,
                        text: raw,
                        binary: false,
                        at: Utc::now(),
                        label: Some("raw".into()),
                    }),
                    _ => {}
                },
                _ => {}
            }
        }
    });
    WsHandle { tx: cmd_tx, rx: ev_rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_namespaces() {
        assert_eq!(
            endpoint("http://localhost:3000", "").unwrap(),
            ("ws://localhost:3000/socket.io/?EIO=4&transport=websocket".into(), "/".into())
        );
        assert_eq!(
            endpoint("https://api.test/admin?token=1", "/ws").unwrap(),
            ("wss://api.test/ws/?token=1&EIO=4&transport=websocket".into(), "/admin".into())
        );
    }

    #[test]
    fn packets() {
        assert_eq!(
            parse_packet(r#"2["chat",{"text":"hi"}]"#),
            Packet::Event {
                namespace: "/".into(),
                id: None,
                name: "chat".into(),
                args: vec![serde_json::json!({"text":"hi"})]
            }
        );
        assert_eq!(
            parse_packet(r#"2/admin,12["tick",1,2]"#),
            Packet::Event {
                namespace: "/admin".into(),
                id: Some(12),
                name: "tick".into(),
                args: vec![1.into(), 2.into()]
            }
        );
        assert_eq!(
            parse_packet(r#"0{"sid":"x"}"#),
            Packet::Connect { namespace: "/".into(), data: Some(r#"{"sid":"x"}"#.into()) }
        );
        assert_eq!(parse_packet("1/admin,"), Packet::Disconnect { namespace: "/admin".into() });
        assert!(matches!(parse_packet(r#"4{"message":"no"}"#), Packet::ConnectError { .. }));
        assert_eq!(event_packet("/", "chat", r#"{"a":1}"#), r#"42["chat",{"a":1}]"#);
        assert_eq!(event_packet("/admin", "ping", ""), r#"42/admin,["ping"]"#);
        assert_eq!(event_packet("/", "say", "halo"), r#"42["say","halo"]"#);
    }
}
