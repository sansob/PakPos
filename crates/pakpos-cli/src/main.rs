//! `pakpos-cli`: run PakPos / Postman collections from the terminal or CI.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use pakpos_core::codegen::{self, SnippetLang};
use pakpos_core::http::{self, format_size, HttpSettings};
use pakpos_core::runner::{run_collection, RequestReport, RunOptions};
use pakpos_core::storage::{self, Workspace};
use pakpos_core::vars::VarScope;
use pakpos_core::{postman, Body, HttpMethod, KeyValue, Request};
use std::io::IsTerminal;

mod mcp;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "pakpos-cli",
    version,
    about = "PakPos command-line runner: send requests and run collections (PakPos or Postman format).",
    long_about = None
)]
struct Cli {
    /// Disable colored output (also honours NO_COLOR).
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every request of a collection and its tests.
    Run(RunArgs),
    /// Send a single request, like curl but with PakPos variables.
    Send(SendArgs),
    /// Convert a Postman collection into PakPos format.
    Import {
        /// Postman collection JSON (v2.0 / v2.1).
        input: PathBuf,
        /// Output file. Defaults to stdout.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Add it to the PakPos workspace (shows up in the desktop app).
        #[arg(long)]
        workspace: bool,
    },
    /// Export a PakPos collection as Postman v2.1.
    Export {
        input: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Convert a cURL command into a PakPos request (JSON) or a code snippet.
    Curl {
        /// The full curl command, quoted.
        command: String,
        /// Output a snippet in another language instead of JSON.
        #[arg(long, value_enum)]
        to: Option<Lang>,
    },
    /// Open a WebSocket: lines from stdin are sent, messages are printed.
    Ws {
        /// ws:// or wss:// URL (http(s):// is converted), may contain {{variables}}.
        url: String,
        /// Header "Name: value" (repeatable), e.g. for Authorization.
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        #[arg(short, long)]
        environment: Option<String>,
        #[arg(long = "var", value_parser = parse_kv)]
        vars: Vec<(String, String)>,
        /// Send this message after connecting (repeatable), then keep reading.
        #[arg(short, long)]
        send: Vec<String>,
        /// Exit after this many seconds (default: run until stdin closes / Ctrl+C).
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Print the workspace location and its contents.
    Workspace,
    /// Start a Model Context Protocol (MCP) server on stdio, so AI agents
    /// (Claude Desktop, Cursor, VS Code, ...) can use your collections.
    Mcp {
        /// Only allow requests to these hosts (repeatable, globs allowed:
        /// `*.example.com`, `localhost:8080`). Strongly recommended.
        #[arg(long = "allow-host")]
        allow_hosts: Vec<String>,
        /// Allow saving new requests into the workspace.
        #[arg(long)]
        allow_write: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Lang {
    Curl,
    Python,
    Javascript,
    Httpie,
}

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Reporter {
    Cli,
    Json,
    Junit,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Collection file (PakPos or Postman JSON), or the name of a collection in the workspace.
    collection: String,
    /// Environment file, or the name of an environment in the workspace.
    #[arg(short, long)]
    environment: Option<String>,
    /// Override a variable: --var key=value (repeatable).
    #[arg(long = "var", value_parser = parse_kv)]
    vars: Vec<(String, String)>,
    /// Only run requests whose folder/name contains this text.
    #[arg(long)]
    filter: Option<String>,
    /// Stop at the first failing request.
    #[arg(long)]
    bail: bool,
    /// Delay between requests in milliseconds.
    #[arg(long, default_value_t = 0)]
    delay: u64,
    /// Request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    /// Do not verify TLS certificates.
    #[arg(short = 'k', long)]
    insecure: bool,
    /// Output format.
    #[arg(short, long, value_enum, default_value_t = Reporter::Cli)]
    reporter: Reporter,
    /// Write the report to a file instead of stdout (json / junit).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Write extracted variables back into the environment file.
    #[arg(long)]
    save_env: bool,
}

#[derive(clap::Args)]
struct SendArgs {
    /// HTTP method (GET, POST, ...).
    method: String,
    /// URL, may contain {{variables}}.
    url: String,
    /// Header "Name: value" (repeatable).
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,
    /// Raw request body. Use @file to read it from a file.
    #[arg(short, long)]
    data: Option<String>,
    /// Environment file or workspace environment name.
    #[arg(short, long)]
    environment: Option<String>,
    #[arg(long = "var", value_parser = parse_kv)]
    vars: Vec<(String, String)>,
    /// Print response headers too.
    #[arg(short, long)]
    include: bool,
    #[arg(short = 'k', long)]
    insecure: bool,
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| format!("expected key=value, got `{s}`"))
}

struct Style {
    color: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    fn status(&self, code: u16) -> String {
        let c = match code {
            200..=299 => "32",
            300..=399 => "36",
            400..=499 => "33",
            _ => "31",
        };
        self.paint(c, &code.to_string())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style =
        Style { color: !cli.no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal() };
    match run(cli, &style) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e:#}", style.red("error:"));
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli, style: &Style) -> Result<ExitCode> {
    match cli.command {
        Command::Run(args) => cmd_run(args, style),
        Command::Send(args) => cmd_send(args, style),
        Command::Import { input, out, workspace } => {
            let (collection, warnings) = storage::load_collection_file(&input).map_err(anyhow::Error::msg)?;
            for w in &warnings {
                eprintln!("{} {w}", style.paint("33", "warning:"));
            }
            if workspace {
                let (ws, _) = Workspace::load(storage::default_root());
                ws.save_collection(&collection)?;
                eprintln!(
                    "Imported `{}` ({} requests) into {}",
                    collection.name,
                    collection.request_count(),
                    ws.collections_dir().display()
                );
            }
            let json = serde_json::to_string_pretty(&collection)?;
            write_output(out.as_ref(), &json, !workspace)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Export { input, out } => {
            let (collection, _) = storage::load_collection_file(&input).map_err(anyhow::Error::msg)?;
            write_output(out.as_ref(), &postman::export_collection(&collection), true)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Curl { command, to } => {
            let req = codegen::parse_curl(&command)?;
            match to {
                None => println!("{}", serde_json::to_string_pretty(&req)?),
                Some(lang) => {
                    let prepared = http::prepare(&req, &VarScope::new(), None)?;
                    let lang = match lang {
                        Lang::Curl => SnippetLang::Curl,
                        Lang::Python => SnippetLang::Python,
                        Lang::Javascript => SnippetLang::JavaScript,
                        Lang::Httpie => SnippetLang::Httpie,
                    };
                    println!("{}", codegen::snippet(lang, &prepared));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Mcp { allow_hosts, allow_write } => {
            mcp::serve(mcp::McpConfig {
                allowed_hosts: allow_hosts,
                allow_write,
                workspace_root: storage::default_root(),
            })?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Ws { url, headers, environment, vars, send, timeout } => {
            cmd_ws(url, headers, environment, vars, send, timeout, style)
        }
        Command::Workspace => {
            let (ws, errors) = Workspace::load(storage::default_root());
            println!("{} {}", style.bold("Workspace:"), ws.root.display());
            println!("{}", style.bold("Collections:"));
            for c in &ws.collections {
                println!("  {} {}", c.name, style.dim(&format!("({} requests)", c.request_count())));
            }
            println!("{}", style.bold("Environments:"));
            for e in &ws.environments {
                let active = ws.settings.active_environment.as_deref() == Some(e.id.as_str());
                println!("  {}{}", e.name, if active { style.green(" (active)") } else { String::new() });
            }
            println!("{} {}", style.bold("History entries:"), ws.history.len());
            for e in errors {
                eprintln!("{} {e}", style.paint("33", "warning:"));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn write_output(out: Option<&PathBuf>, text: &str, stdout_fallback: bool) -> Result<()> {
    match out {
        Some(path) => std::fs::write(path, text).with_context(|| format!("writing {}", path.display())),
        None => {
            if stdout_fallback {
                println!("{text}");
            }
            Ok(())
        }
    }
}

/// Resolves an environment argument: a file path, or a workspace env name.
fn load_env(arg: &str) -> Result<(pakpos_core::Environment, Option<PathBuf>)> {
    let path = PathBuf::from(arg);
    if path.exists() {
        return Ok((storage::load_environment_file(&path).map_err(anyhow::Error::msg)?, Some(path)));
    }
    let (ws, _) = Workspace::load(storage::default_root());
    let env = ws
        .environments
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(arg))
        .cloned()
        .with_context(|| format!("environment `{arg}` not found (not a file, not in workspace)"))?;
    Ok((env, None))
}

fn cmd_run(args: RunArgs, style: &Style) -> Result<ExitCode> {
    let path = PathBuf::from(&args.collection);
    let (collection, warnings) = if path.exists() {
        storage::load_collection_file(&path).map_err(anyhow::Error::msg)?
    } else {
        let (ws, _) = Workspace::load(storage::default_root());
        let c = ws
            .collections
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&args.collection))
            .cloned()
            .with_context(|| format!("collection `{}` not found", args.collection))?;
        (c, Vec::new())
    };
    for w in &warnings {
        eprintln!("{} {w}", style.paint("33", "warning:"));
    }

    let mut env = match &args.environment {
        Some(e) => Some(load_env(e)?),
        None => None,
    };
    let (mut ws, _) = Workspace::load(storage::default_root());
    let options = RunOptions {
        settings: HttpSettings { timeout_secs: args.timeout, verify_tls: !args.insecure, ..Default::default() },
        filter: args.filter.clone(),
        stop_on_failure: args.bail,
        delay_ms: args.delay,
        globals: ws.globals.clone(),
        overrides: args.vars.clone(),
        allowed_hosts: Vec::new(),
        tokens: ws.tokens.clone(),
    };

    let cli_reporter = args.reporter == Reporter::Cli;
    if cli_reporter {
        println!(
            "\n{} {}{}\n",
            style.bold("PakPos ▸"),
            style.bold(&collection.name),
            env.as_ref().map(|(e, _)| style.dim(&format!("  ·  env: {}", e.name))).unwrap_or_default()
        );
    }
    let print = |r: &RequestReport| {
        if !cli_reporter {
            return;
        }
        let folder =
            if r.folder.is_empty() { String::new() } else { style.dim(&format!("{} / ", r.folder.join(" / "))) };
        let mark = if r.passed() { style.green("✔") } else { style.red("✘") };
        let meta = match (r.status, r.elapsed_ms, r.size_bytes) {
            (Some(s), Some(t), Some(b)) => {
                format!("{} {}", style.status(s), style.dim(&format!("{t} ms · {}", format_size(b))))
            }
            _ => style.red("no response"),
        };
        println!("{mark} {folder}{}  {} {}", style.bold(&r.name), style.dim(&r.method), meta);
        println!("    {}", style.dim(&r.url));
        if let Some(e) = &r.error {
            println!("    {} {e}", style.red("error"));
        }
        for a in &r.assertions {
            if a.passed {
                println!("    {} {}", style.green("✓"), a.name);
            } else {
                println!("    {} {} {}", style.red("✗"), a.name, style.dim(&format!("({})", a.message)));
            }
        }
        for (k, v) in &r.extracted {
            println!("    {} {{{{{k}}}}} = {}", style.dim("↳"), style.dim(&truncate(v, 60)));
        }
    };

    let report = run_collection(&collection, env.as_mut().map(|(e, _)| e), &options, print);
    // Reuse OAuth 2.0 tokens across runs, but only in an existing workspace
    // (CI runs without one stay stateless).
    if report.tokens != ws.tokens && ws.root.exists() {
        ws.tokens = report.tokens.clone();
        let _ = ws.save_tokens();
    }

    match args.reporter {
        Reporter::Cli => {
            let (passed, total) = report.assertion_counts();
            let failed_reqs = report.failed_requests();
            println!(
                "\n{}  requests: {}  ·  assertions: {}/{}  ·  {} ms\n",
                if report.passed() { style.green("PASSED") } else { style.red("FAILED") },
                report.requests.len(),
                passed,
                total,
                report.total_ms
            );
            if failed_reqs > 0 {
                println!("{} request(s) failed", style.red(&failed_reqs.to_string()));
            }
        }
        Reporter::Json => write_output(args.out.as_ref(), &serde_json::to_string_pretty(&report)?, true)?,
        Reporter::Junit => write_output(args.out.as_ref(), &report.to_junit(), true)?,
    }

    if args.save_env {
        match &env {
            Some((e, Some(path))) => {
                std::fs::write(path, serde_json::to_string_pretty(e)?)?;
            }
            Some((e, None)) => {
                let (ws, _) = Workspace::load(storage::default_root());
                ws.save_environment(e)?;
            }
            None => bail!("--save-env needs --environment"),
        }
    }

    Ok(if report.passed() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

fn cmd_send(args: SendArgs, style: &Style) -> Result<ExitCode> {
    let method = HttpMethod::parse(&args.method).with_context(|| format!("unknown method `{}`", args.method))?;
    let mut req = Request::new(method, &args.url);
    for h in &args.headers {
        let (k, v) = h.split_once(':').with_context(|| format!("header must be `Name: value`, got `{h}`"))?;
        req.headers.push(KeyValue::new(k.trim(), v.trim()));
    }
    if let Some(data) = &args.data {
        let content = match data.strip_prefix('@') {
            Some(file) => std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?,
            None => data.clone(),
        };
        let language = if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
            pakpos_core::model::RawLanguage::Json
        } else {
            pakpos_core::model::RawLanguage::Text
        };
        req.body = Body::Raw { content, language };
    }
    let env = match &args.environment {
        Some(e) => Some(load_env(e)?.0),
        None => None,
    };
    let (ws, _) = Workspace::load(storage::default_root());
    let mut scope = VarScope::new().with_environment(env.as_ref()).with_globals(&ws.globals);
    for (k, v) in args.vars {
        scope.runtime.insert(k, v);
    }
    let missing = scope.unresolved(&req.url);
    if !missing.is_empty() {
        eprintln!("{} unresolved variables: {}", style.paint("33", "warning:"), missing.join(", "));
    }
    let settings = HttpSettings { timeout_secs: args.timeout, verify_tls: !args.insecure, ..Default::default() };
    let (_, resp) = http::execute(&req, &scope, None, &settings)?;
    eprintln!(
        "{} {}  {}",
        style.status(resp.status),
        resp.status_text,
        style.dim(&format!("{} ms · {}", resp.elapsed_ms, format_size(resp.size_bytes())))
    );
    if args.include {
        for (k, v) in &resp.headers {
            eprintln!("{}: {v}", style.dim(k));
        }
        eprintln!();
    }
    println!("{}", resp.pretty_body());
    Ok(if resp.status < 400 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn cmd_ws(
    url: String,
    headers: Vec<String>,
    environment: Option<String>,
    vars: Vec<(String, String)>,
    send: Vec<String>,
    timeout: Option<u64>,
    style: &Style,
) -> Result<ExitCode> {
    use pakpos_core::ws::{Direction, WsEvent};
    let mut req = Request::new(HttpMethod::Get, &url);
    for h in &headers {
        let (k, v) = h.split_once(':').with_context(|| format!("header must be `Name: value`, got `{h}`"))?;
        req.headers.push(KeyValue::new(k.trim(), v.trim()));
    }
    let env = match &environment {
        Some(e) => Some(load_env(e)?.0),
        None => None,
    };
    let (ws, _) = Workspace::load(storage::default_root());
    let mut scope = VarScope::new().with_environment(env.as_ref()).with_globals(&ws.globals);
    for (k, v) in vars {
        scope.runtime.insert(k, v);
    }
    let prepared = http::prepare(&req, &scope, None)?;
    let handle = pakpos_core::ws::connect(&prepared, || {});
    // Forward stdin lines as messages.
    let tx = handle.tx.clone();
    let stdin_scope = scope.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let _ = tx.send(pakpos_core::ws::WsCommand::Send(stdin_scope.resolve(&line)));
        }
    });
    let deadline = timeout.map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s));
    let mut pending = send.into_iter().map(|m| scope.resolve(&m)).collect::<Vec<_>>();
    loop {
        if deadline.is_some_and(|d| std::time::Instant::now() > d) {
            handle.close();
            return Ok(ExitCode::SUCCESS);
        }
        match handle.rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(WsEvent::Connected { status, .. }) => {
                eprintln!(
                    "{} {}",
                    style.green("connected"),
                    style.dim(&format!("HTTP {status} · {}", pakpos_core::ws::to_ws_url(&prepared.url)))
                );
                for m in pending.drain(..) {
                    handle.send(m);
                }
            }
            Ok(WsEvent::Message { direction: Direction::Sent, text, .. }) => {
                eprintln!("{} {text}", style.paint("33", "↑"))
            }
            Ok(WsEvent::Message { direction: Direction::Received, text, .. }) => println!("{text}"),
            Ok(WsEvent::Error(e)) => eprintln!("{} {e}", style.red("error:")),
            Ok(WsEvent::Closed { reason }) => {
                eprintln!("{} {reason}", style.dim("closed:"));
                return Ok(ExitCode::SUCCESS);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(ExitCode::from(1)),
        }
    }
}
