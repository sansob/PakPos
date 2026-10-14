//! Minimal Model Context Protocol server (JSON-RPC 2.0 over stdio).
//!
//! Exposes the PakPos workspace to AI agents as tools: list collections and
//! environments, send ad-hoc requests, run saved requests or whole collections.
//!
//! Transport: one JSON message per line on stdin/stdout, logs on stderr.

use anyhow::Result;
use pakpos_core::http::{self, HttpSettings};
use pakpos_core::model::{Item, RawLanguage};
use pakpos_core::runner::{run_collection, RunOptions};
use pakpos_core::storage::Workspace;
use pakpos_core::vars::VarScope;
use pakpos_core::{codegen, Body, Collection, Environment, HttpMethod, KeyValue, Request};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// Response bodies are truncated so they fit comfortably in a model context.
const MAX_BODY_CHARS: usize = 100_000;

pub struct McpConfig {
    pub allowed_hosts: Vec<String>,
    pub allow_write: bool,
    /// Run scripts of collections trusted in the app.
    pub allow_scripts: bool,
    pub workspace_root: PathBuf,
}

pub struct Server {
    config: McpConfig,
}

impl Server {
    pub fn new(config: McpConfig) -> Self {
        Self { config }
    }
}

/// Runs the server until stdin closes.
pub fn serve(config: McpConfig) -> Result<()> {
    if config.allowed_hosts.is_empty() {
        eprintln!(
            "pakpos mcp: WARNING no --allow-host given; the agent can reach ANY host, \
             including localhost and your internal network."
        );
    }
    let mut server = Server::new(config);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(msg),
            Err(e) => Some(error_response(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

impl Server {
    /// Handles one JSON-RPC message; returns the reply (None for notifications).
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (no id) never get a reply.
        let id = id?;
        let reply = match method {
            "initialize" => {
                let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(SUPPORTED_VERSIONS[0]);
                let version =
                    SUPPORTED_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(SUPPORTED_VERSIONS[0]);
                ok_response(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "pakpos", "version": pakpos_core::VERSION },
                        "instructions": "PakPos API client. Use list_collections / list_requests to discover saved API requests, run_saved_request or run_collection to execute them with an environment, and send_request for ad-hoc HTTP calls."
                    }),
                )
            }
            "ping" => ok_response(id, json!({})),
            "tools/list" => ok_response(id, json!({ "tools": self.tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = match self.call(name, &args) {
                    Ok(text) => text_result(text, false),
                    Err(e) => text_result(format!("Error: {e:#}"), true),
                };
                ok_response(id, result)
            }
            other => error_response(id, -32601, &format!("method not found: {other}")),
        };
        Some(reply)
    }

    fn tools(&self) -> Vec<Value> {
        let env_prop = json!({ "type": "string", "description": "Environment name from the workspace (optional)." });
        let mut tools = vec![
            json!({
                "name": "list_collections",
                "description": "List collections saved in the PakPos workspace.",
                "inputSchema": { "type": "object", "properties": {} },
                "annotations": { "readOnlyHint": true }
            }),
            json!({
                "name": "list_environments",
                "description": "List environments and their variable names (values are hidden).",
                "inputSchema": { "type": "object", "properties": {} },
                "annotations": { "readOnlyHint": true }
            }),
            json!({
                "name": "list_requests",
                "description": "List the requests of a collection with method, URL and folder.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "collection": { "type": "string", "description": "Collection name." } },
                    "required": ["collection"]
                },
                "annotations": { "readOnlyHint": true }
            }),
            json!({
                "name": "send_request",
                "description": "Send an HTTP request. URL, headers and body may use {{variables}} from the environment.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "method": { "type": "string", "enum": HttpMethod::ALL.iter().map(|m| m.as_str()).collect::<Vec<_>>() },
                        "url": { "type": "string" },
                        "headers": { "type": "object", "additionalProperties": { "type": "string" } },
                        "body": { "type": "string", "description": "Raw body (JSON is detected automatically)." },
                        "environment": env_prop
                    },
                    "required": ["method", "url"]
                },
                "annotations": { "openWorldHint": true }
            }),
            json!({
                "name": "run_saved_request",
                "description": "Execute a request saved in a collection, including its tests.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "request": { "type": "string", "description": "Request name or id." },
                        "environment": env_prop
                    },
                    "required": ["collection", "request"]
                },
                "annotations": { "openWorldHint": true }
            }),
            json!({
                "name": "run_collection",
                "description": "Run all requests of a collection in order and report test results.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "environment": env_prop,
                        "filter": { "type": "string", "description": "Only requests whose folder/name contains this." }
                    },
                    "required": ["collection"]
                },
                "annotations": { "openWorldHint": true }
            }),
        ];
        if self.config.allow_write {
            tools.push(json!({
                "name": "save_curl_request",
                "description": "Parse a cURL command and save it as a request in a collection (created if missing).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "curl": { "type": "string" },
                        "name": { "type": "string" }
                    },
                    "required": ["collection", "curl"]
                }
            }));
        }
        tools
    }

    fn workspace(&self) -> Workspace {
        Workspace::load(&self.config.workspace_root).0
    }

    fn settings(&self) -> HttpSettings {
        HttpSettings {
            // Redirects could bounce to a host outside the allow-list.
            follow_redirects: self.config.allowed_hosts.is_empty(),
            ..Default::default()
        }
    }

    fn find_collection(ws: &Workspace, name: &str) -> Result<Collection> {
        ws.collections.iter().find(|c| c.name.eq_ignore_ascii_case(name) || c.id == name).cloned().ok_or_else(|| {
            let names: Vec<_> = ws.collections.iter().map(|c| c.name.as_str()).collect();
            anyhow::anyhow!("collection `{name}` not found. Available: {}", names.join(", "))
        })
    }

    fn find_env(ws: &Workspace, name: Option<&str>) -> Result<Option<Environment>> {
        match name {
            None => Ok(ws.active_environment().cloned()),
            Some(n) => ws
                .environments
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(n) || e.id == n)
                .cloned()
                .map(Some)
                .ok_or_else(|| anyhow::anyhow!("environment `{n}` not found")),
        }
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<String> {
        let ws = self.workspace();
        match name {
            "list_collections" => Ok(serde_json::to_string_pretty(
                &ws.collections
                    .iter()
                    .map(|c| json!({ "name": c.name, "id": c.id, "requests": c.request_count(), "description": c.description }))
                    .collect::<Vec<_>>(),
            )?),
            "list_environments" => Ok(serde_json::to_string_pretty(
                &ws.environments
                    .iter()
                    .map(|e| json!({
                        "name": e.name,
                        "active": ws.settings.active_environment.as_deref() == Some(e.id.as_str()),
                        "variables": e.variables.iter().filter(|v| v.enabled).map(|v| v.key.clone()).collect::<Vec<_>>()
                    }))
                    .collect::<Vec<_>>(),
            )?),
            "list_requests" => {
                let c = Self::find_collection(&ws, str_arg(args, "collection").unwrap_or(""))?;
                Ok(serde_json::to_string_pretty(
                    &c.flatten()
                        .into_iter()
                        .map(|(folder, r)| json!({
                            "id": r.id, "name": r.name, "method": r.method.as_str(),
                            "url": r.url, "folder": folder.join("/"), "tests": r.assertions.len()
                        }))
                        .collect::<Vec<_>>(),
                )?)
            }
            "send_request" => {
                let method = str_arg(args, "method").and_then(HttpMethod::parse).unwrap_or_default();
                let mut req = Request::new(method, str_arg(args, "url").unwrap_or(""));
                if let Some(headers) = args.get("headers").and_then(Value::as_object) {
                    for (k, v) in headers {
                        req.headers.push(KeyValue::new(k, v.as_str().unwrap_or(&v.to_string())));
                    }
                }
                if let Some(body) = str_arg(args, "body") {
                    let language = if serde_json::from_str::<Value>(body).is_ok() {
                        RawLanguage::Json
                    } else {
                        RawLanguage::Text
                    };
                    req.body = Body::Raw { content: body.to_string(), language };
                }
                let env = Self::find_env(&ws, str_arg(args, "environment"))?;
                self.execute(&ws, &req, env.as_ref(), &[])
            }
            "run_saved_request" => {
                let c = Self::find_collection(&ws, str_arg(args, "collection").unwrap_or(""))?;
                let wanted = str_arg(args, "request").unwrap_or("");
                let (_, req) = c
                    .flatten()
                    .into_iter()
                    .find(|(_, r)| r.id == wanted || r.name.eq_ignore_ascii_case(wanted))
                    .ok_or_else(|| anyhow::anyhow!("request `{wanted}` not found in `{}`", c.name))?;
                let env = Self::find_env(&ws, str_arg(args, "environment"))?;
                self.execute(&ws, &req, env.as_ref(), &c.variables)
            }
            "run_collection" => {
                let c = Self::find_collection(&ws, str_arg(args, "collection").unwrap_or(""))?;
                let mut env = Self::find_env(&ws, str_arg(args, "environment"))?;
                let options = RunOptions {
                    settings: self.settings(),
                    filter: str_arg(args, "filter").map(str::to_string),
                    globals: ws.globals.clone(),
                    allowed_hosts: self.config.allowed_hosts.clone(),
                    tokens: ws.tokens.clone(),
                    allow_scripts: self.config.allow_scripts && ws.is_trusted(&c.id),
                    ..Default::default()
                };
                let report = run_collection(&c, env.as_mut(), &options, |_| {});
                if report.tokens != ws.tokens {
                    let mut ws = ws.clone();
                    ws.tokens = report.tokens.clone();
                    let _ = ws.save_tokens();
                }
                let (passed, total) = report.assertion_counts();
                let mut text = format!(
                    "{} — {} requests, assertions {passed}/{total}, {} ms\n",
                    if report.passed() { "PASSED" } else { "FAILED" },
                    report.requests.len(),
                    report.total_ms
                );
                for r in &report.requests {
                    text.push_str(&format!(
                        "\n{} {} {} → {}",
                        if r.passed() { "✔" } else { "✘" },
                        r.method,
                        r.name,
                        r.status.map(|s| s.to_string()).or(r.error.clone()).unwrap_or_default()
                    ));
                    for a in &r.assertions {
                        text.push_str(&format!("\n    {} {} ({})", if a.passed { "✓" } else { "✗" }, a.name, a.message));
                    }
                }
                Ok(text)
            }
            "save_curl_request" if self.config.allow_write => {
                let mut req = codegen::parse_curl(str_arg(args, "curl").unwrap_or(""))?;
                if let Some(n) = str_arg(args, "name") {
                    req.name = n.to_string();
                }
                let cname = str_arg(args, "collection").unwrap_or("MCP");
                let mut c = Self::find_collection(&ws, cname).unwrap_or_else(|_| Collection::new(cname));
                c.items.push(Item::Request(req.clone()));
                ws.save_collection(&c)?;
                Ok(format!("Saved `{}` ({} {}) into collection `{}`", req.name, req.method, req.url, c.name))
            }
            other => anyhow::bail!("unknown tool `{other}`"),
        }
    }

    fn execute(
        &self,
        ws: &Workspace,
        req: &Request,
        env: Option<&Environment>,
        collection_vars: &[KeyValue],
    ) -> Result<String> {
        let mut scope =
            VarScope::new().with_environment(env).with_collection_vars(collection_vars).with_globals(&ws.globals);
        if let pakpos_core::Auth::OAuth2(cfg) = &req.auth {
            let token_url = cfg.resolved(&scope).token_url;
            if !http::host_allowed(&token_url, &self.config.allowed_hosts) {
                anyhow::bail!("OAuth 2.0 token URL `{token_url}` is not allowed");
            }
            let mut tokens = ws.tokens.clone();
            if pakpos_core::oauth2::ensure_token(&req.auth, &mut scope, &mut tokens, &self.settings())?.is_some() {
                let mut ws = ws.clone();
                ws.tokens = tokens;
                let _ = ws.save_tokens();
            }
        }
        let missing = scope.unresolved(&format!(
            "{} {}",
            req.url,
            req.params.iter().map(|p| p.value.as_str()).collect::<Vec<_>>().join(" ")
        ));
        if !missing.is_empty() {
            anyhow::bail!(
                "unresolved variables: {}. Pass `environment` (see list_environments) or activate one in PakPos.",
                missing.join(", ")
            );
        }
        let prepared = http::prepare(req, &scope, None)?;
        if !http::host_allowed(&prepared.url, &self.config.allowed_hosts) {
            anyhow::bail!(
                "host of `{}` is not allowed. Allowed: {}",
                prepared.url,
                self.config.allowed_hosts.join(", ")
            );
        }
        let client = http::client_for(&self.settings(), &prepared.url)?;
        let resp = http::send(&client, &prepared)?;
        let mut body = resp.pretty_body();
        if body.chars().count() > MAX_BODY_CHARS {
            body = body.chars().take(MAX_BODY_CHARS).collect::<String>() + "\n…[truncated]";
        }
        let headers: Vec<String> = resp.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        let tests = pakpos_core::assertions::run_all(&resp, &req.assertions, |s| scope.resolve(s));
        let mut out = format!(
            "{} {}\nHTTP {} {} · {} ms · {} bytes\n\n{}\n\n{}",
            prepared.method,
            prepared.url,
            resp.status,
            resp.status_text,
            resp.elapsed_ms,
            resp.size_bytes(),
            headers.join("\n"),
            body
        );
        if !tests.is_empty() {
            out.push_str("\n\nTests:");
            for t in tests {
                out.push_str(&format!("\n  {} {} ({})", if t.passed { "✓" } else { "✗" }, t.name, t.message));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(dir: &std::path::Path, hosts: Vec<String>) -> Server {
        Server::new(McpConfig {
            allowed_hosts: hosts,
            allow_write: true,
            allow_scripts: false,
            workspace_root: dir.to_path_buf(),
        })
    }

    #[test]
    fn initialize_and_list_tools() {
        let dir = std::env::temp_dir().join(format!("pakpos-mcp-{}", pakpos_core::model::new_id()));
        let mut s = server(&dir, vec![]);
        let init = s
            .handle(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}))
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(s.handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let tools = s.handle(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<_> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"send_request".to_string()));
        assert!(names.contains(&"save_curl_request".to_string()));
        let unknown = s.handle(json!({"jsonrpc":"2.0","id":3,"method":"nope"})).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn allow_list_blocks_and_write_works() {
        let dir = std::env::temp_dir().join(format!("pakpos-mcp-{}", pakpos_core::model::new_id()));
        let mut s = server(&dir, vec!["api.example.com".into()]);
        let r = s
            .handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send_request","arguments":{"method":"GET","url":"http://169.254.169.254/latest"}}}))
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("not allowed"));

        let r = s
            .handle(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"save_curl_request","arguments":{"collection":"Demo","curl":"curl https://api.example.com/users"}}}))
            .unwrap();
        assert_eq!(r["result"]["isError"], false, "{r}");
        let r = s
            .handle(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_requests","arguments":{"collection":"demo"}}}))
            .unwrap();
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("api.example.com/users"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
