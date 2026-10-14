//! WebSocket client running on a background thread, driven over channels so
//! both the GUI and the CLI can use it without an async runtime.

use crate::http::PreparedRequest;
use chrono::{DateTime, Utc};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    Connected { status: u16, headers: Vec<(String, String)> },
    Message { direction: Direction, text: String, binary: bool, at: DateTime<Utc> },
    Error(String),
    Closed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsCommand {
    Send(String),
    Ping,
    Close,
}

/// Handle to a running connection.
pub struct WsHandle {
    pub tx: Sender<WsCommand>,
    pub rx: Receiver<WsEvent>,
}

impl WsHandle {
    pub fn send(&self, text: impl Into<String>) {
        let _ = self.tx.send(WsCommand::Send(text.into()));
    }

    pub fn close(&self) {
        let _ = self.tx.send(WsCommand::Close);
    }
}

/// `http(s)://` → `ws(s)://`; other schemes unchanged.
pub fn to_ws_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_string()
    }
}

fn set_timeout(ws: &WebSocket<MaybeTlsStream<TcpStream>>, t: Option<Duration>) {
    match ws.get_ref() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(t);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.sock.set_read_timeout(t);
        }
        _ => {}
    }
}

/// Connects using the URL and headers of a prepared request (auth already applied).
/// `on_event` is called after each event is queued (e.g. to request a repaint).
pub fn connect(prepared: &PreparedRequest, on_event: impl Fn() + Send + 'static) -> WsHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>();
    let (ev_tx, ev_rx) = mpsc::channel::<WsEvent>();
    let url = to_ws_url(&prepared.url);
    let headers = prepared.headers.clone();
    std::thread::spawn(move || {
        let emit = |e: WsEvent| {
            let _ = ev_tx.send(e);
            on_event();
        };
        let mut request = match url.as_str().into_client_request() {
            Ok(r) => r,
            Err(e) => return emit(WsEvent::Error(format!("invalid WebSocket URL `{url}`: {e}"))),
        };
        for (k, v) in &headers {
            // Content-Type etc. from the editor make no sense on an upgrade.
            if k.eq_ignore_ascii_case("content-type") {
                continue;
            }
            if let (Ok(name), Ok(value)) =
                (tungstenite::http::HeaderName::from_bytes(k.as_bytes()), tungstenite::http::HeaderValue::from_str(v))
            {
                request.headers_mut().append(name, value);
            }
        }
        let (mut ws, response) = match tungstenite::connect(request) {
            Ok(x) => x,
            Err(e) => {
                emit(WsEvent::Error(format!("connection failed: {e}")));
                return emit(WsEvent::Closed { reason: "not connected".into() });
            }
        };
        emit(WsEvent::Connected {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect(),
        });
        set_timeout(&ws, Some(Duration::from_millis(50)));
        loop {
            // Outgoing first, so typing never waits for the network.
            loop {
                match cmd_rx.try_recv() {
                    Ok(WsCommand::Send(text)) => {
                        if let Err(e) = ws.send(Message::Text(text.clone().into())) {
                            emit(WsEvent::Error(format!("send failed: {e}")));
                        } else {
                            emit(WsEvent::Message { direction: Direction::Sent, text, binary: false, at: Utc::now() });
                        }
                    }
                    Ok(WsCommand::Ping) => {
                        let _ = ws.send(Message::Ping(Vec::new().into()));
                    }
                    Ok(WsCommand::Close) | Err(TryRecvError::Disconnected) => {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        return emit(WsEvent::Closed { reason: "closed by client".into() });
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            match ws.read() {
                Ok(Message::Text(t)) => emit(WsEvent::Message {
                    direction: Direction::Received,
                    text: t.to_string(),
                    binary: false,
                    at: Utc::now(),
                }),
                Ok(Message::Binary(b)) => emit(WsEvent::Message {
                    direction: Direction::Received,
                    text: match std::str::from_utf8(&b) {
                        Ok(s) => s.to_string(),
                        Err(_) => format!("<{} bytes binary>", b.len()),
                    },
                    binary: true,
                    at: Utc::now(),
                }),
                Ok(Message::Close(frame)) => {
                    let reason = frame
                        .map(|f| format!("{} {}", u16::from(f.code), f.reason))
                        .unwrap_or_else(|| "closed by server".into());
                    return emit(WsEvent::Closed { reason });
                }
                Ok(_) => {} // ping/pong handled by tungstenite
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return emit(WsEvent::Closed { reason: "connection closed".into() });
                }
                Err(e) => {
                    emit(WsEvent::Error(e.to_string()));
                    return emit(WsEvent::Closed { reason: "error".into() });
                }
            }
        }
    });
    WsHandle { tx: cmd_tx, rx: ev_rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_conversion() {
        assert_eq!(to_ws_url("https://x/y"), "wss://x/y");
        assert_eq!(to_ws_url("http://x"), "ws://x");
        assert_eq!(to_ws_url("ws://x"), "ws://x");
    }
}
