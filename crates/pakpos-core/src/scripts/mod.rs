//! Sandboxed JavaScript pre-request and test scripts (QuickJS).
//!
//! Scripts get a Postman-compatible subset of `pm` (see `prelude.js`): tests
//! and `expect`, variables (environment / collection / globals / local),
//! the request (editable before sending) and the response. The context has
//! no file, process or network access, a memory cap and a time limit.
//!
//! Scripts from a collection only run when the user trusts it; see
//! [`crate::storage::Settings::trusted_collections`].

use crate::assertions::AssertionResult;
use crate::http::Response;
use crate::model::{Body, KeyValue, Request};
use crate::vars::VarScope;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PRELUDE: &str = include_str!("prelude.js");

/// Limits for one script run.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub timeout: Duration,
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(3), memory_bytes: 64 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    PreRequest,
    Test,
}

#[derive(Serialize)]
struct Header {
    key: String,
    value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    pub method: String,
    /// Full URL including the query string, unresolved.
    pub url: String,
    pub headers: Vec<ScriptHeader>,
    /// Raw body text (only for raw bodies).
    pub body: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ScriptHeader {
    pub key: String,
    pub value: String,
}

#[derive(Serialize)]
struct Input<'a> {
    request: &'a ScriptRequest,
    response: Option<ResponseIn>,
    environment: &'a HashMap<String, String>,
    collection: &'a HashMap<String, String>,
    globals: &'a HashMap<String, String>,
    variables: &'a HashMap<String, String>,
    data: &'a HashMap<String, String>,
    info: Info,
}

#[derive(Serialize)]
struct Info {
    #[serde(rename = "requestName")]
    request_name: String,
    #[serde(rename = "eventName")]
    event_name: &'static str,
    iteration: usize,
}

#[derive(Serialize)]
struct ResponseIn {
    code: u16,
    status: String,
    headers: Vec<Header>,
    body: String,
    time: u128,
    size: usize,
}

#[derive(Deserialize, Default)]
struct Output {
    #[serde(default)]
    tests: Vec<AssertionResult>,
    #[serde(default)]
    logs: Vec<String>,
    #[serde(default)]
    env: HashMap<String, Option<String>>,
    #[serde(default)]
    coll: HashMap<String, Option<String>>,
    #[serde(default)]
    glob: HashMap<String, Option<String>>,
    #[serde(default)]
    vars: HashMap<String, Option<String>>,
    request: Option<ScriptRequest>,
    error: Option<String>,
}

/// Variable changes made by a script: `None` means unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarChanges {
    pub environment: Vec<(String, Option<String>)>,
    pub collection: Vec<(String, Option<String>)>,
    pub globals: Vec<(String, Option<String>)>,
    pub local: Vec<(String, Option<String>)>,
}

impl VarChanges {
    pub fn is_empty(&self) -> bool {
        self.environment.is_empty() && self.collection.is_empty() && self.globals.is_empty() && self.local.is_empty()
    }

    /// Applies the changes to a scope so later requests see them.
    pub fn apply_to(&self, scope: &mut VarScope) {
        fn apply(map: &mut HashMap<String, String>, changes: &[(String, Option<String>)]) {
            for (k, v) in changes {
                match v {
                    Some(v) => {
                        map.insert(k.clone(), v.clone());
                    }
                    None => {
                        map.remove(k);
                    }
                }
            }
        }
        apply(&mut scope.environment, &self.environment);
        apply(&mut scope.collection, &self.collection);
        apply(&mut scope.globals, &self.globals);
        apply(&mut scope.runtime, &self.local);
    }

    /// Applies changes to a list of key/value rows (environment file etc.).
    pub fn apply_to_list(rows: &mut Vec<KeyValue>, changes: &[(String, Option<String>)]) {
        for (k, v) in changes {
            match v {
                Some(v) => match rows.iter_mut().find(|r| &r.key == k) {
                    Some(r) => {
                        r.value = v.clone();
                        r.enabled = true;
                    }
                    None => rows.push(KeyValue::new(k, v.clone())),
                },
                None => rows.retain(|r| &r.key != k),
            }
        }
    }

    pub fn merge(&mut self, other: VarChanges) {
        self.environment.extend(other.environment);
        self.collection.extend(other.collection);
        self.globals.extend(other.globals);
        self.local.extend(other.local);
    }
}

/// Result of running one script.
#[derive(Debug, Clone, Default)]
pub struct ScriptOutcome {
    pub tests: Vec<AssertionResult>,
    pub logs: Vec<String>,
    pub changes: VarChanges,
    /// The request as modified by a pre-request script.
    pub request: Option<ScriptRequest>,
    /// Syntax error, uncaught exception or timeout.
    pub error: Option<String>,
}

