//! MQTT 3.1.1 client (TCP or TLS), with the same handle and events as
//! [`crate::ws`]: `Publish`, `Subscribe` and `Unsubscribe` commands;
//! received messages are labelled with their topic. QoS 0, 1 and 2.

use crate::http::HttpSettings;
use crate::ws::{Direction, WsCommand, WsEvent, WsHandle};
use chrono::Utc;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

/// Connection options.
#[derive(Debug, Clone, Default)]
pub struct MqttOptions {
    /// `mqtt://host[:1883]` or `mqtts://host[:8883]` (also `tcp://`, `ssl://`, `tls://`).
    pub url: String,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub keep_alive_secs: u16,
    pub clean_session: bool,
    /// Topic filters subscribed right after connecting.
    pub subscriptions: Vec<(String, u8)>,
    pub settings: HttpSettings,
}

// ---- packets -------------------------------------------------------------------

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn packet(first: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![first];
    let mut len = body.len();
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    out
}

pub fn connect_packet(o: &MqttOptions) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(&mut body, "MQTT");
    body.push(4); // protocol level 3.1.1
    let mut flags = 0u8;
    if o.clean_session {
        flags |= 0x02;
    }
    if o.password.is_some() {
        flags |= 0x40;
    }
    if o.username.is_some() {
        flags |= 0x80;
    }
    body.push(flags);
    body.extend_from_slice(&o.keep_alive_secs.to_be_bytes());
    put_str(&mut body, &o.client_id);
    if let Some(u) = &o.username {
        put_str(&mut body, u);
    }
    if let Some(p) = &o.password {
        put_str(&mut body, p);
    }
    packet(0x10, &body)
}

pub fn publish_packet(topic: &str, payload: &[u8], qos: u8, retain: bool, id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    put_str(&mut body, topic);
    if qos > 0 {
        body.extend_from_slice(&id.to_be_bytes());
    }
    body.extend_from_slice(payload);
    packet(0x30 | (qos.min(2) << 1) | u8::from(retain), &body)
}

pub fn subscribe_packet(id: u16, topics: &[(String, u8)]) -> Vec<u8> {
    let mut body = id.to_be_bytes().to_vec();
    for (t, q) in topics {
        put_str(&mut body, t);
        body.push((*q).min(2));
    }
    packet(0x82, &body)
}

fn unsubscribe_packet(id: u16, topic: &str) -> Vec<u8> {
    let mut body = id.to_be_bytes().to_vec();
    put_str(&mut body, topic);
    packet(0xA2, &body)
}

fn ack(kind: u8, id: u16) -> Vec<u8> {
    packet(kind, &id.to_be_bytes())
}

/// A decoded packet: (first byte, body).
pub fn decode(buf: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    if buf.len() < 2 {
        return None;
    }
    let (mut len, mut mult, mut i) = (0usize, 1usize, 1usize);
    loop {
        let b = *buf.get(i)?;
        len += (b & 0x7f) as usize * mult;
        i += 1;
        if b & 0x80 == 0 {
            break;
        }
        mult *= 128;
        if i > 4 {
            buf.clear();
            return None;
        }
    }
    if buf.len() < i + len {
        return None;
    }
    let first = buf[0];
    let body = buf[i..i + len].to_vec();
    buf.drain(..i + len);
    Some((first, body))
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map(|x| u16::from_be_bytes([x[0], x[1]])).unwrap_or(0)
}

/// (topic, payload, qos, retain, packet id) of a PUBLISH body.
pub fn parse_publish(first: u8, body: &[u8]) -> Option<(String, Vec<u8>, u8, bool, u16)> {
    let qos = (first >> 1) & 0x03;
    let tlen = u16_at(body, 0) as usize;
    let topic = String::from_utf8_lossy(body.get(2..2 + tlen)?).into_owned();
    let mut at = 2 + tlen;
    let id = if qos > 0 {
        let id = u16_at(body, at);
        at += 2;
        id
    } else {
        0
    };
    Some((topic, body.get(at..)?.to_vec(), qos, first & 1 == 1, id))
}

