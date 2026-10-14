//! Data model shared by the GUI, the CLI and the storage layer.
//!
//! Everything here is plain data (serde-serialisable) so a workspace can be
//! written to disk as human-readable JSON and committed to git.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Generates a fresh random identifier.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// HTTP methods supported by PakPos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const ALL: [HttpMethod; 7] = [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Head,
        HttpMethod::Options,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
        }
    }

    /// Parses a method name case-insensitively.
    pub fn parse(s: &str) -> Option<HttpMethod> {
        HttpMethod::ALL.into_iter().find(|m| m.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A generic key/value row (query params, headers, form fields, variables).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

fn default_true() -> bool {
    true
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), enabled: true, description: String::new() }
    }

    pub fn is_blank(&self) -> bool {
        self.key.trim().is_empty() && self.value.trim().is_empty()
    }
}

impl Default for KeyValue {
    fn default() -> Self {
        Self::new("", "")
    }
}

/// Content type hint for raw bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RawLanguage {
    #[default]
    Json,
    Text,
    Xml,
    Html,
    Javascript,
}

impl RawLanguage {
    pub const ALL: [RawLanguage; 5] =
        [RawLanguage::Json, RawLanguage::Text, RawLanguage::Xml, RawLanguage::Html, RawLanguage::Javascript];

    pub fn label(&self) -> &'static str {
        match self {
            RawLanguage::Json => "JSON",
            RawLanguage::Text => "Text",
            RawLanguage::Xml => "XML",
            RawLanguage::Html => "HTML",
            RawLanguage::Javascript => "JavaScript",
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            RawLanguage::Json => "application/json",
            RawLanguage::Text => "text/plain",
            RawLanguage::Xml => "application/xml",
            RawLanguage::Html => "text/html",
            RawLanguage::Javascript => "application/javascript",
        }
    }
}

/// A multipart form field: either text or a file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub is_file: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for FormField {
    fn default() -> Self {
        Self { key: String::new(), value: String::new(), is_file: false, enabled: true }
    }
}

/// Request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Body {
    #[default]
    None,
    Raw {
        content: String,
        #[serde(default)]
        language: RawLanguage,
    },
    FormUrlEncoded {
        fields: Vec<KeyValue>,
    },
    Multipart {
        fields: Vec<FormField>,
    },
    /// Send the content of a file as the body.
    Binary {
        path: String,
    },
    /// GraphQL over HTTP: sent as `{"query", "variables", "operationName"}` JSON.
    #[serde(rename = "graphql")]
    GraphQl {
        query: String,
        /// JSON object text; may contain `{{variables}}`.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        variables: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        operation_name: String,
    },
}

impl Body {
    pub fn kind(&self) -> BodyKind {
        match self {
            Body::None => BodyKind::None,
            Body::Raw { .. } => BodyKind::Raw,
            Body::FormUrlEncoded { .. } => BodyKind::FormUrlEncoded,
            Body::Multipart { .. } => BodyKind::Multipart,
            Body::Binary { .. } => BodyKind::Binary,
            Body::GraphQl { .. } => BodyKind::GraphQl,
        }
    }

