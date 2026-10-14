//! OpenAPI import (3.0 / 3.1, and Swagger 2.0) and export (3.0.3), JSON or YAML.
//!
//! Import: one request per operation, folders by tag, `{{baseUrl}}` from
//! `servers`, path parameters as `{{variables}}`, example bodies, auth from
//! `securitySchemes`, and response examples as saved examples (so the mock
//! server can serve them right away).

use crate::model::{
    ApiKeyLocation, Auth, Body, Collection, Example, Folder, HttpMethod, Item, KeyValue, OAuth2Config, OAuth2Grant,
    RawLanguage, Request,
};
use serde_json::{json, Map, Value};

#[derive(Debug, thiserror::Error)]
pub enum OpenApiError {
    #[error("not JSON or YAML: {0}")]
    Parse(String),
    #[error("not an OpenAPI / Swagger document (no `openapi` or `swagger` field)")]
    NotOpenApi,
}

/// Result of an import, with notes about what could not be converted.
pub struct Imported {
    pub collection: Collection,
    pub warnings: Vec<String>,
}

// ---- YAML <-> JSON values ----------------------------------------------------

fn yaml_to_json(y: &yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match y {
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::String(s.clone())),
        Yaml::Integer(i) => json!(i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(a) => Value::Array(a.iter().map(yaml_to_json).collect()),
        Yaml::Hash(h) => Value::Object(
            h.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) | Yaml::Real(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => String::new(),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Value::Null,
    }
}

fn json_to_yaml(v: &Value) -> yaml_rust2::Yaml {
    use yaml_rust2::Yaml;
    match v {
        Value::Null => Yaml::Null,
        Value::Bool(b) => Yaml::Boolean(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Yaml::Integer(i),
            None => Yaml::Real(n.to_string()),
        },
        Value::String(s) => Yaml::String(s.clone()),
        Value::Array(a) => Yaml::Array(a.iter().map(json_to_yaml).collect()),
        Value::Object(m) => Yaml::Hash(m.iter().map(|(k, v)| (Yaml::String(k.clone()), json_to_yaml(v))).collect()),
    }
}

/// Parses JSON or YAML into a JSON value.
pub fn parse_document(text: &str) -> Result<Value, OpenApiError> {
    let t = text.trim_start_matches('\u{feff}');
    if t.trim_start().starts_with('{') {
        return serde_json::from_str(t).map_err(|e| OpenApiError::Parse(e.to_string()));
    }
    let docs = yaml_rust2::YamlLoader::load_from_str(t).map_err(|e| OpenApiError::Parse(e.to_string()))?;
    docs.first().map(yaml_to_json).ok_or_else(|| OpenApiError::Parse("empty document".into()))
}

/// Does this text look like an OpenAPI / Swagger document?
pub fn is_openapi(text: &str) -> bool {
    parse_document(text).is_ok_and(|v| v.get("openapi").is_some() || v.get("swagger").is_some())
}

pub fn to_yaml(v: &Value) -> String {
    let mut out = String::new();
    let mut emitter = yaml_rust2::YamlEmitter::new(&mut out);
    let _ = emitter.dump(&json_to_yaml(v));
    out.trim_start_matches("---\n").to_string() + "\n"
}

// ---- import ------------------------------------------------------------------------

struct Ctx<'a> {
    root: &'a Value,
    warnings: Vec<String>,
}