fn connack_error(rc: u8) -> &'static str {
    match rc {
        1 => "unacceptable protocol version",
        2 => "client identifier rejected",
        3 => "server unavailable",
        4 => "bad user name or password",
        5 => "not authorized",
        _ => "refused",
    }
}

// ---- transport -----------------------------------------------------------------

enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Conn {
    fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s,
            Conn::Tls(s) => &s.sock,
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

/// Accepts any server certificate (the "verify TLS" switch is off).
#[derive(Debug)]
struct NoVerify(std::sync::Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_config(settings: &HttpSettings, url: &str) -> Result<rustls::ClientConfig, String> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    let entry = settings.client_cert_for(&url.replacen("mqtts://", "https://", 1));
    if let Some(ca) = entry.map(|e| e.ca_pem()).transpose().map_err(|e| e.to_string())?.flatten() {
        for c in CertificateDer::pem_slice_iter(&ca).flatten() {
            roots.add(c).map_err(|e| e.to_string())?;
        }
    }
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;
    let builder = if settings.verify_tls {
        builder.with_root_certificates(roots)
    } else {
        builder.dangerous().with_custom_certificate_verifier(std::sync::Arc::new(NoVerify(provider)))
    };
    match entry.filter(|e| !e.cert_path.trim().is_empty()) {
        Some(e) => {
            let pem = e.pem().map_err(|e| e.to_string())?;
            let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&pem).flatten().collect();
            let key = PrivateKeyDer::from_pem_slice(&pem).map_err(|e| format!("client key: {e}"))?;
            builder.with_client_auth_cert(chain, key).map_err(|e| e.to_string())
        }
        None => Ok(builder.with_no_client_auth()),
    }
}

/// (host, port, tls) from an MQTT URL.
pub fn parse_url(url: &str) -> Result<(String, u16, bool), String> {
    let url = url.trim();
    let (tls, rest) = if let Some(r) =
        url.strip_prefix("mqtts://").or_else(|| url.strip_prefix("ssl://")).or_else(|| url.strip_prefix("tls://"))
    {
        (true, r)
    } else if let Some(r) = url.strip_prefix("mqtt://").or_else(|| url.strip_prefix("tcp://")) {
        (false, r)
    } else if url.contains("://") {
        return Err(format!("unsupported MQTT URL `{url}` (use mqtt:// or mqtts://)"));
    } else {
        (false, url)
    };
    let hostport = rest.split('/').next().unwrap_or("");
    let parsed = url::Url::parse(&format!("x://{hostport}")).map_err(|e| format!("invalid MQTT URL `{url}`: {e}"))?;
    let host = parsed.host_str().filter(|h| !h.is_empty()).ok_or_else(|| format!("no host in `{url}`"))?;
    Ok((host.trim_matches(['[', ']']).to_string(), parsed.port().unwrap_or(if tls { 8883 } else { 1883 }), tls))
}

fn open(o: &MqttOptions) -> Result<Conn, String> {
    let (host, port, tls) = parse_url(&o.url)?;
    let timeout = Duration::from_secs(o.settings.timeout_secs.max(1));
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("cannot resolve {host}"))?;
    let tcp =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("cannot connect to {host}:{port}: {e}"))?;
    let _ = tcp.set_nodelay(true);
    if !tls {
        return Ok(Conn::Plain(tcp));
    }
    let config = tls_config(&o.settings, &o.url)?;
    let name = rustls::pki_types::ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(std::sync::Arc::new(config), name).map_err(|e| e.to_string())?;
    Ok(Conn::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))))
}

