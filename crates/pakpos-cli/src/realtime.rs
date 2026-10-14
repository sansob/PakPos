//! `pakpos-cli sse`, `socketio` and `mqtt`: realtime sessions in the terminal.
//! Received messages go to stdout, everything else to stderr.

use crate::{load_env, parse_kv, Style};
use anyhow::{Context, Result};
use pakpos_core::http::{self, HttpSettings};
use pakpos_core::storage::{self, Workspace};
use pakpos_core::vars::VarScope;
use pakpos_core::ws::{Direction, WsCommand, WsEvent, WsHandle};
use pakpos_core::{HttpMethod, KeyValue, Request};
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// Options shared by the three commands.
#[derive(clap::Args)]
pub struct Common {
    #[arg(short, long)]
    environment: Option<String>,
    #[arg(long = "var", value_parser = parse_kv)]
    vars: Vec<(String, String)>,
    /// Exit after this many seconds (default: until Ctrl+C or the server closes).
    #[arg(long)]
    timeout: Option<u64>,
    /// Exit after receiving this many messages.
    #[arg(short = 'c', long)]
    count: Option<usize>,
}

#[derive(clap::Args)]
pub struct SseArgs {
    /// http(s):// URL of the event stream, may contain {{variables}}.
    url: String,
    /// Header "Name: value" (repeatable).
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,
    #[command(flatten)]
    common: Common,
}

#[derive(clap::Args)]
pub struct SocketIoArgs {
    /// Server URL; a path selects the namespace (http://host:3000/admin).
    url: String,
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,
    /// Emit after connecting: --emit 'event={"json":1}' (repeatable). stdin lines
    /// in the same form are emitted too.
    #[arg(long)]
    emit: Vec<String>,
    /// Server path (default /socket.io/).
    #[arg(long, default_value = "")]
    path: String,
    /// Auth payload sent with the namespace connect, as JSON.
    #[arg(long, default_value = "")]
    auth: String,
    #[command(flatten)]
    common: Common,
}

#[derive(clap::Args)]
pub struct MqttArgs {
    /// mqtt://host[:1883] or mqtts://host[:8883].
    url: String,
    /// Subscribe to a topic filter, optionally with QoS: --sub 'sensors/#:1' (repeatable).
    #[arg(short, long = "sub")]
    subscribe: Vec<String>,
    /// Topic to publish --message and stdin lines to.
    #[arg(short, long = "pub")]
    publish: Option<String>,
    /// Message to publish after connecting (repeatable).
    #[arg(short, long)]
    message: Vec<String>,
    #[arg(short, long, default_value_t = 0)]
    qos: u8,
    #[arg(long)]
    retain: bool,
    #[arg(long, default_value = "")]
    client_id: String,
    #[arg(short, long)]
    username: Option<String>,
    #[arg(long)]
    password: Option<String>,
    #[arg(short = 'k', long)]
    insecure: bool,
    #[command(flatten)]
    tls: crate::TlsArgs,
    #[command(flatten)]
    common: Common,
}

fn scope_for(common: &Common) -> Result<VarScope> {
    let env = match &common.environment {
        Some(e) => Some(load_env(e)?.0),
        None => None,
    };
    let (ws, _) = Workspace::load(storage::default_root());
    let mut scope = VarScope::new().with_environment(env.as_ref()).with_globals(&ws.globals);
    for (k, v) in &common.vars {
        scope.runtime.insert(k.clone(), v.clone());
    }
    Ok(scope)
}

fn prepared(url: &str, headers: &[String], scope: &VarScope) -> Result<http::PreparedRequest> {
    let mut req = Request::new(HttpMethod::Get, url);
    for h in headers {
        let (k, v) = h.split_once(':').with_context(|| format!("header must be `Name: value`, got `{h}`"))?;
        req.headers.push(KeyValue::new(k.trim(), v.trim()));
    }
    Ok(http::prepare(&req, scope, None)?)
}