    /// Switches to another body kind, keeping data when possible.
    pub fn convert_to(&self, kind: BodyKind) -> Body {
        if self.kind() == kind {
            return self.clone();
        }
        match kind {
            BodyKind::None => Body::None,
            BodyKind::Raw => Body::Raw { content: String::new(), language: RawLanguage::Json },
            BodyKind::FormUrlEncoded => Body::FormUrlEncoded {
                fields: match self {
                    Body::Multipart { fields } => fields
                        .iter()
                        .filter(|f| !f.is_file)
                        .map(|f| KeyValue {
                            key: f.key.clone(),
                            value: f.value.clone(),
                            enabled: f.enabled,
                            description: String::new(),
                        })
                        .collect(),
                    _ => Vec::new(),
                },
            },
            BodyKind::Multipart => Body::Multipart {
                fields: match self {
                    Body::FormUrlEncoded { fields } => fields
                        .iter()
                        .map(|f| FormField {
                            key: f.key.clone(),
                            value: f.value.clone(),
                            is_file: false,
                            enabled: f.enabled,
                        })
                        .collect(),
                    _ => Vec::new(),
                },
            },
            BodyKind::Binary => Body::Binary { path: String::new() },
            BodyKind::GraphQl => Body::GraphQl {
                query: match self {
                    Body::Raw { content, .. } => content.clone(),
                    _ => "query {\n  \n}".into(),
                },
                variables: String::new(),
                operation_name: String::new(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    None,
    Raw,
    FormUrlEncoded,
    Multipart,
    Binary,
    GraphQl,
}

impl BodyKind {
    pub const ALL: [BodyKind; 6] = [
        BodyKind::None,
        BodyKind::Raw,
        BodyKind::FormUrlEncoded,
        BodyKind::Multipart,
        BodyKind::Binary,
        BodyKind::GraphQl,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            BodyKind::None => "none",
            BodyKind::Raw => "raw",
            BodyKind::FormUrlEncoded => "x-www-form-urlencoded",
            BodyKind::Multipart => "form-data",
            BodyKind::Binary => "binary",
            BodyKind::GraphQl => "GraphQL",
        }
    }
}

/// Where an API key is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

/// Authentication settings for a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    /// Use the auth of the parent folder / collection.
    #[default]
    Inherit,
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        key: String,
        value: String,
        #[serde(default)]
        location: ApiKeyLocation,
    },
    /// OAuth 2.0; tokens are kept in the workspace token store, not here.
    #[serde(rename = "oauth2")]
    OAuth2(OAuth2Config),
}

/// OAuth 2.0 grant types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OAuth2Grant {
    #[default]
    ClientCredentials,
    Password,
    AuthorizationCode,
}

impl OAuth2Grant {
    pub const ALL: [OAuth2Grant; 3] =
        [OAuth2Grant::ClientCredentials, OAuth2Grant::Password, OAuth2Grant::AuthorizationCode];

    pub fn label(&self) -> &'static str {
        match self {
            OAuth2Grant::ClientCredentials => "Client credentials",
            OAuth2Grant::Password => "Password",
            OAuth2Grant::AuthorizationCode => "Authorization code",
        }
    }

    /// Grants that can run without a browser (CLI, runner, MCP).
    pub fn is_interactive(&self) -> bool {
        matches!(self, OAuth2Grant::AuthorizationCode)
    }
}

/// How the client authenticates at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuthMethod {
    /// HTTP Basic header (`client_secret_basic`).
    #[default]
    Header,
    /// `client_id` / `client_secret` in the form body (`client_secret_post`).
    Body,
}

fn default_prefix() -> String {
    "Bearer".into()
}

fn default_redirect() -> String {
    "http://127.0.0.1:7777/callback".into()
}

/// OAuth 2.0 settings. Every field may contain `{{variables}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2Config {
    #[serde(default)]
    pub grant: OAuth2Grant,
    #[serde(default)]
    pub token_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub audience: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    #[serde(default = "default_redirect")]
    pub redirect_uri: String,
    #[serde(default = "default_true")]
    pub use_pkce: bool,
    #[serde(default)]
    pub client_auth: ClientAuthMethod,
    #[serde(default = "default_prefix")]
    pub header_prefix: String,
}

impl Default for OAuth2Config {
    fn default() -> Self {
        Self {
            grant: OAuth2Grant::ClientCredentials,
            token_url: String::new(),
            auth_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            audience: String::new(),
            username: String::new(),
            password: String::new(),
            redirect_uri: default_redirect(),
            use_pkce: true,
            client_auth: ClientAuthMethod::Header,
            header_prefix: default_prefix(),
        }
    }
}

impl OAuth2Config {
    /// Identifies a token in the store: same server, client, user and scope
    /// means the same token, wherever the config is used.
    pub fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.token_url.trim(),
            self.client_id.trim(),
            self.scope.trim(),
            self.audience.trim(),
            self.username.trim(),
            match self.grant {
                OAuth2Grant::ClientCredentials => "cc",
                OAuth2Grant::Password => "pw",
                OAuth2Grant::AuthorizationCode => "code",
            }
        )
    }

    /// Copy with every `{{variable}}` resolved.
    pub fn resolved(&self, scope: &crate::vars::VarScope) -> OAuth2Config {
        let r = |s: &str| scope.resolve(s);
        OAuth2Config {
            grant: self.grant,
            token_url: r(&self.token_url),
            auth_url: r(&self.auth_url),
            client_id: r(&self.client_id),
            client_secret: r(&self.client_secret),
            scope: r(&self.scope),
            audience: r(&self.audience),
            username: r(&self.username),
            password: r(&self.password),
            redirect_uri: r(&self.redirect_uri),
            use_pkce: self.use_pkce,
            client_auth: self.client_auth,
            header_prefix: r(&self.header_prefix),
        }
    }
}

