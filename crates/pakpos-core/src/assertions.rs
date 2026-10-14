//! Declarative response tests and value extraction.
//!
//! Postman uses JavaScript for tests. PakPos deliberately uses a small,
//! declarative language instead: it is safe, fast, diff-friendly and good
//! enough for the vast majority of API checks.
//!
//! A *target* selects a value from the response:
//!
//! | target                 | value                                  |
//! |------------------------|----------------------------------------|
//! | `status`               | status code, e.g. `200`                |
//! | `time`                 | response time in milliseconds          |
//! | `size`                 | body size in bytes                     |
//! | `body`                 | whole body as text                     |
//! | `header.<Name>`        | a response header (case-insensitive)   |
//! | `json.<path>`          | JSON value, e.g. `json.data.items[0].id` |

use crate::http::Response;
use crate::model::{Assertion, Extraction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    #[default]
    Equals,
    NotEquals,
    Contains,
    NotContains,
    LessThan,
    GreaterThan,
    Exists,
    NotExists,
    Matches,
}

impl Operator {
    pub const ALL: [Operator; 9] = [
        Operator::Equals,
        Operator::NotEquals,
        Operator::Contains,
        Operator::NotContains,
        Operator::LessThan,
        Operator::GreaterThan,
        Operator::Exists,
        Operator::NotExists,
        Operator::Matches,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Operator::Equals => "equals",
            Operator::NotEquals => "not equals",
            Operator::Contains => "contains",
            Operator::NotContains => "not contains",
            Operator::LessThan => "less than",
            Operator::GreaterThan => "greater than",
            Operator::Exists => "exists",
            Operator::NotExists => "not exists",
            Operator::Matches => "matches (glob)",
        }
    }

    pub fn needs_expected(&self) -> bool {
        !matches!(self, Operator::Exists | Operator::NotExists)
    }
}

/// Outcome of a single assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Navigates a JSON value with a path like `data.items[0].name` or `[2].id`.
pub fn json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    let path = path.trim().trim_start_matches('$').trim_start_matches('.');
    if path.is_empty() {
        return Some(current);
    }
    for segment in path.split('.') {
        let mut seg = segment;
        // Field name before any [index].
        let field_end = seg.find('[').unwrap_or(seg.len());
        let field = &seg[..field_end];
        if !field.is_empty() {
            current = current.get(field)?;
        }
        seg = &seg[field_end..];
        while let Some(stripped) = seg.strip_prefix('[') {
            let close = stripped.find(']')?;
            let idx_str = stripped[..close].trim().trim_matches('"').trim_matches('\'');
            current = match idx_str.parse::<usize>() {
                Ok(i) => current.get(i)?,
                Err(_) => current.get(idx_str)?,
            };
            seg = &stripped[close + 1..];
        }
    }
    Some(current)
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Extracts the value selected by `target` as a string, or `None` if absent.
pub fn select(response: &Response, target: &str) -> Option<String> {
    let target = target.trim();
    match target {
        "status" => Some(response.status.to_string()),
        "time" => Some(response.elapsed_ms.to_string()),
        "size" => Some(response.size_bytes().to_string()),
        "body" => Some(response.text()),
        _ => {
            if let Some(name) = target.strip_prefix("header.") {
                response.header(name).map(str::to_string)
            } else if let Some(path) = target.strip_prefix("json") {
                let json = response.json()?;
                json_path(&json, path).map(value_to_string)
            } else {
                None
            }
        }
    }
}

/// Simple glob: `*` matches any run of characters, `?` a single one.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn compare_numbers(actual: &str, expected: &str) -> Option<std::cmp::Ordering> {
    let a: f64 = actual.trim().parse().ok()?;
    let e: f64 = expected.trim().parse().ok()?;
    a.partial_cmp(&e)
}

