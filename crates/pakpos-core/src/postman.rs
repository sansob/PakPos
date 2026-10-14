//! Import / export of Postman Collection v2.0 / v2.1 and Postman environments.
//!
//! Only the parts that map onto the PakPos model are converted. Pre-request and
//! test *scripts* (JavaScript) are skipped and reported as warnings.

use crate::model::{
    ApiKeyLocation, Auth, Body, ClientAuthMethod, Collection, Environment, Folder, FormField, HttpMethod, Item,
    KeyValue, OAuth2Config, OAuth2Grant, RawLanguage, Request,
};
use serde_json::{json, Map, Value};

pub const SCHEMA_V21: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

#[derive(Debug, thiserror::Error)]
pub enum PostmanError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not a Postman collection (missing `info` or `item`)")]
    NotACollection,
    #[error("not a Postman environment (missing `values`)")]
    NotAnEnvironment,
}

/// Result of an import, with non-fatal warnings.
#[derive(Debug, Clone)]
pub struct Imported<T> {
    pub value: T,
    pub warnings: Vec<String>,
}

fn s(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn description(v: &Value) -> String {
    match v.get("description") {
        Some(Value::String(s)) => s.clone(),
        Some(obj @ Value::Object(_)) => s(obj, "content"),
        _ => String::new(),
    }
}

fn kv_list(v: Option<&Value>) -> Vec<KeyValue> {
    v.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|e| e.is_object())
                .map(|e| KeyValue {
                    key: s(e, "key"),
                    value: s(e, "value"),
                    enabled: !e.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                    description: description(e),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Splits `a=1&b=2` without decoding, so `{{vars}}` survive untouched.
fn split_query(query: &str) -> Vec<KeyValue> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => KeyValue::new(k, v),
            None => KeyValue::new(pair, ""),
        })
        .collect()
}