impl Auth {
    pub fn label(&self) -> &'static str {
        match self {
            Auth::Inherit => "Inherit from parent",
            Auth::None => "No Auth",
            Auth::Basic { .. } => "Basic Auth",
            Auth::Bearer { .. } => "Bearer Token",
            Auth::ApiKey { .. } => "API Key",
            Auth::OAuth2(_) => "OAuth 2.0",
        }
    }

    /// One default instance of every variant, for pickers in the UI.
    pub fn templates() -> Vec<Auth> {
        vec![
            Auth::Inherit,
            Auth::None,
            Auth::Basic { username: String::new(), password: String::new() },
            Auth::Bearer { token: String::new() },
            Auth::ApiKey { key: "X-API-Key".into(), value: String::new(), location: ApiKeyLocation::Header },
            Auth::OAuth2(OAuth2Config::default()),
        ]
    }

    pub fn same_variant(&self, other: &Auth) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// A declarative test that runs against a response (see [`crate::assertions`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// What to inspect, e.g. `status`, `header.Content-Type`, `body`,
    /// `json.data.items[0].id`, `time`.
    pub target: String,
    pub op: crate::assertions::Operator,
    #[serde(default)]
    pub expected: String,
}

impl Default for Assertion {
    fn default() -> Self {
        Self { enabled: true, target: "status".into(), op: crate::assertions::Operator::Equals, expected: "200".into() }
    }
}

/// Values pulled out of a response and stored into the active environment,
/// e.g. save `json.token` into `{{token}}` after a login request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Extraction {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub variable: String,
    /// Same syntax as [`Assertion::target`].
    pub source: String,
}

/// Protocol of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    #[default]
    Http,
    /// URL, params, headers and auth are used for the upgrade; the raw body is
    /// the message composer draft.
    WebSocket,
}

impl RequestKind {
    pub fn is_http(&self) -> bool {
        *self == RequestKind::Http
    }
}

/// A single request definition (HTTP, or another protocol per [`RequestKind`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "RequestKind::is_http")]
    pub kind: RequestKind,
    #[serde(default)]
    pub method: HttpMethod,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<Assertion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extractions: Vec<Extraction>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "New Request".into(),
            kind: RequestKind::Http,
            method: HttpMethod::Get,
            url: String::new(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            assertions: Vec::new(),
            extractions: Vec::new(),
            description: String::new(),
        }
    }
}

impl Request {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            name: if url.is_empty() { "New Request".into() } else { url.clone() },
            method,
            url,
            ..Default::default()
        }
    }

    /// Returns a copy with a fresh id (used for "Duplicate").
    pub fn duplicate(&self) -> Self {
        Self { id: new_id(), name: format!("{} copy", self.name), ..self.clone() }
    }
}

/// A folder inside a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: new_id(), name: name.into(), items: Vec::new(), auth: Auth::Inherit, description: String::new() }
    }
}

/// An entry in a collection tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    Folder(Folder),
    Request(Request),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::Folder(f) => &f.id,
            Item::Request(r) => &r.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Item::Folder(f) => &f.name,
            Item::Request(r) => &r.name,
        }
    }
}

/// A named collection of requests, the unit of sharing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub items: Vec<Item>,
    /// Collection-level variables (lower priority than environment ones).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<KeyValue>,
    /// Collection-level auth applied to requests that inherit.
    #[serde(default = "auth_none")]
    pub auth: Auth,
}

fn auth_none() -> Auth {
    Auth::None
}