/// Snapshot of a request for scripts.
pub fn script_request(req: &Request) -> ScriptRequest {
    let q: Vec<String> = req
        .params
        .iter()
        .filter(|p| p.enabled && !p.is_blank())
        .map(|p| if p.value.is_empty() { p.key.clone() } else { format!("{}={}", p.key, p.value) })
        .collect();
    ScriptRequest {
        method: req.method.as_str().to_string(),
        url: if q.is_empty() { req.url.clone() } else { format!("{}?{}", req.url, q.join("&")) },
        headers: req
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.trim().is_empty())
            .map(|h| ScriptHeader { key: h.key.clone(), value: h.value.clone() })
            .collect(),
        body: match &req.body {
            Body::Raw { content, .. } => Some(content.clone()),
            Body::GraphQl { query, .. } => Some(query.clone()),
            _ => None,
        },
    }
}

/// Writes a script's request changes back into the PakPos request.
pub fn apply_request(req: &mut Request, changed: &ScriptRequest) {
    if let Some(m) = crate::model::HttpMethod::parse(&changed.method) {
        req.method = m;
    }
    let (base, query) = changed.url.split_once('?').unwrap_or((&changed.url, ""));
    req.url = base.to_string();
    let disabled: Vec<KeyValue> = req.params.iter().filter(|p| !p.enabled).cloned().collect();
    req.params = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => KeyValue::new(k, v),
            None => KeyValue::new(p, ""),
        })
        .chain(disabled)
        .collect();
    let disabled: Vec<KeyValue> = req.headers.iter().filter(|h| !h.enabled).cloned().collect();
    req.headers = changed.headers.iter().map(|h| KeyValue::new(&h.key, &h.value)).chain(disabled).collect();
    match (&mut req.body, &changed.body) {
        (Body::Raw { content, .. }, Some(b)) => *content = b.clone(),
        (Body::GraphQl { query, .. }, Some(b)) => *query = b.clone(),
        (body @ Body::None, Some(b)) if !b.is_empty() => {
            *body = Body::Raw { content: b.clone(), language: crate::model::RawLanguage::Text }
        }
        _ => {}
    }
}

