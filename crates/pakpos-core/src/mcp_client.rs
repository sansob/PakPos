//! Model Context Protocol client, for testing MCP servers like REST APIs:
//! list and call tools, read resources, get prompts.
//!
//! Transports: **stdio** (the "URL" is a command line, the server is spawned)
//! and **Streamable HTTP** (`http(s)://…`, JSON or SSE answers, session id).

use crate::http::HttpSettings;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("cannot start `{0}`: {1}")]
    Spawn(String, String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("the server answered with an error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("no answer within {0:?}")]
    Timeout(Duration),
    #[error("invalid answer: {0}")]
    Invalid(String),
}

/// What the server said about itself in `initialize`.
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    pub instructions: String,
    pub capabilities: Value,
}

enum Transport {
    Stdio { child: Child, stdin: ChildStdin, lines: Receiver<String>, stderr: Receiver<String> },
    Http { client: reqwest::blocking::Client, url: String, headers: Vec<(String, String)>, session: Option<String> },
}

/// A connected (initialized) client.
pub struct McpClient {
    transport: Transport,
    next_id: u64,
    timeout: Duration,
    pub server: ServerInfo,
}

impl Drop for McpClient {
    fn drop(&mut self) {
        match &mut self.transport {
            Transport::Stdio { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Transport::Http { client, url, headers, session: Some(s) } => {
                // Best effort: end the session.
                let mut req = client.delete(url.as_str()).header("Mcp-Session-Id", s.as_str());
                for (k, v) in headers.iter() {
                    req = req.header(k, v);
                }
                let _ = req.send();
            }
            Transport::Http { .. } => {}
        }
    }
}

/// Splits a command line: spaces separate words, single and double quotes
/// group them. A backslash only escapes a quote, a backslash or a space, so
/// Windows paths (`C:\tools\server.exe`) work unquoted.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                cur.extend(chars.next());
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') if matches!(chars.peek(), Some('"' | '\'' | '\\' | ' ')) => {
                cur.extend(chars.next());
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

/// Is this target an HTTP endpoint (otherwise a command line)?
pub fn is_http(target: &str) -> bool {
    let t = target.trim_start();
    t.starts_with("http://") || t.starts_with("https://")
}

/// The JSON-RPC message with `id` inside an SSE body (Streamable HTTP).
fn from_sse(body: &str, id: u64) -> Option<Value> {
    let mut parser = crate::sse::Parser::default();
    let mut events = parser.feed(body);
    events.extend(parser.feed("\n\n"));
    events
        .into_iter()
        .filter_map(|e| serde_json::from_str::<Value>(&e.data).ok())
        .find(|v| v.get("id") == Some(&json!(id)))
}

impl McpClient {
    /// Connects and initializes. For stdio, `env` is added to the server's environment.
    pub fn connect(
        target: &str,
        headers: &[(String, String)],
        env: &[(String, String)],
        settings: &HttpSettings,
    ) -> Result<Self, McpError> {
        let timeout = Duration::from_secs(settings.timeout_secs.max(1));
        let transport = if is_http(target) {
            let client = crate::http::client_for(settings, target).map_err(|e| McpError::Transport(e.to_string()))?;
            Transport::Http { client, url: target.trim().to_string(), headers: headers.to_vec(), session: None }
        } else {
            let argv = split_command(target);
            let Some((program, args)) = argv.split_first() else {
                return Err(McpError::Spawn(target.into(), "empty command".into()));
            };
            let mut cmd = Command::new(program);
            cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            for (k, v) in env {
                cmd.env(k, v);
            }
            let mut child = cmd.spawn().map_err(|e| McpError::Spawn(target.into(), e.to_string()))?;
            let stdin = child.stdin.take().ok_or_else(|| McpError::Spawn(target.into(), "no stdin".into()))?;
            let stdout = child.stdout.take().ok_or_else(|| McpError::Spawn(target.into(), "no stdout".into()))?;
            let stderr = child.stderr.take().ok_or_else(|| McpError::Spawn(target.into(), "no stderr".into()))?;
            let (tx, lines) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            let (etx, stderr_rx) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let _ = etx.send(line);
                }
            });
            Transport::Stdio { child, stdin, lines, stderr: stderr_rx }
        };
        let mut client = McpClient { transport, next_id: 0, timeout, server: ServerInfo::default() };
        let result = client.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "pakpos", "version": crate::VERSION },
            }),
        )?;
        let info = result.get("serverInfo").cloned().unwrap_or(Value::Null);
        client.server = ServerInfo {
            name: info.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            version: info.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
            protocol_version: result.get("protocolVersion").and_then(Value::as_str).unwrap_or("").to_string(),
            instructions: result.get("instructions").and_then(Value::as_str).unwrap_or("").to_string(),
            capabilities: result.get("capabilities").cloned().unwrap_or(Value::Null),
        };
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    /// Server stderr lines so far (stdio only), e.g. for a console view.
    pub fn stderr_lines(&self) -> Vec<String> {
        match &self.transport {
            Transport::Stdio { stderr, .. } => stderr.try_iter().collect(),
            Transport::Http { .. } => Vec::new(),
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        match &mut self.transport {
            Transport::Stdio { stdin, .. } => {
                writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).map_err(|e| McpError::Transport(e.to_string()))
            }
            Transport::Http { .. } => self.post(&msg, None).map(|_| ()),
        }
    }

    fn post(&mut self, msg: &Value, id: Option<u64>) -> Result<Option<Value>, McpError> {
        let Transport::Http { client, url, headers, session } = &mut self.transport else { unreachable!() };
        let mut req = client
            .post(url.as_str())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_VERSION)
            .body(msg.to_string());
        for (k, v) in headers.iter() {
            req = req.header(k, v);
        }
        if let Some(s) = session.as_deref() {
            req = req.header("Mcp-Session-Id", s);
        }
        let resp = req.send().map_err(|e| McpError::Transport(e.to_string()))?;
        if let Some(s) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *session = Some(s.to_string());
        }
        let status = resp.status();
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let body = resp.text().map_err(|e| McpError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(McpError::Transport(format!("HTTP {status}: {}", body.chars().take(300).collect::<String>())));
        }
        let Some(id) = id else { return Ok(None) };
        if content_type.contains("text/event-stream") {
            return from_sse(&body, id)
                .map(Some)
                .ok_or_else(|| McpError::Invalid("no answer in the event stream".into()));
        }
        serde_json::from_str(&body).map(Some).map_err(|e| McpError::Invalid(format!("{e}: {body}")))
    }

    /// Sends a request and waits for its answer; returns `result`.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let answer = match &mut self.transport {
            Transport::Http { .. } => self.post(&msg, Some(id))?.ok_or_else(|| McpError::Invalid("empty".into()))?,
            Transport::Stdio { stdin, lines, child, .. } => {
                writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).map_err(|e| McpError::Transport(e.to_string()))?;
                let deadline = Instant::now() + self.timeout;
                loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    let line = match lines.recv_timeout(left) {
                        Ok(l) => l,
                        Err(mpsc::RecvTimeoutError::Timeout) => return Err(McpError::Timeout(self.timeout)),
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let code = child.try_wait().ok().flatten().and_then(|s| s.code());
                            return Err(McpError::Transport(format!(
                                "the server exited{}",
                                code.map(|c| format!(" with code {c}")).unwrap_or_default()
                            )));
                        }
                    };
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    if v.get("id") == Some(&json!(id)) && v.get("method").is_none() {
                        break v;
                    }
                    // A request from the server (ping, roots/list…): answer it.
                    if let (Some(sid), Some(m)) = (v.get("id"), v.get("method").and_then(Value::as_str)) {
                        let reply = if m == "ping" {
                            json!({ "jsonrpc": "2.0", "id": sid, "result": {} })
                        } else {
                            json!({ "jsonrpc": "2.0", "id": sid, "error": { "code": -32601, "message": "not supported by PakPos" } })
                        };
                        let _ = writeln!(stdin, "{reply}").and_then(|_| stdin.flush());
                    }
                }
            }
        };
        if let Some(e) = answer.get("error") {
            return Err(McpError::Rpc {
                code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: e.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
            });
        }
        answer.get("result").cloned().ok_or_else(|| McpError::Invalid(answer.to_string()))
    }

    /// All pages of a `*/list` call (`tools`, `resources`, `prompts`).
    pub fn list(&mut self, what: &str) -> Result<Vec<Value>, McpError> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let r = self.request(&format!("{what}/list"), params)?;
            items.extend(r.get(what).and_then(Value::as_array).cloned().unwrap_or_default());
            cursor = r.get("nextCursor").and_then(Value::as_str).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(items)
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request("tools/call", json!({ "name": name, "arguments": arguments }))
    }

    pub fn read_resource(&mut self, uri: &str) -> Result<Value, McpError> {
        self.request("resources/read", json!({ "uri": uri }))
    }

    pub fn get_prompt(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request("prompts/get", json!({ "name": name, "arguments": arguments }))
    }
}