impl Collection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            description: String::new(),
            items: Vec::new(),
            variables: Vec::new(),
            auth: Auth::None,
        }
    }

    /// Depth-first list of every request with the effective (inherited) auth.
    pub fn flatten(&self) -> Vec<(Vec<String>, Request)> {
        fn walk(items: &[Item], path: &mut Vec<String>, parent_auth: &Auth, out: &mut Vec<(Vec<String>, Request)>) {
            for item in items {
                match item {
                    Item::Request(r) => {
                        let mut r = r.clone();
                        if r.auth == Auth::Inherit {
                            r.auth = parent_auth.clone();
                        }
                        out.push((path.clone(), r));
                    }
                    Item::Folder(f) => {
                        let auth = if f.auth == Auth::Inherit { parent_auth.clone() } else { f.auth.clone() };
                        path.push(f.name.clone());
                        walk(&f.items, path, &auth, out);
                        path.pop();
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut Vec::new(), &self.auth, &mut out);
        out
    }

    pub fn find_request(&self, id: &str) -> Option<&Request> {
        fn walk<'a>(items: &'a [Item], id: &str) -> Option<&'a Request> {
            for item in items {
                match item {
                    Item::Request(r) if r.id == id => return Some(r),
                    Item::Folder(f) => {
                        if let Some(r) = walk(&f.items, id) {
                            return Some(r);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        walk(&self.items, id)
    }

    pub fn find_request_mut(&mut self, id: &str) -> Option<&mut Request> {
        fn walk<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut Request> {
            for item in items {
                match item {
                    Item::Request(r) if r.id == id => return Some(r),
                    Item::Folder(f) => {
                        if let Some(r) = walk(&mut f.items, id) {
                            return Some(r);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// Effective auth for a request id, resolving `Inherit` through folders.
    pub fn effective_auth(&self, request_id: &str) -> Option<Auth> {
        self.flatten().into_iter().find(|(_, r)| r.id == request_id).map(|(_, r)| r.auth)
    }

    /// Removes an item (request or folder) by id anywhere in the tree.
    pub fn remove_item(&mut self, id: &str) -> Option<Item> {
        fn walk(items: &mut Vec<Item>, id: &str) -> Option<Item> {
            if let Some(pos) = items.iter().position(|i| i.id() == id) {
                return Some(items.remove(pos));
            }
            for item in items.iter_mut() {
                if let Item::Folder(f) = item {
                    if let Some(found) = walk(&mut f.items, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    /// Finds a folder by id anywhere in the tree.
    pub fn find_folder_mut(&mut self, id: &str) -> Option<&mut Folder> {
        fn walk<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut Folder> {
            for item in items {
                if let Item::Folder(f) = item {
                    if f.id == id {
                        return Some(f);
                    }
                    if let Some(found) = walk(&mut f.items, id) {
                        return Some(found);
                    }
                }
            }
            None
        }
        walk(&mut self.items, id)
    }

    pub fn request_count(&self) -> usize {
        self.flatten().len()
    }
}

/// A set of variables, e.g. "Local", "Staging", "Production".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub variables: Vec<KeyValue>,
}

impl Environment {
    pub fn new(name: impl Into<String>) -> Self {
        Self { id: new_id(), name: name.into(), variables: Vec::new() }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.variables.iter().find(|v| v.enabled && v.key == key).map(|v| v.value.as_str())
    }

    /// Sets (or inserts) a variable.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(v) = self.variables.iter_mut().find(|v| v.key == key) {
            v.value = value;
            v.enabled = true;
        } else {
            self.variables.push(KeyValue::new(key, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_parse_roundtrip() {
        for m in HttpMethod::ALL {
            assert_eq!(HttpMethod::parse(m.as_str()), Some(m));
            assert_eq!(HttpMethod::parse(&m.as_str().to_lowercase()), Some(m));
        }
        assert_eq!(HttpMethod::parse("FOO"), None);
    }

    #[test]
    fn auth_inherits_through_folders() {
        let mut c = Collection::new("c");
        c.auth = Auth::Bearer { token: "abc".into() };
        let mut f = Folder::new("f");
        let r = Request::new(HttpMethod::Get, "http://x");
        let id = r.id.clone();
        f.items.push(Item::Request(r));
        c.items.push(Item::Folder(f));
        assert_eq!(c.effective_auth(&id), Some(Auth::Bearer { token: "abc".into() }));
    }

    #[test]
    fn remove_nested_item() {
        let mut c = Collection::new("c");
        let mut f = Folder::new("f");
        let r = Request::new(HttpMethod::Get, "http://x");
        let id = r.id.clone();
        f.items.push(Item::Request(r));
        c.items.push(Item::Folder(f));
        assert!(c.remove_item(&id).is_some());
        assert_eq!(c.request_count(), 0);
    }

    #[test]
    fn serde_roundtrip() {
        let mut c = Collection::new("demo");
        let mut r = Request::new(HttpMethod::Post, "{{base}}/users");
        r.body = Body::Raw { content: "{\"a\":1}".into(), language: RawLanguage::Json };
        c.items.push(Item::Request(r));
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: Collection = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}
