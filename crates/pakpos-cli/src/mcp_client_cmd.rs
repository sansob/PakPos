//! `pakpos-cli mcp-client`: talk to an MCP server from the terminal.

use crate::Style;
use anyhow::{Context, Result};
use pakpos_core::http::HttpSettings;
use pakpos_core::mcp_client::McpClient;
use serde_json::Value;
use std::process::ExitCode;

#[derive(clap::Args)]
pub struct McpClientArgs {
    /// Server: an http(s):// URL (Streamable HTTP) or a command line (stdio),
    /// e.g. "npx -y @modelcontextprotocol/server-everything".
    target: String,
    #[command(subcommand)]
    action: Option<McpAction>,
    /// HTTP header "Name: value" (repeatable), e.g. Authorization.
    #[arg(short = 'H', long = "header", global = true)]
    headers: Vec<String>,
    /// Environment variable for a stdio server: --env KEY=VALUE (repeatable).
    #[arg(long = "env", value_parser = crate::parse_kv, global = true)]
    env: Vec<(String, String)>,
    #[arg(long, default_value_t = 30, global = true)]
    timeout: u64,
}

#[derive(clap::Subcommand)]
enum McpAction {
    /// List the tools with their arguments (default).
    Tools,
    /// Call a tool: --args '{"city":"Jakarta"}'. Exit code 1 if the tool reports an error.
    Call {
        tool: String,
        #[arg(short, long, default_value = "{}")]
        args: String,
    },
    /// List the resources.
    Resources,
    /// Read a resource by URI.
    Read { uri: String },
    /// List the prompts.
    Prompts,
    /// Get a prompt: --args '{"name":"value"}'.
    Prompt {
        name: String,
        #[arg(short, long, default_value = "{}")]
        args: String,
    },
}

fn parse_args(s: &str) -> Result<Value> {
    let s = s.strip_prefix('@').map(std::fs::read_to_string).transpose()?.unwrap_or_else(|| s.to_string());
    serde_json::from_str(&s).context("--args must be a JSON object")
}

pub fn run(a: McpClientArgs, style: &Style) -> Result<ExitCode> {
    let headers = a
        .headers
        .iter()
        .map(|h| {
            h.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .with_context(|| format!("header must be `Name: value`, got `{h}`"))
        })
        .collect::<Result<Vec<_>>>()?;
    let settings = HttpSettings { timeout_secs: a.timeout, ..Default::default() };
    let mut client = McpClient::connect(&a.target, &headers, &a.env, &settings)?;
    let s = &client.server;
    eprintln!(
        "{} {} {}",
        style.green("connected"),
        style.bold(&format!("{} {}", s.name, s.version)),
        style.dim(&format!("(protocol {})", s.protocol_version))
    );
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap_or_default();
    match a.action.unwrap_or(McpAction::Tools) {
        McpAction::Tools => {
            for t in client.list("tools")? {
                let name = t.get("name").and_then(Value::as_str).unwrap_or("");
                let desc = t.get("description").and_then(Value::as_str).unwrap_or("").lines().next().unwrap_or("");
                let args: Vec<String> = t
                    .pointer("/inputSchema/properties")
                    .and_then(Value::as_object)
                    .map(|p| p.keys().cloned().collect())
                    .unwrap_or_default();
                println!("{}({})  {}", style.bold(name), args.join(", "), style.dim(desc));
            }
        }
        McpAction::Call { tool, args } => {
            let result = client.call_tool(&tool, parse_args(&args)?)?;
            let is_error = result.get("isError").and_then(Value::as_bool) == Some(true);
            for c in result.get("content").and_then(Value::as_array).cloned().unwrap_or_default() {
                match c.get("type").and_then(Value::as_str) {
                    Some("text") => println!("{}", c.get("text").and_then(Value::as_str).unwrap_or("")),
                    _ => println!("{}", pretty(&c)),
                }
            }
            if let Some(sc) = result.get("structuredContent") {
                println!("{}", pretty(sc));
            }
            if is_error {
                eprintln!("{}", style.red("the tool reported an error"));
                return Ok(ExitCode::from(1));
            }
        }
        McpAction::Resources => {
            for r in client.list("resources")? {
                println!(
                    "{}  {}",
                    style.bold(r.get("uri").and_then(Value::as_str).unwrap_or("")),
                    style.dim(r.get("name").and_then(Value::as_str).unwrap_or(""))
                );
            }
        }
        McpAction::Read { uri } => println!("{}", pretty(&client.read_resource(&uri)?)),
        McpAction::Prompts => {
            for p in client.list("prompts")? {
                println!(
                    "{}  {}",
                    style.bold(p.get("name").and_then(Value::as_str).unwrap_or("")),
                    style.dim(p.get("description").and_then(Value::as_str).unwrap_or(""))
                );
            }
        }
        McpAction::Prompt { name, args } => println!("{}", pretty(&client.get_prompt(&name, parse_args(&args)?)?)),
    }
    Ok(ExitCode::SUCCESS)
}