/// An example `arguments` object from a tool's JSON Schema.
pub fn arguments_template(input_schema: &Value) -> Value {
    fn example(schema: &Value, depth: usize) -> Value {
        if depth > 6 {
            return Value::Null;
        }
        if let Some(d) = schema.get("default") {
            return d.clone();
        }
        if let Some(e) = schema.get("enum").and_then(Value::as_array).and_then(|a| a.first()) {
            return e.clone();
        }
        let ty = match schema.get("type") {
            Some(Value::String(t)) => t.clone(),
            Some(Value::Array(a)) => {
                a.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("null").to_string()
            }
            _ if schema.get("properties").is_some() => "object".into(),
            _ => String::new(),
        };
        match ty.as_str() {
            "object" => {
                let mut m = serde_json::Map::new();
                if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                    for (k, v) in props {
                        m.insert(k.clone(), example(v, depth + 1));
                    }
                }
                Value::Object(m)
            }
            "array" => Value::Array(vec![example(schema.get("items").unwrap_or(&Value::Null), depth + 1)]),
            "string" => Value::String(String::new()),
            "integer" | "number" => json!(0),
            "boolean" => json!(false),
            _ => Value::Null,
        }
    }
    match example(input_schema, 0) {
        Value::Object(m) => Value::Object(m),
        _ => json!({}),
    }
}

