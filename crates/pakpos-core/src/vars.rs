//! `{{variable}}` substitution.
//!
//! Resolution order (first match wins):
//! 1. runtime variables (set by extractions during a run),
//! 2. the active environment,
//! 3. collection variables,
//! 4. global variables,
//! 5. dynamic variables (`{{$guid}}`, `{{$timestamp}}`, ...).
//!
//! Unknown variables are left untouched so the user can see what is missing.

use crate::model::{Environment, KeyValue};
use rand::Rng;
use std::collections::{BTreeSet, HashMap};

/// Layered variable lookup.
#[derive(Debug, Clone, Default)]
pub struct VarScope {
    pub runtime: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub globals: HashMap<String, String>,
}

fn to_map(vars: &[KeyValue]) -> HashMap<String, String> {
    vars.iter().filter(|v| v.enabled && !v.key.is_empty()).map(|v| (v.key.clone(), v.value.clone())).collect()
}

impl VarScope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_environment(mut self, env: Option<&Environment>) -> Self {
        self.environment = env.map(|e| to_map(&e.variables)).unwrap_or_default();
        self
    }

    pub fn with_collection_vars(mut self, vars: &[KeyValue]) -> Self {
        self.collection = to_map(vars);
        self
    }

    pub fn with_globals(mut self, vars: &[KeyValue]) -> Self {
        self.globals = to_map(vars);
        self
    }

    /// Looks up a single variable name (without braces).
    pub fn get(&self, name: &str) -> Option<String> {
        if let Some(v) = self
            .runtime
            .get(name)
            .or_else(|| self.environment.get(name))
            .or_else(|| self.collection.get(name))
            .or_else(|| self.globals.get(name))
        {
            return Some(v.clone());
        }
        dynamic(name)
    }

    /// Replaces every `{{name}}` occurrence. Values are resolved recursively
    /// (a variable may reference another one), with a depth limit to avoid
    /// infinite loops.
    pub fn resolve(&self, input: &str) -> String {
        let mut current = input.to_string();
        for _ in 0..8 {
            let next = self.resolve_once(&current);
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn resolve_once(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    match self.get(name) {
                        Some(v) => out.push_str(&v),
                        None => {
                            out.push_str("{{");
                            out.push_str(&after[..end]);
                            out.push_str("}}");
                        }
                    }
                    rest = &after[end + 2..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Names referenced in `input` that cannot be resolved.
    pub fn unresolved(&self, input: &str) -> Vec<String> {
        let resolved = self.resolve(input);
        let mut missing = BTreeSet::new();
        for name in referenced(&resolved) {
            if self.get(&name).is_none() {
                missing.insert(name);
            }
        }
        missing.into_iter().collect()
    }
}

/// Returns every variable name referenced as `{{name}}` in `input`.
pub fn referenced(input: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                if !name.is_empty() {
                    names.push(name.to_string());
                }
                rest = &after[end + 2..];
            }
            None => break,
        }
    }
    names
}

/// Built-in dynamic variables, compatible with the Postman names.
fn dynamic(name: &str) -> Option<String> {
    let mut rng = rand::rng();
    Some(match name {
        "$guid" | "$randomUUID" => uuid::Uuid::new_v4().to_string(),
        "$timestamp" => chrono::Utc::now().timestamp().to_string(),
        "$isoTimestamp" => chrono::Utc::now().to_rfc3339(),
        "$randomInt" => rng.random_range(0..=1000).to_string(),
        "$randomBoolean" => rng.random_bool(0.5).to_string(),
        "$randomEmail" => format!("user{}@example.com", rng.random_range(1000..=9999)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> VarScope {
        let mut s = VarScope::new();
        s.environment.insert("base".into(), "https://api.test".into());
        s.environment.insert("v".into(), "v1".into());
        s.collection.insert("path".into(), "{{v}}/users".into());
        s.collection.insert("base".into(), "ignored".into());
        s
    }

    #[test]
    fn resolves_and_nests() {
        assert_eq!(scope().resolve("{{base}}/{{ path }}"), "https://api.test/v1/users");
    }

    #[test]
    fn keeps_unknown() {
        assert_eq!(scope().resolve("a {{nope}} b"), "a {{nope}} b");
        assert_eq!(scope().unresolved("{{base}}/{{nope}}"), vec!["nope"]);
    }

    #[test]
    fn unterminated_braces() {
        assert_eq!(scope().resolve("x {{base"), "x {{base");
    }

    #[test]
    fn runtime_overrides_environment() {
        let mut s = scope();
        s.runtime.insert("base".into(), "http://localhost".into());
        assert_eq!(s.resolve("{{base}}"), "http://localhost");
    }

    #[test]
    fn dynamic_vars() {
        let s = VarScope::new();
        assert_eq!(s.resolve("{{$guid}}").len(), 36);
        assert!(s.resolve("{{$timestamp}}").parse::<i64>().is_ok());
    }

    #[test]
    fn self_reference_terminates() {
        let mut s = VarScope::new();
        s.environment.insert("a".into(), "{{a}}x".into());
        let out = s.resolve("{{a}}");
        assert!(out.ends_with('x'));
    }
}