fn pairs(m: HashMap<String, Option<String>>) -> Vec<(String, Option<String>)> {
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Runs one script.
pub fn run(
    source: &str,
    phase: Phase,
    request_name: &str,
    request: &ScriptRequest,
    response: Option<&Response>,
    scope: &VarScope,
    limits: Limits,
) -> ScriptOutcome {
    if source.trim().is_empty() {
        return ScriptOutcome::default();
    }
    let input = Input {
        request,
        response: response.map(|r| ResponseIn {
            code: r.status,
            status: r.status_text.clone(),
            headers: r.headers.iter().map(|(k, v)| Header { key: k.clone(), value: v.clone() }).collect(),
            body: r.text(),
            time: r.elapsed_ms,
            size: r.size_bytes(),
        }),
        environment: &scope.environment,
        collection: &scope.collection,
        globals: &scope.globals,
        variables: &scope.runtime,
        data: &scope.data,
        info: Info {
            iteration: scope.iteration,
            request_name: request_name.to_string(),
            event_name: match phase {
                Phase::PreRequest => "prerequest",
                Phase::Test => "test",
            },
        },
    };
    let input_json = match serde_json::to_string(&input) {
        Ok(j) => j,
        Err(e) => return ScriptOutcome { error: Some(e.to_string()), ..Default::default() },
    };
    match execute(source, &input_json, limits) {
        Ok(out) => {
            let mut tests = out.tests;
            ScriptOutcome {
                tests: std::mem::take(&mut tests),
                logs: out.logs,
                changes: VarChanges {
                    environment: pairs(out.env),
                    collection: pairs(out.coll),
                    globals: pairs(out.glob),
                    local: pairs(out.vars),
                },
                request: if phase == Phase::PreRequest { out.request } else { None },
                error: out.error,
            }
        }
        Err(e) => ScriptOutcome { error: Some(e), ..Default::default() },
    }
}

fn execute(source: &str, input_json: &str, limits: Limits) -> Result<Output, String> {
    use rquickjs::{Context, Runtime};
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(limits.memory_bytes);
    rt.set_max_stack_size(512 * 1024);
    let deadline = Instant::now() + limits.timeout;
    // Disarmed after the user script so results can still be read back.
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let armed_handler = armed.clone();
    rt.set_interrupt_handler(Some(Box::new(move || {
        armed_handler.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() > deadline
    })));
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    ctx.with(|ctx| {
        let describe = |ctx: &rquickjs::Ctx, e: rquickjs::Error| -> String {
            if let rquickjs::Error::Exception = e {
                let ex = ctx.catch();
                if let Some(obj) = ex.as_object() {
                    let msg: String = obj.get("message").unwrap_or_default();
                    let stack: String = obj.get("stack").unwrap_or_default();
                    if !msg.is_empty() {
                        return if stack.is_empty() { msg } else { format!("{msg}\n{stack}") };
                    }
                }
                if let Ok(s) = ex.get::<rquickjs::Coerced<String>>() {
                    return s.0;
                }
            }
            e.to_string()
        };
        ctx.globals().set("__PAKPOS_INPUT", input_json).map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(PRELUDE).map_err(|e| format!("prelude: {}", describe(&ctx, e)))?;
        // The user script runs in its own function scope; an uncaught error is
        // recorded instead of aborting so tests that already ran still count.
        let wrapped = format!(
            "try {{ (function () {{\n{source}\n}})(); }} catch (e) {{ __out.error = String((e && e.stack) ? (e.message + '\\n' + e.stack) : e); }}\n\
             for (const [k, v] of Object.entries(globalThis.tests || {{}})) __out.tests.push({{ name: k, passed: !!v, message: v ? 'passed' : 'failed' }});"
        );
        let result = ctx.eval::<(), _>(wrapped);
        armed.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = result {
            let msg = describe(&ctx, e);
            let timed_out = Instant::now() > deadline;
            // Interrupted or syntax error: still try to read what was collected.
            let partial: Option<String> = ctx.eval("JSON.stringify(__out)").ok();
            let mut out: Output = partial.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default();
            out.error = Some(if timed_out { format!("script timed out after {:?}", limits.timeout) } else { msg });
            return Ok(out);
        }
        let json: String = ctx.eval("JSON.stringify(__out)").map_err(|e| describe(&ctx, e))?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    })
}

/// Combined result of running a chain of scripts (collection → folders → request).
#[derive(Debug, Clone, Default)]
pub struct ChainResult {
    pub tests: Vec<AssertionResult>,
    pub logs: Vec<String>,
    pub changes: VarChanges,
    pub errors: Vec<String>,
}

impl ChainResult {
    fn absorb(&mut self, label: &str, out: ScriptOutcome, scope: &mut VarScope) {
        out.changes.apply_to(scope);
        self.tests.extend(out.tests);
        self.logs.extend(out.logs);
        self.changes.merge(out.changes);
        if let Some(e) = out.error {
            self.logs.push(format!("[error] {label}: {e}"));
            self.errors.push(format!("{label}: {}", e.lines().next().unwrap_or("")));
        }
    }

    /// Script errors reported as failed tests, so runs and CI notice them.
    pub fn error_results(&self) -> Vec<AssertionResult> {
        self.errors
            .iter()
            .map(|e| AssertionResult { name: "script error".into(), passed: false, message: e.clone() })
            .collect()
    }
}

/// The scripts that apply to a request, outermost first, including its own.
pub fn chain_for(collection: Option<&crate::model::Collection>, req: &Request) -> Vec<crate::model::Scripts> {
    let mut chain = collection.map(|c| c.script_chain(&req.id)).unwrap_or_default();
    if !req.scripts.is_empty() {
        chain.push(req.scripts.clone());
    }
    chain
}

/// Runs every pre-request script; edits `req` and `scope` in place.
pub fn run_pre_request(
    chain: &[crate::model::Scripts],
    req: &mut Request,
    scope: &mut VarScope,
    limits: Limits,
) -> ChainResult {
    let mut result = ChainResult::default();
    for s in chain.iter().filter(|s| !s.pre_request.trim().is_empty()) {
        let out = run(&s.pre_request, Phase::PreRequest, &req.name, &script_request(req), None, scope, limits);
        if let Some(changed) = &out.request {
            apply_request(req, changed);
        }
        result.absorb("pre-request script", out, scope);
    }
    result
}

/// Runs every test script against the response.
pub fn run_tests(
    chain: &[crate::model::Scripts],
    req: &Request,
    response: &Response,
    scope: &mut VarScope,
    limits: Limits,
) -> ChainResult {
    let mut result = ChainResult::default();
    let snapshot = script_request(req);
    for s in chain.iter().filter(|s| !s.test.trim().is_empty()) {
        let out = run(&s.test, Phase::Test, &req.name, &snapshot, Some(response), scope, limits);
        result.absorb("test script", out, scope);
    }
    result
}

#[cfg(test)]
mod tests;