/// Prints events until the session ends. `on_connect` commands are sent
/// once connected; stdin lines are turned into commands by `from_line`.
fn drive(
    handle: WsHandle,
    on_connect: Vec<WsCommand>,
    from_line: Option<Box<dyn Fn(String) -> Option<WsCommand> + Send>>,
    common: &Common,
    style: &Style,
) -> Result<ExitCode> {
    if let Some(convert) = from_line {
        let tx = handle.tx.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if let Some(cmd) = convert(line) {
                    let _ = tx.send(cmd);
                }
            }
        });
    }
    let deadline = common.timeout.map(|s| Instant::now() + Duration::from_secs(s));
    let mut pending = on_connect;
    let mut received = 0usize;
    loop {
        if deadline.is_some_and(|d| Instant::now() > d) || common.count.is_some_and(|c| received >= c) {
            handle.close();
            // Give the worker a moment to say goodbye (MQTT DISCONNECT etc.).
            let _ = handle.rx.recv_timeout(Duration::from_millis(300));
            return Ok(ExitCode::SUCCESS);
        }
        match handle.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(WsEvent::Connected { status, .. }) => {
                let detail = if status == 0 { String::new() } else { format!(" HTTP {status}") };
                eprintln!("{}{}", style.green("connected"), style.dim(&detail));
                for c in pending.drain(..) {
                    let _ = handle.tx.send(c);
                }
            }
            Ok(WsEvent::Message { direction, text, label, .. }) => {
                let label = label.map(|l| format!("[{l}] ")).unwrap_or_default();
                match direction {
                    Direction::Sent => eprintln!("{} {}{text}", style.paint("33", "↑"), style.dim(&label)),
                    Direction::Received => {
                        received += 1;
                        println!("{}{text}", style.dim(&label));
                    }
                }
            }
            Ok(WsEvent::Info(t)) => eprintln!("{} {t}", style.dim("•")),
            Ok(WsEvent::Error(e)) => eprintln!("{} {e}", style.red("error:")),
            Ok(WsEvent::Closed { reason }) => {
                eprintln!("{} {reason}", style.dim("closed:"));
                return Ok(if reason == "not connected" { ExitCode::from(2) } else { ExitCode::SUCCESS });
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(ExitCode::from(1)),
        }
    }
}

pub fn sse(a: SseArgs, style: &Style) -> Result<ExitCode> {
    let scope = scope_for(&a.common)?;
    let p = prepared(&a.url, &a.headers, &scope)?;
    let handle = pakpos_core::sse::connect(&p, &HttpSettings::default(), || {});
    drive(handle, Vec::new(), None, &a.common, style)
}

fn parse_emit(s: &str) -> Option<WsCommand> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (event, data) = s.split_once('=').unwrap_or((s, ""));
    Some(WsCommand::Emit { event: event.trim().to_string(), data: data.to_string() })
}

pub fn socketio(a: SocketIoArgs, style: &Style) -> Result<ExitCode> {
    let scope = scope_for(&a.common)?;
    let p = prepared(&a.url, &a.headers, &scope)?;
    let handle = pakpos_core::socketio::connect(&p, &a.path, &scope.resolve(&a.auth), || {});
    let emits = a.emit.iter().filter_map(|e| parse_emit(&scope.resolve(e))).collect();
    let line_scope = scope.clone();
    drive(handle, emits, Some(Box::new(move |l| parse_emit(&line_scope.resolve(&l)))), &a.common, style)
}

fn topic_qos(s: &str) -> (String, u8) {
    match s.rsplit_once(':') {
        Some((t, q)) if matches!(q, "0" | "1" | "2") => (t.to_string(), q.parse().unwrap_or(0)),
        _ => (s.to_string(), 0),
    }
}

pub fn mqtt(a: MqttArgs, style: &Style) -> Result<ExitCode> {
    let scope = scope_for(&a.common)?;
    let options = pakpos_core::mqtt::MqttOptions {
        url: scope.resolve(&a.url),
        client_id: scope.resolve(&a.client_id),
        username: a.username.as_deref().map(|u| scope.resolve(u)),
        password: a.password.as_deref().map(|p| scope.resolve(p)),
        keep_alive_secs: 60,
        clean_session: true,
        subscriptions: a.subscribe.iter().map(|s| topic_qos(&scope.resolve(s))).collect(),
        settings: HttpSettings {
            verify_tls: !a.insecure,
            client_certs: a.tls.client_certs(&Workspace::load(storage::default_root()).0),
            ..Default::default()
        },
    };
    let handle = pakpos_core::mqtt::connect(options, || {});
    let topic = a.publish.as_deref().map(|t| scope.resolve(t));
    let publish = |topic: &str, payload: String| WsCommand::Publish {
        topic: topic.to_string(),
        payload,
        qos: a.qos.min(2),
        retain: a.retain,
    };
    let mut on_connect = Vec::new();
    if let Some(t) = &topic {
        on_connect.extend(a.message.iter().map(|m| publish(t, scope.resolve(m))));
    } else if !a.message.is_empty() {
        anyhow::bail!("--message needs --pub <topic>");
    }
    let from_line: Option<Box<dyn Fn(String) -> Option<WsCommand> + Send>> = topic.clone().map(|t| {
        let (qos, retain) = (a.qos.min(2), a.retain);
        Box::new(move |line: String| {
            (!line.trim().is_empty()).then(|| WsCommand::Publish { topic: t.clone(), payload: line, qos, retain })
        }) as Box<dyn Fn(String) -> Option<WsCommand> + Send>
    });
    drive(handle, on_connect, from_line, &a.common, style)
}