impl Ctx<'_> {
    /// Follows a local `$ref` (with a depth limit for cycles).
    fn deref<'b>(&'b self, v: &'b Value) -> &'b Value {
        let mut cur = v;
        for _ in 0..16 {
            match cur.get("$ref").and_then(Value::as_str) {
                Some(r) if r.starts_with("#/") => {
                    let pointer = r[1..].replace("~1", "/").replace("~0", "~");
                    match self.root.pointer(&pointer) {
                        Some(next) => cur = next,
                        None => return &Value::Null,
                    }
                }
                _ => return cur,
            }
        }
        &Value::Null
    }

    /// An example value for a schema: `example`, `default`, first `enum`,
    /// or a value of the right type; objects and arrays recursively.
    fn example(&self, schema: &Value, depth: usize) -> Value {
        let s = self.deref(schema);
        if depth > 8 {
            return Value::Null;
        }
        for key in ["example", "default"] {
            if let Some(v) = s.get(key) {
                return v.clone();
            }
        }
        if let Some(v) = s.get("examples").and_then(Value::as_array).and_then(|a| a.first()) {
            return v.clone();
        }
        if let Some(v) = s.get("enum").and_then(Value::as_array).and_then(|a| a.first()) {
            return v.clone();
        }
        for combo in ["allOf", "oneOf", "anyOf"] {
            if let Some(list) = s.get(combo).and_then(Value::as_array) {
                if combo == "allOf" {
                    let mut merged = Map::new();
                    for part in list {
                        if let Value::Object(m) = self.example(part, depth + 1) {
                            merged.extend(m);
                        }
                    }
                    return Value::Object(merged);
                }
                return list.first().map(|p| self.example(p, depth + 1)).unwrap_or(Value::Null);
            }
        }
        let ty = match s.get("type") {
            Some(Value::String(t)) => t.as_str(),
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("null"),
            _ if s.get("properties").is_some() => "object",
            _ if s.get("items").is_some() => "array",
            _ => "",
        };
        match ty {
            "object" => Value::Object(
                s.get("properties")
                    .and_then(Value::as_object)
                    .map(|p| p.iter().map(|(k, v)| (k.clone(), self.example(v, depth + 1))).collect())
                    .unwrap_or_default(),
            ),
            "array" => Value::Array(vec![self.example(s.get("items").unwrap_or(&Value::Null), depth + 1)]),
            "integer" => json!(0),
            "number" => json!(0.0),
            "boolean" => json!(false),
            "string" => match s.get("format").and_then(Value::as_str) {
                Some("date-time") => json!("2026-01-01T00:00:00Z"),
                Some("date") => json!("2026-01-01"),
                Some("email") => json!("budi@example.com"),
                Some("uuid") => json!("00000000-0000-0000-0000-000000000000"),
                Some("uri") | Some("url") => json!("https://example.com"),
                _ => json!(""),
            },
            _ => Value::Null,
        }
    }

    /// Example for a media type object: `example`, first of `examples`, or from the schema.
    fn media_example(&self, media: &Value) -> Value {
        if let Some(e) = media.get("example") {
            return e.clone();
        }
        if let Some(first) = media.get("examples").and_then(Value::as_object).and_then(|m| m.values().next()) {
            if let Some(v) = self.deref(first).get("value") {
                return v.clone();
            }
        }
        self.example(media.get("schema").unwrap_or(&Value::Null), 0)
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `{petId}` → `{{petId}}`.
fn path_to_vars(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    let mut rest = path;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => {
                out.push_str("{{");
                out.push_str(&rest[i + 1..i + j]);
                out.push_str("}}");
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn base_url(doc: &Value) -> String {
    if doc.get("swagger").is_some() {
        let scheme = doc.pointer("/schemes/0").and_then(Value::as_str).unwrap_or("https");
        let host = doc.get("host").and_then(Value::as_str).unwrap_or("localhost");
        let base = doc.get("basePath").and_then(Value::as_str).unwrap_or("");
        return format!("{scheme}://{host}{}", base.trim_end_matches('/'));
    }
    let Some(server) = doc.pointer("/servers/0") else { return "http://localhost".into() };
    let mut url = server.get("url").and_then(Value::as_str).unwrap_or("/").to_string();
    if let Some(vars) = server.get("variables").and_then(Value::as_object) {
        for (k, v) in vars {
            url = url.replace(&format!("{{{k}}}"), &as_text(v.get("default").unwrap_or(&Value::Null)));
        }
    }
    url.trim_end_matches('/').to_string()
}

fn scheme_to_auth(ctx: &mut Ctx, name: &str, scheme: &Value, scopes: &[String]) -> Option<Auth> {
    let s = ctx.deref(scheme).clone();
    let ty = s.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "http" | "basic" => match s
            .get("scheme")
            .and_then(Value::as_str)
            .unwrap_or(if ty == "basic" { "basic" } else { "" })
            .to_ascii_lowercase()
            .as_str()
        {
            "basic" => Some(Auth::Basic { username: "{{username}}".into(), password: "{{password}}".into() }),
            "bearer" => Some(Auth::Bearer { token: "{{bearerToken}}".into() }),
            other => {
                ctx.warnings.push(format!("security scheme `{name}`: HTTP `{other}` is not supported"));
                None
            }
        },
        "apiKey" => {
            let key = s.get("name").and_then(Value::as_str).unwrap_or("X-API-Key").to_string();
            let location = match s.get("in").and_then(Value::as_str) {
                Some("query") => ApiKeyLocation::Query,
                Some("cookie") => {
                    ctx.warnings
                        .push(format!("security scheme `{name}`: API key in a cookie, sent as a header instead"));
                    ApiKeyLocation::Header
                }
                _ => ApiKeyLocation::Header,
            };
            Some(Auth::ApiKey { key, value: "{{apiKey}}".into(), location })
        }
        "oauth2" => {
            let flows = s.get("flows").cloned().unwrap_or_else(|| {
                // Swagger 2.0: flow + tokenUrl / authorizationUrl on the scheme itself.
                let flow = match s.get("flow").and_then(Value::as_str).unwrap_or("") {
                    "application" => "clientCredentials",
                    "accessCode" => "authorizationCode",
                    other => other,
                };
                json!({ flow: s })
            });
            let (grant, flow) = if let Some(f) = flows.get("clientCredentials") {
                (OAuth2Grant::ClientCredentials, f)
            } else if let Some(f) = flows.get("authorizationCode") {
                (OAuth2Grant::AuthorizationCode, f)
            } else if let Some(f) = flows.get("password") {
                (OAuth2Grant::Password, f)
            } else {
                ctx.warnings.push(format!("security scheme `{name}`: only clientCredentials, password and authorizationCode flows are supported"));
                return None;
            };
            Some(Auth::OAuth2(OAuth2Config {
                grant,
                token_url: flow.get("tokenUrl").and_then(Value::as_str).unwrap_or("").into(),
                auth_url: flow.get("authorizationUrl").and_then(Value::as_str).unwrap_or("").into(),
                client_id: "{{clientId}}".into(),
                client_secret: "{{clientSecret}}".into(),
                scope: scopes.join(" "),
                ..Default::default()
            }))
        }
        "openIdConnect" => {
            ctx.warnings.push(format!(
                "security scheme `{name}`: OpenID Connect discovery is not supported; set up OAuth 2.0 by hand"
            ));
            None
        }
        _ => None,
    }
}

/// The first usable auth of a `security` requirement list.
fn security_auth(ctx: &mut Ctx, requirements: &Value) -> Option<Auth> {
    let list = requirements.as_array()?;
    if list.is_empty() || list.iter().all(|r| r.as_object().is_some_and(Map::is_empty)) {
        return Some(Auth::None);
    }
    let schemes = ctx
        .root
        .pointer("/components/securitySchemes")
        .or_else(|| ctx.root.get("securityDefinitions"))
        .cloned()
        .unwrap_or(Value::Null);
    for req in list {
        for (name, scopes) in req.as_object()? {
            let scopes: Vec<String> = scopes
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if let Some(scheme) = schemes.get(name) {
                if let Some(a) = scheme_to_auth(ctx, name, scheme, &scopes) {
                    return Some(a);
                }
            }
        }
    }
    None
}

const METHODS: [(&str, HttpMethod); 7] = [
    ("get", HttpMethod::Get),
    ("post", HttpMethod::Post),
    ("put", HttpMethod::Put),
    ("patch", HttpMethod::Patch),
    ("delete", HttpMethod::Delete),
    ("head", HttpMethod::Head),
    ("options", HttpMethod::Options),
];

/// Imports an OpenAPI 3.x or Swagger 2.0 document (JSON or YAML).
pub fn import(text: &str) -> Result<Imported, OpenApiError> {
    let doc = parse_document(text)?;
    if doc.get("openapi").is_none() && doc.get("swagger").is_none() {
        return Err(OpenApiError::NotOpenApi);
    }
    let swagger2 = doc.get("swagger").is_some();
    let mut ctx = Ctx { root: &doc, warnings: Vec::new() };
    let title = doc.pointer("/info/title").and_then(Value::as_str).unwrap_or("Imported API");
    let mut collection = Collection::new(title);
    collection.description = doc.pointer("/info/description").and_then(Value::as_str).unwrap_or("").to_string();
    collection.variables.push(KeyValue::new("baseUrl", base_url(&doc)));
    if let Some(a) = doc.get("security").and_then(|s| security_auth(&mut ctx, s)) {
        collection.auth = a;
    }
    let mut folders: Vec<Folder> = Vec::new();
    let mut loose: Vec<Item> = Vec::new();
    let mut path_vars: Vec<(String, String)> = Vec::new();

    let paths = doc.get("paths").and_then(Value::as_object).cloned().unwrap_or_default();
    for (path, item) in &paths {
        let item = ctx.deref(item).clone();
        let shared_params = item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        for (key, method) in METHODS {
            let Some(op) = item.get(key) else { continue };
            let name = op
                .get("summary")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .or_else(|| op.get("operationId").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} {path}", key.to_ascii_uppercase()));
            let mut req = Request::new(method, format!("{{{{baseUrl}}}}{}", path_to_vars(path)));
            req.name = name;
            req.description = [op.get("summary"), op.get("description")]
                .iter()
                .filter_map(|v| v.and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n");
            if op.get("deprecated").and_then(Value::as_bool) == Some(true) {
                req.description = format!("**Deprecated.** {}", req.description);
            }
            // Parameters: operation-level ones override path-level ones with the same name + in.
            let mut params: Vec<Value> = Vec::new();
            for p in shared_params.iter().chain(op.get("parameters").and_then(Value::as_array).into_iter().flatten()) {
                let p = ctx.deref(p).clone();
                let id = (p.get("name").cloned(), p.get("in").cloned());
                params.retain(|q| (q.get("name").cloned(), q.get("in").cloned()) != id);
                params.push(p);
            }
            let mut form_fields: Vec<KeyValue> = Vec::new();
            for p in &params {
                let pname = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
                let value = as_text(&p.get("example").cloned().unwrap_or_else(|| {
                    ctx.example(p.get("schema").unwrap_or(p), 0) // Swagger 2.0 puts type on the parameter
                }));
                let mut kv = KeyValue::new(pname.clone(), value.clone());
                kv.enabled = required || !value.is_empty();
                kv.description = p.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                match p.get("in").and_then(Value::as_str) {
                    Some("query") => req.params.push(kv),
                    Some("header") if !pname.eq_ignore_ascii_case("authorization") => req.headers.push(kv),
                    Some("path") => {
                        if !path_vars.iter().any(|(k, _)| *k == pname) {
                            path_vars.push((pname, if value.is_empty() { "1".into() } else { value }));
                        }
                    }
                    Some("formData") => form_fields.push(kv),
                    Some("body") if swagger2 => {
                        let ex = ctx.example(p.get("schema").unwrap_or(&Value::Null), 0);
                        req.body = Body::Raw {
                            content: serde_json::to_string_pretty(&ex).unwrap_or_default(),
                            language: RawLanguage::Json,
                        };
                    }
                    Some("cookie") => ctx.warnings.push(format!("{}: cookie parameter `{pname}` skipped", req.name)),
                    _ => {}
                }
            }
            if !form_fields.is_empty() {
                req.body = Body::FormUrlEncoded { fields: form_fields };
            }
            if let Some(body) = op.get("requestBody").map(|b| ctx.deref(b).clone()) {
                if let Some(content) = body.get("content").and_then(Value::as_object) {
                    let pick = content.keys().find(|k| k.contains("json")).or_else(|| content.keys().next()).cloned();
                    if let Some(ct) = pick {
                        let media = &content[&ct];
                        if ct.contains("x-www-form-urlencoded") || ct.contains("multipart") {
                            let ex = ctx.example(media.get("schema").unwrap_or(&Value::Null), 0);
                            let fields: Vec<KeyValue> = ex
                                .as_object()
                                .map(|m| m.iter().map(|(k, v)| KeyValue::new(k.clone(), as_text(v))).collect())
                                .unwrap_or_default();
                            req.body = if ct.contains("multipart") {
                                Body::Multipart {
                                    fields: fields
                                        .into_iter()
                                        .map(|kv| crate::model::FormField {
                                            key: kv.key,
                                            value: kv.value,
                                            ..Default::default()
                                        })
                                        .collect(),
                                }
                            } else {
                                Body::FormUrlEncoded { fields }
                            };
                        } else {
                            let ex = ctx.media_example(media);
                            let language = if ct.contains("json") {
                                RawLanguage::Json
                            } else if ct.contains("xml") {
                                RawLanguage::Xml
                            } else {
                                RawLanguage::Text
                            };
                            let content = match (&ex, language) {
                                (Value::String(s), _) => s.clone(),
                                (v, _) => serde_json::to_string_pretty(v).unwrap_or_default(),
                            };
                            req.body = Body::Raw { content, language };
                            if language == RawLanguage::Text {
                                req.headers.push(KeyValue::new("Content-Type", ct.clone()));
                            }
                        }
                    }
                }
            }
            // Accept the first response media type.
            let responses = op.get("responses").and_then(Value::as_object).cloned().unwrap_or_default();
            if let Some(ct) = responses
                .values()
                .filter_map(|r| {
                    ctx.deref(r).get("content").and_then(Value::as_object).and_then(|c| c.keys().next().cloned())
                })
                .next()
            {
                if !req.headers.iter().any(|h| h.key.eq_ignore_ascii_case("accept")) {
                    req.headers.push(KeyValue::new("Accept", ct));
                }
            }
            // Response examples become saved examples (and feed the mock server).
            for (code, resp) in &responses {
                let resp = ctx.deref(resp).clone();
                let status: u16 = code.parse().unwrap_or(200);
                let content = resp.get("content").and_then(Value::as_object).cloned().unwrap_or_default();
                let schema_ex = resp.get("schema").map(|s| ctx.example(s, 0)); // Swagger 2.0
                let (ct, body) = match content.iter().next() {
                    Some((ct, media)) => (ct.clone(), Some(ctx.media_example(media))),
                    None => ("application/json".to_string(), schema_ex),
                };
                let Some(body) = body.filter(|b| !b.is_null()) else { continue };
                req.examples.push(Example {
                    id: crate::model::new_id(),
                    name: format!("{code} {}", resp.get("description").and_then(Value::as_str).unwrap_or(""))
                        .trim()
                        .to_string(),
                    status,
                    headers: vec![KeyValue::new("Content-Type", ct)],
                    body: match body {
                        Value::String(s) => s,
                        v => serde_json::to_string_pretty(&v).unwrap_or_default(),
                    },
                });
            }
            if let Some(sec) = op.get("security") {
                if let Some(a) = security_auth(&mut ctx, sec) {
                    req.auth = a;
                }
            }
            let tag = op.pointer("/tags/0").and_then(Value::as_str).map(str::to_string);
            match tag {
                Some(t) => match folders.iter_mut().find(|f| f.name == t) {
                    Some(f) => f.items.push(Item::Request(req)),
                    None => {
                        let mut f = Folder::new(t.clone());
                        f.description = doc
                            .get("tags")
                            .and_then(Value::as_array)
                            .and_then(|tags| {
                                tags.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(t.as_str()))
                            })
                            .and_then(|x| x.get("description").and_then(Value::as_str))
                            .unwrap_or("")
                            .to_string();
                        f.items.push(Item::Request(req));
                        folders.push(f);
                    }
                },
                None => loose.push(Item::Request(req)),
            }
        }
    }
    for (k, v) in path_vars {
        collection.variables.push(KeyValue::new(k, v));
    }
    if let Some(w) = doc.get("webhooks").and_then(Value::as_object).filter(|w| !w.is_empty()) {
        ctx.warnings.push(format!(
            "{} webhook(s) are not imported (they are requests the API sends, not ones you call)",
            w.len()
        ));
    }
    collection.items = folders.into_iter().map(Item::Folder).chain(loose).collect();
    let warnings = ctx.warnings;
    Ok(Imported { collection, warnings })
}

// ---- export ------------------------------------------------------------------------

/// `{{id}}` → `{id}`; returns the path and the parameter names.
fn vars_to_path(path: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("}}") {
            Some(j) => {
                let name = rest[i + 2..i + 2 + j].trim().to_string();
                out.push_str(&format!("{{{name}}}"));
                names.push(name);
                rest = &rest[i + 2 + j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    (out, names)
}

/// (server, path) of a request URL: `{{baseUrl}}/pets/{{id}}` → (`{{baseUrl}}`, `/pets/{{id}}`).
fn split_url(url: &str) -> (String, String) {
    let url = url.split(['?', '#']).next().unwrap_or("").trim();
    if let Some(rest) = url.strip_prefix("{{") {
        if let Some(end) = rest.find("}}") {
            let server = format!("{{{{{}}}}}", &rest[..end]);
            let path = &rest[end + 2..];
            return (server, if path.is_empty() { "/".into() } else { path.to_string() });
        }
    }
    match url.find("://").map(|i| i + 3) {
        Some(start) => {
            let path_at = url[start..].find('/').map(|p| start + p).unwrap_or(url.len());
            let path = &url[path_at..];
            (url[..path_at].to_string(), if path.is_empty() { "/".into() } else { path.to_string() })
        }
        None => (String::new(), if url.starts_with('/') { url.to_string() } else { format!("/{url}") }),
    }
}

fn infer_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => json!({
            "type": "object",
            "properties": m.iter().map(|(k, v)| (k.clone(), infer_schema(v))).collect::<Map<_, _>>()
        }),
        Value::Array(a) => json!({ "type": "array", "items": a.first().map(infer_schema).unwrap_or(json!({})) }),
        Value::String(_) => json!({ "type": "string" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(n) if n.is_i64() || n.is_u64() => json!({ "type": "integer" }),
        Value::Number(_) => json!({ "type": "number" }),
        Value::Null => json!({}),
    }
}

/// Exports a collection as an OpenAPI 3.0.3 document (JSON value; see [`to_yaml`]).
pub fn export(c: &Collection) -> Value {
    let mut paths = Map::new();
    let mut servers: Vec<String> = Vec::new();
    let mut schemes = Map::new();
    let mut tags: Vec<String> = Vec::new();
    let mut used_ids = std::collections::HashSet::new();
    let var = |name: &str| c.variables.iter().find(|v| v.key == name).map(|v| v.value.clone());

    let mut add_scheme = |auth: &Auth| -> Option<String> {
        let (name, scheme) = match auth {
            Auth::Basic { .. } => ("basicAuth", json!({ "type": "http", "scheme": "basic" })),
            Auth::Bearer { .. } => ("bearerAuth", json!({ "type": "http", "scheme": "bearer" })),
            Auth::Digest { .. } => ("digestAuth", json!({ "type": "http", "scheme": "digest" })),
            Auth::ApiKey { key, location, .. } => (
                "apiKeyAuth",
                json!({ "type": "apiKey", "name": key, "in": if *location == ApiKeyLocation::Query { "query" } else { "header" } }),
            ),
            Auth::OAuth2(o) => {
                let flow = match o.grant {
                    OAuth2Grant::ClientCredentials => {
                        json!({ "clientCredentials": { "tokenUrl": o.token_url, "scopes": {} } })
                    }
                    OAuth2Grant::Password => json!({ "password": { "tokenUrl": o.token_url, "scopes": {} } }),
                    OAuth2Grant::AuthorizationCode => {
                        json!({ "authorizationCode": { "authorizationUrl": o.auth_url, "tokenUrl": o.token_url, "scopes": {} } })
                    }
                };
                ("oauth2", json!({ "type": "oauth2", "flows": flow }))
            }
            _ => return None,
        };
        schemes.insert(name.to_string(), scheme);
        Some(name.to_string())
    };
    let collection_scheme = add_scheme(&c.auth);

    for (folder, r) in c.flatten() {
        if !r.kind.is_http() {
            continue;
        }
        let (server, raw_path) = split_url(&r.url);
        let resolved_server = match server.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
            Some(v) => var(v).unwrap_or(server.clone()),
            None => server.clone(),
        };
        if !resolved_server.is_empty() && !servers.contains(&resolved_server) {
            servers.push(resolved_server);
        }
        let (path, path_params) = vars_to_path(&raw_path);
        let mut op = Map::new();
        op.insert("summary".into(), json!(r.name));
        if !r.description.trim().is_empty() {
            op.insert("description".into(), json!(r.description));
        }
        let id: String = r.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if !id.is_empty() {
            // operationId must be unique: same-named requests in other folders get a number.
            let base = format!("{}{}", r.method.as_str().to_ascii_lowercase(), id);
            let mut unique = base.clone();
            let mut n = 2;
            while !used_ids.insert(unique.clone()) {
                unique = format!("{base}{n}");
                n += 1;
            }
            op.insert("operationId".into(), json!(unique));
        }
        if let Some(t) = folder.first() {
            op.insert("tags".into(), json!([t]));
            if !tags.contains(t) {
                tags.push(t.clone());
            }
        }
        let mut params: Vec<Value> = path_params
            .iter()
            .map(|n| {
                let mut p = json!({ "name": n, "in": "path", "required": true, "schema": { "type": "string" } });
                if let Some(v) = var(n) {
                    p["example"] = json!(v);
                }
                p
            })
            .collect();
        for q in r.params.iter().filter(|q| !q.key.is_empty()) {
            let mut p = json!({ "name": q.key, "in": "query", "required": q.enabled, "schema": { "type": "string" } });
            if !q.value.is_empty() && !q.value.contains("{{") {
                p["example"] = json!(q.value);
            }
            if !q.description.is_empty() {
                p["description"] = json!(q.description);
            }
            params.push(p);
        }
        for h in r.headers.iter().filter(|h| {
            !h.key.is_empty()
                && !["content-type", "accept", "authorization"].contains(&h.key.to_ascii_lowercase().as_str())
        }) {
            params
                .push(json!({ "name": h.key, "in": "header", "required": h.enabled, "schema": { "type": "string" } }));
        }
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }
        let body = match &r.body {
            Body::Raw { content, language } if !content.trim().is_empty() => {
                let (ct, schema, example) = match language {
                    RawLanguage::Json => match serde_json::from_str::<Value>(content) {
                        Ok(v) => ("application/json", infer_schema(&v), v),
                        Err(_) => ("application/json", json!({}), json!(content)),
                    },
                    RawLanguage::Xml => ("application/xml", json!({ "type": "string" }), json!(content)),
                    _ => ("text/plain", json!({ "type": "string" }), json!(content)),
                };
                Some(json!({ "content": { ct: { "schema": schema, "example": example } } }))
            }
            Body::FormUrlEncoded { fields } => {
                Some(json!({ "content": { "application/x-www-form-urlencoded": { "schema": {
                "type": "object",
                "properties": fields.iter().filter(|f| !f.key.is_empty()).map(|f| (f.key.clone(), json!({ "type": "string", "example": f.value }))).collect::<Map<_, _>>()
            } } } }))
            }
            Body::Multipart { fields } => Some(json!({ "content": { "multipart/form-data": { "schema": {
                "type": "object",
                "properties": fields.iter().filter(|f| !f.key.is_empty()).map(|f| (f.key.clone(), if f.is_file { json!({ "type": "string", "format": "binary" }) } else { json!({ "type": "string" }) })).collect::<Map<_, _>>()
            } } } })),
            Body::GraphQl { query, .. } => {
                Some(json!({ "content": { "application/json": { "example": { "query": query } } } }))
            }
            _ => None,
        };
        if let Some(b) = body {
            op.insert("requestBody".into(), b);
        }
        let mut responses = Map::new();
        for e in &r.examples {
            let ct = e
                .headers
                .iter()
                .find(|h| h.key.eq_ignore_ascii_case("content-type"))
                .map(|h| h.value.clone())
                .unwrap_or_else(|| "application/json".into());
            let example = serde_json::from_str::<Value>(&e.body).unwrap_or(json!(e.body));
            responses.insert(
                e.status.to_string(),
                json!({ "description": if e.name.is_empty() { "Example".to_string() } else { e.name.clone() }, "content": { ct: { "example": example } } }),
            );
        }
        if responses.is_empty() {
            responses.insert("200".into(), json!({ "description": "OK" }));
        }
        op.insert("responses".into(), Value::Object(responses));
        match &r.auth {
            Auth::Inherit => {}
            Auth::None => {
                op.insert("security".into(), json!([]));
            }
            other => {
                if let Some(name) = add_scheme(other) {
                    if Some(&name) != collection_scheme.as_ref() {
                        op.insert("security".into(), json!([{ name: [] }]));
                    }
                }
            }
        }
        let entry = paths.entry(path).or_insert_with(|| json!({}));
        entry[r.method.as_str().to_ascii_lowercase()] = Value::Object(op);
    }

    let mut doc = json!({
        "openapi": "3.0.3",
        "info": { "title": c.name, "version": "1.0.0" },
        "paths": paths,
    });
    if !c.description.trim().is_empty() {
        doc["info"]["description"] = json!(c.description);
    }
    if !servers.is_empty() {
        doc["servers"] = Value::Array(servers.into_iter().map(|u| json!({ "url": u })).collect());
    }
    if !tags.is_empty() {
        doc["tags"] = Value::Array(tags.into_iter().map(|t| json!({ "name": t })).collect());
    }
    if !schemes.is_empty() {
        doc["components"] = json!({ "securitySchemes": schemes });
    }
    if let Some(name) = collection_scheme {
        doc["security"] = json!([{ name: [] }]);
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETSTORE: &str = r#"
openapi: 3.0.3
info:
  title: Petstore
  description: A sample API
  version: 1.0.0
servers:
  - url: https://{env}.petstore.test/v1
    variables:
      env:
        default: api
tags:
  - name: pets
    description: Everything about pets
security:
  - bearer: []
paths:
  /pets:
    get:
      summary: List pets
      tags: [pets]
      parameters:
        - name: limit
          in: query
          schema: { type: integer, example: 20 }
        - $ref: '#/components/parameters/TraceId'
      responses:
        '200':
          description: A list
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Pet' }
    post:
      summary: Create a pet
      tags: [pets]
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses:
        '201': { description: Created }
  /pets/{petId}:
    parameters:
      - name: petId
        in: path
        required: true
        schema: { type: string, example: "42" }
    get:
      operationId: showPetById
      tags: [pets]
      security:
        - apiKey: []
      responses:
        '200':
          description: One pet
          content:
            application/json:
              example: { id: 42, name: Kucing }
        '404':
          $ref: '#/components/responses/NotFound'
  /health:
    get:
      security: []
      responses:
        '200': { description: OK }
components:
  parameters:
    TraceId:
      name: X-Trace-Id
      in: header
      schema: { type: string }
  responses:
    NotFound:
      description: Not found
      content:
        application/json:
          example: { error: not found }
  schemas:
    Pet:
      type: object
      required: [name]
      properties:
        id: { type: integer, format: int64 }
        name: { type: string, example: Budi }
        tag: { type: string, enum: [cat, dog] }
  securitySchemes:
    bearer: { type: http, scheme: bearer }
    apiKey: { type: apiKey, name: X-API-Key, in: header }
"#;

    #[test]
    fn imports_petstore() {
        let Imported { collection: c, warnings } = import(PETSTORE).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(c.name, "Petstore");
        assert_eq!(c.variables[0], KeyValue::new("baseUrl", "https://api.petstore.test/v1"));
        assert!(c.variables.iter().any(|v| v.key == "petId" && v.value == "42"));
        assert_eq!(c.auth, Auth::Bearer { token: "{{bearerToken}}".into() });
        let flat = c.flatten();
        assert_eq!(flat.len(), 4);
        let (folder, list) = &flat[0];
        assert_eq!(folder, &vec!["pets".to_string()]);
        assert_eq!(list.url, "{{baseUrl}}/pets");
        assert_eq!(list.params[0].key, "limit");
        assert_eq!(list.params[0].value, "20");
        assert!(list.headers.iter().any(|h| h.key == "X-Trace-Id"));
        let create = &flat[1].1;
        let Body::Raw { content, .. } = &create.body else { panic!("{:?}", create.body) };
        let v: Value = serde_json::from_str(content).unwrap();
        assert_eq!(v, json!({ "id": 0, "name": "Budi", "tag": "cat" }));
        let show = &flat[2].1;
        assert_eq!(show.name, "showPetById");
        assert_eq!(show.url, "{{baseUrl}}/pets/{{petId}}");
        assert!(matches!(show.auth, Auth::ApiKey { .. }));
        assert_eq!(show.examples.len(), 2);
        assert_eq!(show.examples[1].status, 404);
        assert!(show.examples[1].body.contains("not found"));
        let health = &flat[3].1;
        assert_eq!(health.auth, Auth::None);
        assert!(flat[0].1.examples[0].body.contains("Budi"), "array example from the schema");
    }

    #[test]
    fn export_then_import_keeps_operations() {
        let c = import(PETSTORE).unwrap().collection;
        let doc = export(&c);
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["servers"][0]["url"], "https://api.petstore.test/v1");
        assert!(doc["paths"]["/pets/{petId}"]["get"]["parameters"][0]["name"] == "petId");
        assert_eq!(doc["paths"]["/health"]["get"]["security"], json!([]));
        assert!(doc["components"]["securitySchemes"]["bearerAuth"].is_object());
        let yaml = to_yaml(&doc);
        assert!(is_openapi(&yaml));
        let back = import(&yaml).unwrap().collection;
        let names: Vec<_> = back.flatten().iter().map(|(_, r)| (r.method, r.url.clone())).collect();
        let orig: Vec<_> = c.flatten().iter().map(|(_, r)| (r.method, r.url.clone())).collect();
        assert_eq!(names, orig);
    }

    #[test]
    fn operation_ids_are_unique() {
        let mut c = Collection::new("Dup");
        for folder in ["A", "B"] {
            let mut f = Folder::new(folder);
            let mut r = Request::new(HttpMethod::Get, format!("https://x.test/{folder}"));
            r.name = "List".into();
            f.items.push(Item::Request(r));
            c.items.push(Item::Folder(f));
        }
        let doc = export(&c);
        assert_eq!(doc["paths"]["/A"]["get"]["operationId"], "getList");
        assert_eq!(doc["paths"]["/B"]["get"]["operationId"], "getList2");
    }

    #[test]
    fn swagger2_basics() {
        let doc = r#"{"swagger":"2.0","info":{"title":"Old"},"host":"old.test","basePath":"/api","schemes":["http"],
            "paths":{"/items":{"post":{"parameters":[{"name":"body","in":"body","schema":{"type":"object","properties":{"n":{"type":"integer"}}}}],
            "responses":{"200":{"description":"ok","schema":{"type":"object","properties":{"ok":{"type":"boolean"}}}}}}}}}"#;
        let c = import(doc).unwrap().collection;
        assert_eq!(c.variables[0].value, "http://old.test/api");
        let r = &c.flatten()[0].1;
        assert!(matches!(&r.body, Body::Raw { content, .. } if content.contains("\"n\": 0")));
        assert_eq!(r.examples[0].body.replace(char::is_whitespace, ""), r#"{"ok":false}"#);
        assert!(matches!(import("{}"), Err(OpenApiError::NotOpenApi)));
    }
}