/// Evaluates one assertion. `expected` should already be variable-resolved.
pub fn evaluate(response: &Response, assertion: &Assertion, expected: &str) -> AssertionResult {
    let name = if assertion.op.needs_expected() {
        format!("{} {} {}", assertion.target, assertion.op.label(), expected)
    } else {
        format!("{} {}", assertion.target, assertion.op.label())
    };
    let actual = select(response, &assertion.target);
    let shown = actual
        .as_deref()
        .map(|a| {
            if a.chars().count() > 80 {
                format!("{}…", a.chars().take(80).collect::<String>())
            } else {
                a.to_string()
            }
        })
        .unwrap_or_else(|| "<missing>".into());

    let passed = match (assertion.op, actual.as_deref()) {
        (Operator::Exists, a) => a.is_some(),
        (Operator::NotExists, a) => a.is_none(),
        (_, None) => false,
        (Operator::Equals, Some(a)) => a == expected || compare_numbers(a, expected) == Some(std::cmp::Ordering::Equal),
        (Operator::NotEquals, Some(a)) => {
            a != expected && compare_numbers(a, expected) != Some(std::cmp::Ordering::Equal)
        }
        (Operator::Contains, Some(a)) => a.contains(expected),
        (Operator::NotContains, Some(a)) => !a.contains(expected),
        (Operator::LessThan, Some(a)) => compare_numbers(a, expected) == Some(std::cmp::Ordering::Less),
        (Operator::GreaterThan, Some(a)) => compare_numbers(a, expected) == Some(std::cmp::Ordering::Greater),
        (Operator::Matches, Some(a)) => glob_match(expected, a),
    };

    AssertionResult { name, passed, message: if passed { "passed".into() } else { format!("actual: {shown}") } }
}

/// Runs every enabled assertion; `resolve` turns `{{vars}}` in expected values.
pub fn run_all(
    response: &Response,
    assertions: &[Assertion],
    resolve: impl Fn(&str) -> String,
) -> Vec<AssertionResult> {
    assertions
        .iter()
        .filter(|a| a.enabled && !a.target.trim().is_empty())
        .map(|a| evaluate(response, a, &resolve(&a.expected)))
        .collect()
}

/// Returns `(variable, value)` pairs for every extraction that matched.
pub fn extract_all(response: &Response, extractions: &[Extraction]) -> Vec<(String, String)> {
    extractions
        .iter()
        .filter(|e| e.enabled && !e.variable.trim().is_empty())
        .filter_map(|e| select(response, &e.source).map(|v| (e.variable.trim().to_string(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> Response {
        Response {
            status: 201,
            status_text: "Created".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: br#"{"data":{"items":[{"id":7,"name":"pak"}]},"token":"abc"}"#.to_vec(),
            elapsed_ms: 42,
            url: "http://x".into(),
            http_version: "HTTP/1.1".into(),
        }
    }

    fn a(target: &str, op: Operator, expected: &str) -> Assertion {
        Assertion { enabled: true, target: target.into(), op, expected: expected.into() }
    }

    #[test]
    fn json_paths() {
        let v: serde_json::Value = serde_json::from_str(r#"{"a":[{"b":1}],"c d":2}"#).unwrap();
        assert_eq!(json_path(&v, "a[0].b"), Some(&serde_json::json!(1)));
        assert_eq!(json_path(&v, "$.a[0].b"), Some(&serde_json::json!(1)));
        assert_eq!(json_path(&v, "[\"c d\"]"), Some(&serde_json::json!(2)));
        assert_eq!(json_path(&v, "a[5]"), None);
    }

    #[test]
    fn assertions() {
        let r = response();
        let cases = [
            (a("status", Operator::Equals, "201"), true),
            (a("status", Operator::Equals, "200"), false),
            (a("time", Operator::LessThan, "500"), true),
            (a("header.content-type", Operator::Contains, "json"), true),
            (a("json.data.items[0].id", Operator::Equals, "7"), true),
            (a("json.data.items[0].name", Operator::Matches, "p*k"), true),
            (a("json.missing", Operator::NotExists, ""), true),
            (a("json.token", Operator::Exists, ""), true),
            (a("body", Operator::NotContains, "error"), true),
        ];
        for (assertion, expected) in cases {
            let res = evaluate(&r, &assertion, &assertion.expected);
            assert_eq!(res.passed, expected, "{}: {}", res.name, res.message);
        }
    }

    #[test]
    fn extraction() {
        let r = response();
        let ex = vec![Extraction { enabled: true, variable: "token".into(), source: "json.token".into() }];
        assert_eq!(extract_all(&r, &ex), vec![("token".into(), "abc".into())]);
    }

    #[test]
    fn globbing() {
        assert!(glob_match("a*c", "abbbc"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
    }
}
