//! Server-Sent Events (`text/event-stream`) client, with the same handle and
//! events as [`crate::ws`]. Receive-only: `Send` commands are refused.

use crate::http::{HttpSettings, PreparedRequest};
use crate::ws::{Direction, WsCommand, WsEvent, WsHandle};
use chrono::Utc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

/// One parsed event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// `event:` field; `message` when absent.
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

/// Incremental parser (WHATWG "event stream interpretation").
#[derive(Debug, Default)]
pub struct Parser {
    buf: String,
    event: String,
    data: Vec<String>,
    id: Option<String>,
    retry: Option<u64>,
}

impl Parser {
    /// Feeds raw bytes, keeping a UTF-8 sequence split across chunks for later.
    pub fn feed_bytes(&mut self, pending: &mut Vec<u8>, bytes: &[u8]) -> Vec<SseEvent> {
        pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(pending) {
            Ok(_) => pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => pending.len(), // really invalid: decode lossily
        };
        let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
        pending.drain(..valid);
        self.feed(&text)
    }

    /// Feeds text; returns the events completed by it.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find(['\n', '\r']) {
            let line: String = self.buf[..pos].to_string();
            // \r\n counts as one line break.
            let skip = if self.buf[pos..].starts_with("\r\n") { 2 } else { 1 };
            self.buf.drain(..pos + skip);
            if line.is_empty() {
                if !self.data.is_empty() || !self.event.is_empty() {
                    out.push(SseEvent {
                        event: if self.event.is_empty() { "message".into() } else { std::mem::take(&mut self.event) },
                        data: self.data.join("\n"),
                        id: self.id.clone(),
                        retry: self.retry,
                    });
                }
                self.data.clear();
                self.event.clear();
                continue;
            }
            if line.starts_with(':') {
                continue; // comment / keep-alive
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => self.event = value.to_string(),
                "data" => self.data.push(value.to_string()),
                "id" if !value.contains('\0') => self.id = Some(value.to_string()),
                "retry" => self.retry = value.parse().ok(),
                _ => {}
            }
        }
        out
    }
}

/// Opens the stream with the prepared request's method, URL, headers and body.
pub fn connect(prepared: &PreparedRequest, settings: &HttpSettings, on_event: impl Fn() + Send + 'static) -> WsHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>();
    let (ev_tx, ev_rx) = mpsc::channel::<WsEvent>();
    let prepared = prepared.clone();
    let settings = settings.clone();
    std::thread::spawn(move || {
        let emit = |e: WsEvent| {
            let _ = ev_tx.send(e);
            on_event();
        };
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return emit(WsEvent::Closed { reason: e.to_string() }),
        };
        rt.block_on(async move {
            let client = match crate::http::streaming_client_for(&settings, &prepared.url) {
                Ok(c) => c,
                Err(e) => return emit(WsEvent::Closed { reason: e.to_string() }),
            };
            let method = reqwest::Method::from_bytes(prepared.method.as_bytes()).unwrap_or(reqwest::Method::GET);
            let mut req = client.request(method, &prepared.url);
            let mut has_accept = false;
            for (k, v) in &prepared.headers {
                has_accept |= k.eq_ignore_ascii_case("accept");
                req = req.header(k, v);
            }
            if !has_accept {
                req = req.header("Accept", "text/event-stream");
            }
            req = req.header("Cache-Control", "no-cache");
            if let crate::http::PreparedBody::Text(t) = &prepared.body {
                req = req.body(t.clone());
            }
            let mut resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    emit(WsEvent::Error(format!("connection failed: {e}")));
                    return emit(WsEvent::Closed { reason: "not connected".into() });
                }
            };
            let status = resp.status().as_u16();
            let headers: Vec<(String, String)> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect();
            let content_type =
                resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            emit(WsEvent::Connected { status, headers });
            if !(200..300).contains(&status) || !content_type.contains("text/event-stream") {
                let body = resp.text().await.unwrap_or_default();
                emit(WsEvent::Error(format!(
                    "expected a 2xx text/event-stream response, got {status} {content_type}{}",
                    if body.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", body.chars().take(300).collect::<String>())
                    }
                )));
                return emit(WsEvent::Closed { reason: "not an event stream".into() });
            }
            let mut parser = Parser::default();
            let mut pending = Vec::new();
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    chunk = resp.chunk() => match chunk {
                        Ok(Some(bytes)) => {
                            for ev in parser.feed_bytes(&mut pending, &bytes) {
                                emit(WsEvent::Message {
                                    direction: Direction::Received,
                                    text: ev.data,
                                    binary: false,
                                    at: Utc::now(),
                                    label: Some(ev.event),
                                });
                            }
                        }
                        Ok(None) => return emit(WsEvent::Closed { reason: "stream ended by the server".into() }),
                        Err(e) => {
                            emit(WsEvent::Error(e.to_string()));
                            return emit(WsEvent::Closed { reason: "error".into() });
                        }
                    },
                    _ = tick.tick() => loop {
                        match cmd_rx.try_recv() {
                            Ok(WsCommand::Close) | Err(TryRecvError::Disconnected) => {
                                return emit(WsEvent::Closed { reason: "closed by client".into() });
                            }
                            Ok(WsCommand::Ping) => {}
                            Ok(_) => emit(WsEvent::Error("Server-Sent Events are receive-only".into())),
                            Err(TryRecvError::Empty) => break,
                        }
                    },
                }
            }
        });
    });
    WsHandle { tx: cmd_tx, rx: ev_rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_across_chunks() {
        let mut p = Parser::default();
        assert!(p.feed(": keep-alive\n\ndata: hel").is_empty());
        let evs = p.feed("lo\ndata: world\n\nevent: price\nid: 7\nretry: 3000\ndata: {\"x\":1}\r\n\r\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], SseEvent { event: "message".into(), data: "hello\nworld".into(), id: None, retry: None });
        assert_eq!(evs[1].event, "price");
        assert_eq!(evs[1].data, "{\"x\":1}");
        assert_eq!(evs[1].id.as_deref(), Some("7"));
        assert_eq!(evs[1].retry, Some(3000));
        // The id sticks to later events; "data" without colon is an empty line of data.
        let evs = p.feed("data\n\n");
        assert_eq!(evs[0].data, "");
        assert_eq!(evs[0].id.as_deref(), Some("7"));
        // "é" split between two chunks.
        let mut pending = Vec::new();
        assert!(p.feed_bytes(&mut pending, b"data: caf\xc3").is_empty());
        let evs = p.feed_bytes(&mut pending, b"\xa9\n\n");
        assert_eq!(evs[0].data, "café");
    }
}
