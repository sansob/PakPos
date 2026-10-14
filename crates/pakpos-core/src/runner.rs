//! Collection runner, shared by the CLI (`pakpos-cli run`) and the GUI.

use crate::assertions::{self, AssertionResult};
use crate::http::{self, HttpSettings};
use crate::model::{Collection, Environment, KeyValue};
use crate::vars::VarScope;
use serde::Serialize;

/// Result of one request inside a run.
#[derive(Debug, Clone, Serialize)]
pub struct RequestReport {
    pub name: String,
    pub folder: Vec<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
    pub size_bytes: Option<usize>,
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
    pub extracted: Vec<(String, String)>,
    /// `console.log` output and script notices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

impl RequestReport {
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.assertions.iter().all(|a| a.passed)
    }
}

/// Summary of a full run.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RunReport {
    pub collection: String,
    pub environment: Option<String>,
    pub requests: Vec<RequestReport>,
    pub total_ms: u128,
    /// Token store after the run (new / refreshed OAuth 2.0 tokens), for the caller to persist.
    #[serde(skip)]
    pub tokens: crate::oauth2::TokenStore,
    /// Variable changes made by scripts (collection / global ones are for the caller to persist).
    #[serde(skip)]
    pub script_changes: crate::scripts::VarChanges,
}

impl RunReport {
    pub fn passed(&self) -> bool {
        self.requests.iter().all(RequestReport::passed)
    }

    pub fn assertion_counts(&self) -> (usize, usize) {
        let all = self.requests.iter().flat_map(|r| r.assertions.iter());
        let total = all.clone().count();
        let passed = all.filter(|a| a.passed).count();
        (passed, total)
    }

    pub fn failed_requests(&self) -> usize {
        self.requests.iter().filter(|r| !r.passed()).count()
    }

    /// JUnit XML, for CI systems (GitHub Actions, GitLab, Jenkins).
    pub fn to_junit(&self) -> String {
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
        }
        let mut cases = String::new();
        let mut tests = 0;
        let mut failures = 0;
        for r in &self.requests {
            let class = esc(&if r.folder.is_empty() {
                self.collection.clone()
            } else {
                format!("{}.{}", self.collection, r.folder.join("."))
            });
            let time = r.elapsed_ms.unwrap_or(0) as f64 / 1000.0;
            if let Some(err) = &r.error {
                tests += 1;
                failures += 1;
                cases.push_str(&format!(
                    "    <testcase classname=\"{class}\" name=\"{}\" time=\"{time:.3}\"><failure message=\"{}\"/></testcase>\n",
                    esc(&r.name),
                    esc(err)
                ));
                continue;
            }
            if r.assertions.is_empty() {
                tests += 1;
                cases.push_str(&format!(
                    "    <testcase classname=\"{class}\" name=\"{}\" time=\"{time:.3}\"/>\n",
                    esc(&r.name)
                ));
            }
            for a in &r.assertions {
                tests += 1;
                let name = esc(&format!("{} › {}", r.name, a.name));
                if a.passed {
                    cases.push_str(&format!(
                        "    <testcase classname=\"{class}\" name=\"{name}\" time=\"{time:.3}\"/>\n"
                    ));
                } else {
                    failures += 1;
                    cases.push_str(&format!(
                        "    <testcase classname=\"{class}\" name=\"{name}\" time=\"{time:.3}\"><failure message=\"{}\"/></testcase>\n",
                        esc(&a.message)
                    ));
                }
            }
        }
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n  <testsuite name=\"{}\" tests=\"{tests}\" failures=\"{failures}\" time=\"{:.3}\">\n{cases}  </testsuite>\n</testsuites>\n",
            esc(&self.collection),
            self.total_ms as f64 / 1000.0
        )
    }
}

/// Options for a run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub settings: HttpSettings,
    /// Only run requests whose name or folder contains this text.
    pub filter: Option<String>,
    pub stop_on_failure: bool,
    /// Delay between requests.
    pub delay_ms: u64,
    pub globals: Vec<KeyValue>,
    /// Extra `--var key=value` overrides.
    pub overrides: Vec<(String, String)>,
    /// If non-empty, only these hosts may be contacted (see [`http::host_allowed`]).
    pub allowed_hosts: Vec<String>,
    /// Known OAuth 2.0 tokens (e.g. the workspace's `tokens.json`).
    pub tokens: crate::oauth2::TokenStore,
    /// Run JavaScript scripts (only for trusted collections).
    pub allow_scripts: bool,
}