fn parse_url(v: Option<&Value>) -> (String, Vec<KeyValue>) {
    match v {
        Some(Value::String(raw)) => match raw.split_once('?') {
            Some((base, q)) => (base.to_string(), split_query(q)),
            None => (raw.clone(), Vec::new()),
        },
        Some(obj @ Value::Object(_)) => {
            let raw = s(obj, "raw");
            let base = raw.split_once('?').map(|(b, _)| b.to_string()).unwrap_or(raw.clone());
            let params = if obj.get("query").is_some() {
                kv_list(obj.get("query"))
            } else {
                raw.split_once('?').map(|(_, q)| split_query(q)).unwrap_or_default()
            };
            (base, params)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// Reads an auth parameter in either v2.1 (array) or v2.0 (object) shape.
fn auth_param(section: Option<&Value>, key: &str) -> String {
    match section {
        Some(Value::Array(arr)) => arr
            .iter()
            .find(|e| e.get("key").and_then(Value::as_str) == Some(key))
            .map(|e| s(e, "value"))
            .unwrap_or_default(),
        Some(obj @ Value::Object(_)) => s(obj, key),
        _ => String::new(),
    }
}

fn parse_auth(v: Option<&Value>, warnings: &mut Vec<String>) -> Auth {
    let Some(auth) = v.filter(|a| a.is_object()) else {
        return Auth::Inherit;
    };
    let kind = s(auth, "type");
    match kind.as_str() {
        "noauth" => Auth::None,
        "basic" => Auth::Basic {
            username: auth_param(auth.get("basic"), "username"),
            password: auth_param(auth.get("basic"), "password"),
        },
        "bearer" => Auth::Bearer { token: auth_param(auth.get("bearer"), "token") },
        "apikey" => {
            let section = auth.get("apikey");
            let key = auth_param(section, "key");
            Auth::ApiKey {
                key: if key.is_empty() { "X-API-Key".into() } else { key },
                value: auth_param(section, "value"),
                location: if auth_param(section, "in") == "query" {
                    ApiKeyLocation::Query
                } else {
                    ApiKeyLocation::Header
                },
            }
        }
        "oauth2" => {
            let sec = auth.get("oauth2");
            let p = |k: &str| auth_param(sec, k);
            let grant = match p("grant_type").as_str() {
                "password_credentials" => OAuth2Grant::Password,
                "authorization_code" | "authorization_code_with_pkce" => OAuth2Grant::AuthorizationCode,
                "implicit" => {
                    warnings.push("OAuth 2.0 implicit grant is deprecated; imported as authorization code".into());
                    OAuth2Grant::AuthorizationCode
                }
                _ => OAuth2Grant::ClientCredentials,
            };
            let mut cfg = OAuth2Config {
                grant,
                token_url: p("accessTokenUrl"),
                auth_url: p("authUrl"),
                client_id: p("clientId"),
                client_secret: p("clientSecret"),
                scope: p("scope"),
                username: p("username"),
                password: p("password"),
                use_pkce: p("grant_type") != "authorization_code" || p("challengeAlgorithm") == "S256",
                client_auth: if p("client_authentication") == "body" {
                    ClientAuthMethod::Body
                } else {
                    ClientAuthMethod::Header
                },
                ..Default::default()
            };
            if !p("redirect_uri").is_empty() {
                cfg.redirect_uri = p("redirect_uri");
            }
            if !p("headerPrefix").is_empty() {
                cfg.header_prefix = p("headerPrefix");
            }
            if grant == OAuth2Grant::AuthorizationCode
                && !cfg.redirect_uri.starts_with("http://127.0.0.1")
                && !cfg.redirect_uri.starts_with("http://localhost")
            {
                warnings.push(format!(
                    "OAuth 2.0 redirect `{}` replaced by http://127.0.0.1:7777/callback (PakPos listens locally); register it with your provider",
                    cfg.redirect_uri
                ));
                cfg.redirect_uri = "http://127.0.0.1:7777/callback".into();
            }
            Auth::OAuth2(cfg)
        }
        "" => Auth::Inherit,
        other => {
            warnings.push(format!("auth type `{other}` is not supported yet; skipped"));
            Auth::Inherit
        }
    }
}

fn parse_body(v: Option<&Value>, warnings: &mut Vec<String>) -> Body {
    let Some(body) = v.filter(|b| b.is_object()) else {
        return Body::None;
    };
    match s(body, "mode").as_str() {
        "raw" => {
            let lang = body.pointer("/options/raw/language").and_then(Value::as_str).unwrap_or("text");
            Body::Raw {
                content: s(body, "raw"),
                language: match lang {
                    "json" => RawLanguage::Json,
                    "xml" => RawLanguage::Xml,
                    "html" => RawLanguage::Html,
                    "javascript" => RawLanguage::Javascript,
                    _ => RawLanguage::Text,
                },
            }
        }
        "urlencoded" => Body::FormUrlEncoded { fields: kv_list(body.get("urlencoded")) },
        "formdata" => Body::Multipart {
            fields: body
                .get("formdata")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|e| {
                            let is_file = s(e, "type") == "file";
                            FormField {
                                key: s(e, "key"),
                                value: if is_file {
                                    match e.get("src") {
                                        Some(Value::Array(a)) => {
                                            a.first().and_then(Value::as_str).unwrap_or("").to_string()
                                        }
                                        _ => s(e, "src"),
                                    }
                                } else {
                                    s(e, "value")
                                },
                                is_file,
                                enabled: !e.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        "file" => Body::Binary { path: body.get("file").map(|f| s(f, "src")).unwrap_or_default() },
        "graphql" => Body::GraphQl {
            query: body.pointer("/graphql/query").and_then(Value::as_str).unwrap_or("").to_string(),
            variables: match body.pointer("/graphql/variables") {
                Some(Value::String(v)) => v.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => serde_json::to_string_pretty(other).unwrap_or_default(),
            },
            operation_name: String::new(),
        },
        "" => Body::None,
        other => {
            warnings.push(format!("body mode `{other}` is not supported; skipped"));
            Body::None
        }
    }
}

fn parse_items(items: Option<&Value>, warnings: &mut Vec<String>) -> Vec<Item> {
    let Some(items) = items.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|it| {
            if it.get("event").and_then(Value::as_array).is_some_and(|e| !e.is_empty()) {
                warnings.push(format!(
                    "`{}`: JavaScript scripts are not executed by PakPos; use Tests/Extract instead",
                    s(it, "name")
                ));
            }
            if it.get("item").is_some() {
                Item::Folder(Folder {
                    id: crate::model::new_id(),
                    name: s(it, "name"),
                    items: parse_items(it.get("item"), warnings),
                    auth: parse_auth(it.get("auth"), warnings),
                    description: description(it),
                })
            } else {
                let req = it.get("request").cloned().unwrap_or(Value::Null);
                // A request may be a bare URL string in v2.x.
                let (url, params, method, headers, body, auth, desc) = match &req {
                    Value::String(u) => {
                        let (url, params) = parse_url(Some(&Value::String(u.clone())));
                        (url, params, HttpMethod::Get, Vec::new(), Body::None, Auth::Inherit, String::new())
                    }
                    _ => {
                        let (url, params) = parse_url(req.get("url"));
                        (
                            url,
                            params,
                            HttpMethod::parse(&s(&req, "method")).unwrap_or_default(),
                            kv_list(req.get("header")),
                            parse_body(req.get("body"), warnings),
                            parse_auth(req.get("auth"), warnings),
                            description(&req),
                        )
                    }
                };
                Item::Request(Request {
                    id: crate::model::new_id(),
                    name: s(it, "name"),
                    method,
                    url,
                    params,
                    headers,
                    body,
                    auth,
                    assertions: Vec::new(),
                    extractions: Vec::new(),
                    description: desc,
                })
            }
        })
        .collect()
}

/// Imports a Postman collection (v2.0 or v2.1) from JSON text.
pub fn import_collection(json_text: &str) -> Result<Imported<Collection>, PostmanError> {
    let root: Value = serde_json::from_str(json_text)?;
    let info = root.get("info").ok_or(PostmanError::NotACollection)?;
    if root.get("item").is_none() {
        return Err(PostmanError::NotACollection);
    }
    let mut warnings = Vec::new();
    let auth = match parse_auth(root.get("auth"), &mut warnings) {
        Auth::Inherit => Auth::None,
        a => a,
    };
    let collection = Collection {
        id: crate::model::new_id(),
        name: {
            let n = s(info, "name");
            if n.is_empty() {
                "Imported collection".into()
            } else {
                n
            }
        },
        description: description(info),
        items: parse_items(root.get("item"), &mut warnings),
        variables: kv_list(root.get("variable")),
        auth,
    };
    warnings.dedup();
    Ok(Imported { value: collection, warnings })
}

/// Imports a Postman environment export.
pub fn import_environment(json_text: &str) -> Result<Environment, PostmanError> {
    let root: Value = serde_json::from_str(json_text)?;
    let values = root.get("values").ok_or(PostmanError::NotAnEnvironment)?;
    let variables = values
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| KeyValue {
                    key: s(e, "key"),
                    value: s(e, "value"),
                    enabled: e.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                    description: String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Environment {
        id: crate::model::new_id(),
        name: {
            let n = s(&root, "name");
            if n.is_empty() {
                "Imported environment".into()
            } else {
                n
            }
        },
        variables,
    })
}

fn kv_to_json(kvs: &[KeyValue]) -> Value {
    Value::Array(
        kvs.iter()
            .map(|kv| {
                let mut m = Map::new();
                m.insert("key".into(), json!(kv.key));
                m.insert("value".into(), json!(kv.value));
                if !kv.enabled {
                    m.insert("disabled".into(), json!(true));
                }
                if !kv.description.is_empty() {
                    m.insert("description".into(), json!(kv.description));
                }
                Value::Object(m)
            })
            .collect(),
    )
}

fn auth_to_json(auth: &Auth) -> Option<Value> {
    let kv = |k: &str, v: &str| json!({ "key": k, "value": v, "type": "string" });
    match auth {
        Auth::Inherit => None,
        Auth::None => Some(json!({ "type": "noauth" })),
        Auth::Basic { username, password } => Some(json!({
            "type": "basic",
            "basic": [kv("username", username), kv("password", password)]
        })),
        Auth::Bearer { token } => Some(json!({ "type": "bearer", "bearer": [kv("token", token)] })),
        Auth::ApiKey { key, value, location } => Some(json!({
            "type": "apikey",
            "apikey": [
                kv("key", key),
                kv("value", value),
                kv("in", match location { ApiKeyLocation::Header => "header", ApiKeyLocation::Query => "query" }),
            ]
        })),
        Auth::OAuth2(c) => {
            let grant = match c.grant {
                OAuth2Grant::ClientCredentials => "client_credentials",
                OAuth2Grant::Password => "password_credentials",
                OAuth2Grant::AuthorizationCode if c.use_pkce => "authorization_code_with_pkce",
                OAuth2Grant::AuthorizationCode => "authorization_code",
            };
            Some(json!({
                "type": "oauth2",
                "oauth2": [
                    kv("grant_type", grant),
                    kv("accessTokenUrl", &c.token_url),
                    kv("authUrl", &c.auth_url),
                    kv("clientId", &c.client_id),
                    kv("clientSecret", &c.client_secret),
                    kv("scope", &c.scope),
                    kv("username", &c.username),
                    kv("password", &c.password),
                    kv("redirect_uri", &c.redirect_uri),
                    kv("headerPrefix", &c.header_prefix),
                    kv("challengeAlgorithm", "S256"),
                    kv("client_authentication", match c.client_auth { ClientAuthMethod::Header => "header", ClientAuthMethod::Body => "body" }),
                    kv("addTokenTo", "header"),
                ]
            }))
        }
    }
}

fn body_to_json(body: &Body) -> Option<Value> {
    match body {
        Body::None => None,
        Body::Raw { content, language } => Some(json!({
            "mode": "raw",
            "raw": content,
            "options": { "raw": { "language": match language {
                RawLanguage::Json => "json",
                RawLanguage::Text => "text",
                RawLanguage::Xml => "xml",
                RawLanguage::Html => "html",
                RawLanguage::Javascript => "javascript",
            } } }
        })),
        Body::FormUrlEncoded { fields } => Some(json!({ "mode": "urlencoded", "urlencoded": kv_to_json(fields) })),
        Body::Multipart { fields } => Some(json!({
            "mode": "formdata",
            "formdata": fields.iter().map(|f| {
                let mut m = Map::new();
                m.insert("key".into(), json!(f.key));
                if f.is_file {
                    m.insert("type".into(), json!("file"));
                    m.insert("src".into(), json!(f.value));
                } else {
                    m.insert("type".into(), json!("text"));
                    m.insert("value".into(), json!(f.value));
                }
                if !f.enabled { m.insert("disabled".into(), json!(true)); }
                Value::Object(m)
            }).collect::<Vec<_>>()
        })),
        Body::Binary { path } => Some(json!({ "mode": "file", "file": { "src": path } })),
        Body::GraphQl { query, variables, .. } => {
            Some(json!({ "mode": "graphql", "graphql": { "query": query, "variables": variables } }))
        }
    }
}

fn raw_url(req: &Request) -> String {
    let q: Vec<String> = req.params.iter().filter(|p| p.enabled).map(|p| format!("{}={}", p.key, p.value)).collect();
    if q.is_empty() {
        req.url.clone()
    } else {
        format!("{}?{}", req.url, q.join("&"))
    }
}

fn items_to_json(items: &[Item]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| match item {
                Item::Folder(f) => {
                    let mut m = Map::new();
                    m.insert("name".into(), json!(f.name));
                    m.insert("item".into(), items_to_json(&f.items));
                    if let Some(a) = auth_to_json(&f.auth) {
                        m.insert("auth".into(), a);
                    }
                    if !f.description.is_empty() {
                        m.insert("description".into(), json!(f.description));
                    }
                    Value::Object(m)
                }
                Item::Request(r) => {
                    let mut req = Map::new();
                    req.insert("method".into(), json!(r.method.as_str()));
                    req.insert("header".into(), kv_to_json(&r.headers));
                    req.insert("url".into(), json!({ "raw": raw_url(r), "query": kv_to_json(&r.params) }));
                    if let Some(b) = body_to_json(&r.body) {
                        req.insert("body".into(), b);
                    }
                    if let Some(a) = auth_to_json(&r.auth) {
                        req.insert("auth".into(), a);
                    }
                    if !r.description.is_empty() {
                        req.insert("description".into(), json!(r.description));
                    }
                    json!({ "name": r.name, "request": Value::Object(req) })
                }
            })
            .collect(),
    )
}

/// Exports a collection as Postman v2.1 JSON.
pub fn export_collection(c: &Collection) -> String {
    let mut root = Map::new();
    root.insert(
        "info".into(),
        json!({
            "_postman_id": c.id,
            "name": c.name,
            "description": c.description,
            "schema": SCHEMA_V21,
        }),
    );
    root.insert("item".into(), items_to_json(&c.items));
    if let Some(a) = auth_to_json(&c.auth) {
        root.insert("auth".into(), a);
    }
    if !c.variables.is_empty() {
        root.insert("variable".into(), kv_to_json(&c.variables));
    }
    serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
}

/// Exports an environment in Postman's format.
pub fn export_environment(env: &Environment) -> String {
    serde_json::to_string_pretty(&json!({
        "id": env.id,
        "name": env.name,
        "values": env.variables.iter().map(|v| json!({
            "key": v.key, "value": v.value, "enabled": v.enabled, "type": "default"
        })).collect::<Vec<_>>(),
        "_postman_variable_scope": "environment",
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "info": {"name": "Sample", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
      "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}", "type": "string"}]},
      "variable": [{"key": "base", "value": "https://api.test"}],
      "item": [
        {"name": "Users", "item": [
          {"name": "List users",
           "event": [{"listen": "test", "script": {"exec": ["pm.test()"]}}],
           "request": {"method": "GET", "header": [{"key": "Accept", "value": "application/json"}],
             "url": {"raw": "{{base}}/users?page=1&limit={{limit}}", "host": ["{{base}}"], "path": ["users"],
                     "query": [{"key": "page", "value": "1"}, {"key": "limit", "value": "{{limit}}", "disabled": true}]}}},
          {"name": "Create user",
           "request": {"method": "POST", "url": "{{base}}/users",
             "body": {"mode": "raw", "raw": "{\"name\":\"x\"}", "options": {"raw": {"language": "json"}}},
             "auth": {"type": "noauth"}}}
        ]},
        {"name": "Health", "request": "https://api.test/health"}
      ]
    }"#;

    #[test]
    fn imports_sample() {
        let imported = import_collection(SAMPLE).unwrap();
        let c = imported.value;
        assert_eq!(c.name, "Sample");
        assert_eq!(c.auth, Auth::Bearer { token: "{{token}}".into() });
        assert_eq!(c.variables[0].key, "base");
        let flat = c.flatten();
        assert_eq!(flat.len(), 3);
        let (path, list) = &flat[0];
        assert_eq!(path, &vec!["Users".to_string()]);
        assert_eq!(list.url, "{{base}}/users");
        assert_eq!(list.params.len(), 2);
        assert!(!list.params[1].enabled);
        assert_eq!(list.auth, Auth::Bearer { token: "{{token}}".into() });
        assert_eq!(flat[1].1.method, HttpMethod::Post);
        assert_eq!(flat[1].1.auth, Auth::None);
        assert_eq!(flat[2].1.url, "https://api.test/health");
        assert_eq!(imported.warnings.len(), 1);
    }

    #[test]
    fn export_then_import_roundtrip() {
        let c = import_collection(SAMPLE).unwrap().value;
        let exported = export_collection(&c);
        let back = import_collection(&exported).unwrap().value;
        let a: Vec<_> = c.flatten().into_iter().map(|(p, r)| (p, r.method, r.url, r.params, r.body, r.auth)).collect();
        let b: Vec<_> =
            back.flatten().into_iter().map(|(p, r)| (p, r.method, r.url, r.params, r.body, r.auth)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn environment_roundtrip() {
        let mut env = Environment::new("Staging");
        env.set("base", "https://staging");
        let back = import_environment(&export_environment(&env)).unwrap();
        assert_eq!(back.name, "Staging");
        assert_eq!(back.get("base"), Some("https://staging"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(import_collection("{}"), Err(PostmanError::NotACollection)));
        assert!(import_collection("nope").is_err());
    }
}