/// A result as a response: status 200 (or 500 for a tool error), body =
/// pretty JSON, server name in the headers. Tests can then check e.g.
/// `json.content[0].text`.
pub fn to_response(
    result: Result<Value, McpError>,
    server: &ServerInfo,
    elapsed: Duration,
    target: &str,
) -> crate::http::Response {
    let (status, text, body) = match result {
        Ok(v) if v.get("isError").and_then(Value::as_bool) == Some(true) => (500, "Tool error", v),
        Ok(v) => (200, "OK", v),
        Err(McpError::Rpc { code, message }) => {
            (400, "JSON-RPC error", json!({ "error": { "code": code, "message": message } }))
        }
        Err(e) => (502, "MCP error", json!({ "error": { "message": e.to_string() } })),
    };
    crate::http::Response {
        status,
        status_text: text.into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("mcp-server".into(), format!("{} {}", server.name, server.version).trim().to_string()),
            ("mcp-protocol-version".into(), server.protocol_version.clone()),
        ],
        body: serde_json::to_string_pretty(&body).unwrap_or_default().into_bytes(),
        elapsed_ms: elapsed.as_millis(),
        url: target.to_string(),
        http_version: "MCP".into(),
    }
}

/// Runs an MCP request: connects, does the configured operation and returns
/// what was sent (for display) and the result as a response.
pub fn execute(
    req: &crate::model::Request,
    scope: &crate::vars::VarScope,
    fallback_auth: Option<&crate::model::Auth>,
    settings: &HttpSettings,
) -> (crate::http::PreparedRequest, crate::http::Response) {
    use crate::model::{Body, McpOperation};
    let started = Instant::now();
    let cfg = req.mcp.clone().unwrap_or_default();
    let target = scope.resolve(req.url.trim());
    let name = scope.resolve(cfg.target.trim());
    let args_text = match &req.body {
        Body::Raw { content, .. } => scope.resolve(content),
        _ => String::new(),
    };
    // Headers and auth only matter for HTTP servers.
    let headers = if is_http(&target) {
        let mut meta = req.clone();
        meta.body = Body::None;
        crate::http::prepare(&meta, scope, fallback_auth).map(|p| p.headers).unwrap_or_default()
    } else {
        Vec::new()
    };
    let env: Vec<(String, String)> = cfg
        .env
        .iter()
        .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
        .map(|kv| (scope.resolve(&kv.key), scope.resolve(&kv.value)))
        .collect();
    let mut server = ServerInfo::default();
    let result = (|| -> Result<Value, McpError> {
        let args: Value = if args_text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&args_text)
                .map_err(|e| McpError::Invalid(format!("arguments are not valid JSON: {e}")))?
        };
        let mut client = McpClient::connect(&target, &headers, &env, settings)?;
        server = client.server.clone();
        match cfg.operation {
            McpOperation::CallTool => client.call_tool(&name, args),
            McpOperation::ReadResource => client.read_resource(&name),
            McpOperation::GetPrompt => client.get_prompt(&name, args),
            McpOperation::ListTools => client.list("tools").map(|t| json!({ "tools": t })),
        }
    })();
    let response = to_response(result, &server, started.elapsed(), &target);
    let prepared = crate::http::PreparedRequest {
        method: "MCP".into(),
        url: format!("{target}  ·  {} {name}", cfg.operation.label()),
        headers,
        body: crate::http::PreparedBody::Text(args_text),
        challenge: None,
    };
    (prepared, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines() {
        assert_eq!(split_command("npx -y @mcp/server  /tmp/a\\ b"), ["npx", "-y", "@mcp/server", "/tmp/a b"]);
        assert_eq!(split_command(r#"python3 -c "print('hi there')" ''"#), ["python3", "-c", "print('hi there')", ""]);
        assert!(is_http(" https://x/mcp") && !is_http("pakpos-cli mcp"));
        assert_eq!(split_command(r"C:\tools\server.exe --dir C:\data"), [r"C:\tools\server.exe", "--dir", r"C:\data"]);
        assert_eq!(split_command(r#""C:\Program Files\srv.exe" mcp"#), [r"C:\Program Files\srv.exe", "mcp"]);
    }

    #[test]
    fn templates_from_schemas() {
        let schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" },
                "days": { "type": "integer", "default": 3 },
                "units": { "enum": ["metric", "imperial"] },
                "tags": { "type": "array", "items": { "type": "string" } },
                "opts": { "type": ["object", "null"], "properties": { "x": { "type": "boolean" } } }
            }
        });
        assert_eq!(
            arguments_template(&schema),
            json!({ "city": "", "days": 3, "units": "metric", "tags": [""], "opts": { "x": false } })
        );
        assert_eq!(arguments_template(&json!({})), json!({}));
    }

    #[test]
    fn sse_answers() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\n";
        assert_eq!(from_sse(body, 3).unwrap()["result"]["ok"], true);
        assert!(from_sse(body, 4).is_none());
    }
}