/// Runs every request in `collection` sequentially.
///
/// Extracted values are stored as runtime variables (visible to later
/// requests) and, if an environment is given, written into it so the caller
/// can persist them.
pub fn run_collection(
    collection: &Collection,
    mut environment: Option<&mut Environment>,
    options: &RunOptions,
    mut on_progress: impl FnMut(&RequestReport),
) -> RunReport {
    let started = std::time::Instant::now();
    let mut scope = VarScope::new()
        .with_environment(environment.as_deref())
        .with_collection_vars(&collection.variables)
        .with_globals(&options.globals);
    for (k, v) in &options.overrides {
        scope.runtime.insert(k.clone(), v.clone());
    }

    let client = http::build_client(&options.settings);
    let mut report = RunReport {
        collection: collection.name.clone(),
        environment: environment.as_ref().map(|e| e.name.clone()),
        tokens: options.tokens.clone(),
        ..Default::default()
    };

    let filter = options.filter.as_ref().map(|f| f.to_lowercase());
    for (folder, mut req) in collection.flatten() {
        // Only HTTP requests are runnable in a sequential test run.
        if !req.kind.is_http() {
            continue;
        }
        if let Some(f) = &filter {
            let hay = format!("{} {}", folder.join("/"), req.name).to_lowercase();
            if !hay.contains(f) {
                continue;
            }
        }
        if options.delay_ms > 0 && !report.requests.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(options.delay_ms));
        }
        let mut entry = RequestReport {
            name: req.name.clone(),
            folder: folder.clone(),
            method: req.method.to_string(),
            url: scope.resolve(&req.url),
            status: None,
            elapsed_ms: None,
            size_bytes: None,
            error: None,
            assertions: Vec::new(),
            extracted: Vec::new(),
            logs: Vec::new(),
        };
        let chain = crate::scripts::chain_for(Some(collection), &req);
        let run_scripts = !chain.is_empty() && options.allow_scripts;
        if !chain.is_empty() && !options.allow_scripts {
            entry.logs.push("scripts skipped: the collection is not trusted (pakpos-cli run --allow-scripts)".into());
        }
        let mut script_failures = Vec::new();
        if run_scripts {
            let pre = crate::scripts::run_pre_request(&chain, &mut req, &mut scope, crate::scripts::Limits::default());
            persist_env(environment.as_deref_mut(), &pre.changes);
            script_failures.extend(pre.error_results());
            entry.logs.extend(pre.logs.iter().cloned());
            report.script_changes.merge(pre.changes);
        }
        // The token endpoint is a host too: keep it inside the allow-list.
        let token = match &req.auth {
            crate::model::Auth::OAuth2(c)
                if !http::host_allowed(&c.resolved(&scope).token_url, &options.allowed_hosts) =>
            {
                Err(format!("OAuth 2.0 token URL `{}` is not in the allow-list", c.resolved(&scope).token_url))
            }
            _ => crate::oauth2::ensure_token(&req.auth, &mut scope, &mut report.tokens, &options.settings)
                .map_err(|e| e.to_string()),
        };
        let outcome = match (&client, token) {
            (_, Err(e)) => Err(http::HttpError::Transport(e)),
            (Ok(client), Ok(_)) => http::prepare(&req, &scope, None).and_then(|p| {
                if !http::host_allowed(&p.url, &options.allowed_hosts) {
                    return Err(http::HttpError::Transport(format!("host of `{}` is not in the allow-list", p.url)));
                }
                http::send(client, &p).map(|r| (p, r))
            }),
            (Err(e), _) => Err(http::HttpError::Transport(e.to_string())),
        };
        match outcome {
            Ok((prepared, resp)) => {
                entry.url = prepared.url;
                entry.status = Some(resp.status);
                entry.elapsed_ms = Some(resp.elapsed_ms);
                entry.size_bytes = Some(resp.size_bytes());
                entry.assertions = assertions::run_all(&resp, &req.assertions, |s| scope.resolve(s));
                entry.extracted = assertions::extract_all(&resp, &req.extractions);
                for (k, v) in &entry.extracted {
                    scope.runtime.insert(k.clone(), v.clone());
                    if let Some(env) = environment.as_deref_mut() {
                        env.set(k, v.clone());
                    }
                }
                if run_scripts {
                    let t =
                        crate::scripts::run_tests(&chain, &req, &resp, &mut scope, crate::scripts::Limits::default());
                    persist_env(environment.as_deref_mut(), &t.changes);
                    entry.assertions.extend(t.tests.iter().cloned());
                    script_failures.extend(t.error_results());
                    entry.logs.extend(t.logs.iter().cloned());
                    report.script_changes.merge(t.changes);
                }
                entry.assertions.extend(script_failures);
            }
            Err(e) => entry.error = Some(e.to_string()),
        }
        on_progress(&entry);
        let failed = !entry.passed();
        report.requests.push(entry);
        if failed && options.stop_on_failure {
            break;
        }
    }
    report.total_ms = started.elapsed().as_millis();
    report
}

/// Writes script environment changes into the environment being run.
fn persist_env(env: Option<&mut Environment>, changes: &crate::scripts::VarChanges) {
    if let Some(env) = env {
        crate::scripts::VarChanges::apply_to_list(&mut env.variables, &changes.environment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junit_escapes_and_counts() {
        let report = RunReport {
            collection: "C<1>".into(),
            environment: None,
            requests: vec![
                RequestReport {
                    name: "a".into(),
                    folder: vec![],
                    method: "GET".into(),
                    url: "http://x".into(),
                    status: Some(200),
                    elapsed_ms: Some(10),
                    size_bytes: Some(1),
                    error: None,
                    assertions: vec![
                        AssertionResult { name: "s".into(), passed: true, message: "passed".into() },
                        AssertionResult { name: "t".into(), passed: false, message: "\"bad\"".into() },
                    ],
                    extracted: vec![],
                    logs: vec![],
                },
                RequestReport {
                    name: "b".into(),
                    folder: vec!["F".into()],
                    method: "GET".into(),
                    url: "http://y".into(),
                    status: None,
                    elapsed_ms: None,
                    size_bytes: None,
                    error: Some("boom".into()),
                    assertions: vec![],
                    extracted: vec![],
                    logs: vec![],
                },
            ],
            total_ms: 20,
            tokens: Default::default(),
            script_changes: Default::default(),
        };
        let xml = report.to_junit();
        assert!(xml.contains("tests=\"3\" failures=\"2\""));
        assert!(xml.contains("C&lt;1&gt;"));
        assert!(xml.contains("&quot;bad&quot;"));
        assert_eq!(report.assertion_counts(), (1, 2));
        assert_eq!(report.failed_requests(), 2);
        assert!(!report.passed());
    }
}