/// Connects to a broker on a background thread.
pub fn connect(options: MqttOptions, on_event: impl Fn() + Send + 'static) -> WsHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WsCommand>();
    let (ev_tx, ev_rx) = mpsc::channel::<WsEvent>();
    std::thread::spawn(move || {
        let emit = |e: WsEvent| {
            let _ = ev_tx.send(e);
            on_event();
        };
        let fail = |e: String| {
            emit(WsEvent::Error(e));
            emit(WsEvent::Closed { reason: "not connected".into() });
        };
        let mut conn = match open(&options) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let _ = conn.tcp().set_read_timeout(Some(Duration::from_secs(options.settings.timeout_secs.max(1))));
        let mut o = options.clone();
        if o.client_id.trim().is_empty() {
            o.client_id = format!("pakpos-{}", &crate::model::new_id()[..8]);
        }
        if let Err(e) = conn.write_all(&connect_packet(&o)).and_then(|_| conn.flush()) {
            return fail(e.to_string());
        }
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        // CONNACK
        let connack = loop {
            if let Some(p) = decode(&mut buf) {
                break p;
            }
            match conn.read(&mut chunk) {
                Ok(0) => return fail("the broker closed the connection".into()),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) => return fail(format!("no CONNACK: {e}")),
            }
        };
        if connack.0 >> 4 != 2 || connack.1.get(1).copied().unwrap_or(0xff) != 0 {
            let rc = connack.1.get(1).copied().unwrap_or(0);
            return fail(format!("connection refused: {} (code {rc})", connack_error(rc)));
        }
        emit(WsEvent::Connected { status: 0, headers: vec![("client-id".into(), o.client_id.clone())] });
        let _ = conn.tcp().set_read_timeout(Some(Duration::from_millis(50)));

        let mut next_id: u16 = 0;
        let mut id = || {
            next_id = next_id.wrapping_add(1).max(1);
            next_id
        };
        let mut pending_subs: std::collections::HashMap<u16, Vec<String>> = Default::default();
        let keep_alive = Duration::from_secs(u64::from(o.keep_alive_secs.max(5)));
        let mut last_sent = Instant::now();
        let write = |conn: &mut Conn, bytes: Vec<u8>, last_sent: &mut Instant| -> bool {
            *last_sent = Instant::now();
            conn.write_all(&bytes).and_then(|_| conn.flush()).is_ok()
        };
        if !o.subscriptions.is_empty() {
            let pid = id();
            pending_subs.insert(pid, o.subscriptions.iter().map(|(t, _)| t.clone()).collect());
            write(&mut conn, subscribe_packet(pid, &o.subscriptions), &mut last_sent);
        }
        loop {
            loop {
                match cmd_rx.try_recv() {
                    Ok(WsCommand::Publish { topic, payload, qos, retain }) => {
                        let pid = if qos > 0 { id() } else { 0 };
                        if write(
                            &mut conn,
                            publish_packet(&topic, payload.as_bytes(), qos, retain, pid),
                            &mut last_sent,
                        ) {
                            emit(WsEvent::Message {
                                direction: Direction::Sent,
                                text: payload,
                                binary: false,
                                at: Utc::now(),
                                label: Some(topic),
                            });
                        } else {
                            emit(WsEvent::Error("publish failed".into()));
                        }
                    }
                    Ok(WsCommand::Send(text)) => {
                        emit(WsEvent::Error(format!("pick a topic to publish `{text}` to")));
                    }
                    Ok(WsCommand::Subscribe { topic, qos }) => {
                        let pid = id();
                        pending_subs.insert(pid, vec![topic.clone()]);
                        write(&mut conn, subscribe_packet(pid, &[(topic, qos)]), &mut last_sent);
                    }
                    Ok(WsCommand::Unsubscribe(topic)) => {
                        let pid = id();
                        if write(&mut conn, unsubscribe_packet(pid, &topic), &mut last_sent) {
                            emit(WsEvent::Info(format!("unsubscribed from {topic}")));
                        }
                    }
                    Ok(WsCommand::Ping) => {
                        write(&mut conn, vec![0xC0, 0], &mut last_sent);
                    }
                    Ok(WsCommand::Emit { .. }) => emit(WsEvent::Error("not supported over MQTT".into())),
                    Ok(WsCommand::Close) | Err(TryRecvError::Disconnected) => {
                        write(&mut conn, vec![0xE0, 0], &mut last_sent);
                        return emit(WsEvent::Closed { reason: "disconnected by client".into() });
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            if last_sent.elapsed() >= keep_alive * 3 / 4 {
                write(&mut conn, vec![0xC0, 0], &mut last_sent);
            }
            match conn.read(&mut chunk) {
                Ok(0) => return emit(WsEvent::Closed { reason: "the broker closed the connection".into() }),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => {
                    emit(WsEvent::Error(e.to_string()));
                    return emit(WsEvent::Closed { reason: "error".into() });
                }
            }
            while let Some((first, body)) = decode(&mut buf) {
                match first >> 4 {
                    3 => {
                        let Some((topic, payload, qos, retain, pid)) = parse_publish(first, &body) else { continue };
                        match qos {
                            1 => {
                                write(&mut conn, ack(0x40, pid), &mut last_sent);
                            }
                            2 => {
                                write(&mut conn, ack(0x50, pid), &mut last_sent);
                            }
                            _ => {}
                        }
                        let text = match String::from_utf8(payload) {
                            Ok(t) => t,
                            Err(e) => format!("<{} bytes binary>", e.as_bytes().len()),
                        };
                        let label = if retain { format!("{topic} (retained)") } else { topic };
                        emit(WsEvent::Message {
                            direction: Direction::Received,
                            text,
                            binary: false,
                            at: Utc::now(),
                            label: Some(label),
                        });
                    }
                    5 => {
                        // PUBREC for our QoS 2 publish → PUBREL
                        write(&mut conn, ack(0x62, u16_at(&body, 0)), &mut last_sent);
                    }
                    6 => {
                        // PUBREL for a QoS 2 message we received → PUBCOMP
                        write(&mut conn, ack(0x70, u16_at(&body, 0)), &mut last_sent);
                    }
                    9 => {
                        let pid = u16_at(&body, 0);
                        let topics = pending_subs.remove(&pid).unwrap_or_default();
                        for (i, code) in body.iter().skip(2).enumerate() {
                            let topic = topics.get(i).cloned().unwrap_or_default();
                            if *code == 0x80 {
                                emit(WsEvent::Error(format!("subscription to {topic} refused")));
                            } else {
                                emit(WsEvent::Info(format!("subscribed to {topic} (QoS {code})")));
                            }
                        }
                    }
                    _ => {} // PUBACK, PUBCOMP, UNSUBACK, PINGRESP
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
    fn urls() {
        assert_eq!(parse_url("mqtt://broker.local").unwrap(), ("broker.local".into(), 1883, false));
        assert_eq!(parse_url("mqtts://broker.local").unwrap(), ("broker.local".into(), 8883, true));
        assert_eq!(parse_url("localhost:1884").unwrap(), ("localhost".into(), 1884, false));
        assert!(parse_url("http://x").is_err());
    }

    #[test]
    fn packets_round_trip() {
        let o = MqttOptions {
            client_id: "c1".into(),
            username: Some("u".into()),
            password: Some("p".into()),
            keep_alive_secs: 60,
            clean_session: true,
            ..Default::default()
        };
        let c = connect_packet(&o);
        assert_eq!(c[0], 0x10);
        assert_eq!(&c[2..10], &[0, 4, b'M', b'Q', b'T', b'T', 4, 0xC2]);

        let mut buf = publish_packet("a/b", &vec![b'x'; 300], 1, true, 7);
        buf.extend(publish_packet("t", b"hi", 0, false, 0));
        let (first, body) = decode(&mut buf).unwrap();
        assert_eq!(first, 0x33);
        let (topic, payload, qos, retain, id) = parse_publish(first, &body).unwrap();
        assert_eq!((topic.as_str(), payload.len(), qos, retain, id), ("a/b", 300, 1, true, 7));
        let (first, body) = decode(&mut buf).unwrap();
        assert_eq!(parse_publish(first, &body).unwrap().1, b"hi");
        assert!(buf.is_empty());
        // Incomplete packets wait for more bytes.
        let mut part = subscribe_packet(1, &[("x/#".into(), 1)]);
        let tail = part.split_off(4);
        assert!(decode(&mut part).is_none());
        part.extend(tail);
        assert_eq!(decode(&mut part).unwrap().0, 0x82);
    }
}
