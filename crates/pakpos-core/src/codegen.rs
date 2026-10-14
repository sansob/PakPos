//! cURL import and code snippet generation (cURL, Python `requests`,
//! JavaScript `fetch`, HTTPie).

use crate::http::{PreparedBody, PreparedRequest};
use crate::model::{Auth, Body, FormField, HttpMethod, KeyValue, RawLanguage, Request};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CurlError {
    #[error("the command does not start with `curl`")]
    NotCurl,
    #[error("unterminated quote in command")]
    UnterminatedQuote,
    #[error("option `{0}` expects a value")]
    MissingValue(String),
    #[error("no URL found in command")]
    NoUrl,
}

/// Splits a shell command line into words (handles quotes, escapes and `\`
/// line continuations, which is what browsers' "Copy as cURL" produce).
pub fn shell_split(input: &str) -> Result<Vec<String>, CurlError> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => cur.push(ch),
                        None => return Err(CurlError::UnterminatedQuote),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => cur.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                cur.push('\\');
                                cur.push(ch);
                            }
                            None => return Err(CurlError::UnterminatedQuote),
                        },
                        Some(ch) => cur.push(ch),
                        None => return Err(CurlError::UnterminatedQuote),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') | Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(ch) => {
                    in_word = true;
                    cur.push(ch);
                }
                None => {}
            },
            // Windows cmd "Copy as cURL" uses ^ for continuation.
            '^' if matches!(chars.peek(), Some('\n') | Some('\r')) => {
                while matches!(chars.peek(), Some('\n') | Some('\r')) {
                    chars.next();
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

/// Parses a cURL command into a [`Request`].
pub fn parse_curl(command: &str) -> Result<Request, CurlError> {
    let words = shell_split(command.trim())?;
    let mut it = words.into_iter().peekable();
    match it.next() {
        Some(w) if w == "curl" || w.ends_with("/curl") || w.eq_ignore_ascii_case("curl.exe") => {}
        _ => return Err(CurlError::NotCurl),
    }

    let mut method: Option<HttpMethod> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut data: Vec<String> = Vec::new();
    let mut urlencoded: Vec<String> = Vec::new();
    let mut form: Vec<FormField> = Vec::new();
    let mut auth = Auth::Inherit;
    let mut get = false;

    while let Some(word) = it.next() {
        // Support --opt=value.
        let (flag, inline) = match word.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (word.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, CurlError> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            it.next().ok_or_else(|| CurlError::MissingValue(name.to_string()))
        };
        match flag.as_str() {
            "-X" | "--request" => method = HttpMethod::parse(&value(&flag)?),
            "-H" | "--header" => {
                let h = value(&flag)?;
                if let Some((k, v)) = h.split_once(':') {
                    headers.push(KeyValue::new(k.trim(), v.trim()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--json" => {
                if flag == "--json" {
                    headers.push(KeyValue::new("Content-Type", "application/json"));
                    headers.push(KeyValue::new("Accept", "application/json"));
                }
                data.push(value(&flag)?);
            }
            "--data-urlencode" => urlencoded.push(value(&flag)?),
            "-F" | "--form" | "--form-string" => {
                let f = value(&flag)?;
                let (k, v) = f.split_once('=').unwrap_or((f.as_str(), ""));
                let is_file = flag != "--form-string" && v.starts_with('@');
                form.push(FormField {
                    key: k.to_string(),
                    value: if is_file { v[1..].to_string() } else { v.to_string() },
                    is_file,
                    enabled: true,
                });
            }
            "-u" | "--user" => {
                let u = value(&flag)?;
                let (user, pass) = u.split_once(':').unwrap_or((u.as_str(), ""));
                auth = Auth::Basic {
                    username: user.to_string(),
                    password: pass.to_string(),
                };
            }
            "-A" | "--user-agent" => headers.push(KeyValue::new("User-Agent", value(&flag)?)),
            "-b" | "--cookie" => headers.push(KeyValue::new("Cookie", value(&flag)?)),
            "-e" | "--referer" => headers.push(KeyValue::new("Referer", value(&flag)?)),
            "-G" | "--get" => get = true,
            "-I" | "--head" => method = Some(HttpMethod::Head),
            "--url" => url = Some(value(&flag)?),
            // Options that take a value we don't use.
            "-o" | "--output" | "-m" | "--max-time" | "--connect-timeout" | "-x" | "--proxy"
            | "-w" | "--write-out" | "--retry" | "-c" | "--cookie-jar" | "--cacert" | "--cert"
            | "--key" => {
                let _ = value(&flag)?;
            }
            f if f.starts_with('-') => {} // flags like -s -L -k --compressed
            _ => {
                if url.is_none() {
                    url = Some(word);
                }
            }
        }
    }

    let url = url.ok_or(CurlError::NoUrl)?;
    let (base, mut params) = match url.split_once('?') {
        Some((b, q)) => (
            b.to_string(),
            q.split('&')
                .filter(|p| !p.is_empty())
                .map(|p| match p.split_once('=') {
                    Some((k, v)) => KeyValue::new(k, v),
                    None => KeyValue::new(p, ""),
                })
                .collect::<Vec<_>>(),
        ),
        None => (url.clone(), Vec::new()),
    };

    let content_type = headers
        .iter()
        .find(|h| h.key.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_lowercase());

    let mut body = Body::None;
    if get {
        for d in data.iter().chain(urlencoded.iter()) {
            for pair in d.split('&') {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                params.push(KeyValue::new(k, v));
            }
        }
    } else if !form.is_empty() {
        body = Body::Multipart { fields: form };
    } else if !urlencoded.is_empty()
        || (!data.is_empty()
            && content_type
                .as_deref()
                .is_some_and(|c| c.contains("x-www-form-urlencoded")))
    {
        let mut fields = Vec::new();
        for d in data.iter().chain(urlencoded.iter()) {
            for pair in d.split('&') {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                fields.push(KeyValue::new(k, v));
            }
        }
        // The body type implies the header; avoid duplicating it.
        headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
        body = Body::FormUrlEncoded { fields };
    } else if !data.is_empty() {
        let content = data.join("&");
        let language = match content_type.as_deref() {
            Some(c) if c.contains("json") => RawLanguage::Json,
            Some(c) if c.contains("xml") => RawLanguage::Xml,
            Some(c) if c.contains("html") => RawLanguage::Html,
            None if serde_json::from_str::<serde_json::Value>(&content).is_ok() => {
                RawLanguage::Json
            }
            _ => RawLanguage::Text,
        };
        body = Body::Raw { content, language };
    }

    let has_body = !matches!(body, Body::None);
    let method = method.unwrap_or(if has_body {
        HttpMethod::Post
    } else {
        HttpMethod::Get
    });

    Ok(Request {
        name: format!("{} {}", method, base),
        method,
        url: base,
        params,
        headers,
        body,
        auth,
        ..Default::default()
    })
}

fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Supported snippet languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetLang {
    Curl,
    Python,
    JavaScript,
    Httpie,
}

impl SnippetLang {
    pub const ALL: [SnippetLang; 4] = [
        SnippetLang::Curl,
        SnippetLang::Python,
        SnippetLang::JavaScript,
        SnippetLang::Httpie,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SnippetLang::Curl => "cURL",
            SnippetLang::Python => "Python (requests)",
            SnippetLang::JavaScript => "JavaScript (fetch)",
            SnippetLang::Httpie => "HTTPie",
        }
    }
}

pub fn snippet(lang: SnippetLang, p: &PreparedRequest) -> String {
    match lang {
        SnippetLang::Curl => to_curl(p),
        SnippetLang::Python => to_python(p),
        SnippetLang::JavaScript => to_fetch(p),
        SnippetLang::Httpie => to_httpie(p),
    }
}

/// Renders a prepared request as a multi-line cURL command.
pub fn to_curl(p: &PreparedRequest) -> String {
    let mut parts = vec![format!("curl -X {} {}", p.method, sh_quote(&p.url))];
    let is_multipart = matches!(p.body, PreparedBody::Multipart(_));
    for (k, v) in &p.headers {
        if is_multipart && k.eq_ignore_ascii_case("content-type") {
            continue;
        }
        parts.push(format!("-H {}", sh_quote(&format!("{k}: {v}"))));
    }
    match &p.body {
        PreparedBody::None => {}
        PreparedBody::Text(t) => parts.push(format!("--data-raw {}", sh_quote(t))),
        PreparedBody::Bytes(b) => parts.push(format!("--data-binary {}", sh_quote(&String::from_utf8_lossy(b)))),
        PreparedBody::Form(fields) => {
            for (k, v) in fields {
                parts.push(format!("--data-urlencode {}", sh_quote(&format!("{k}={v}"))));
            }
        }
        PreparedBody::Multipart(fields) => {
            for (k, v, is_file) in fields {
                let val = if *is_file { format!("{k}=@{v}") } else { format!("{k}={v}") };
                parts.push(format!("-F {}", sh_quote(&val)));
            }
        }
    }
    parts.join(" \\\n  ")
}

fn py_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}"))
}

fn to_python(p: &PreparedRequest) -> String {
    let mut out = String::from("import requests\n\n");
    out.push_str(&format!("url = {}\n", py_str(&p.url)));
    let headers: Vec<String> = p
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(p.body, PreparedBody::Multipart(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("    {}: {},", py_str(k), py_str(v)))
        .collect();
    out.push_str(&format!("headers = {{\n{}\n}}\n", headers.join("\n")));
    let mut args = vec!["url".to_string(), "headers=headers".to_string()];
    match &p.body {
        PreparedBody::None => {}
        PreparedBody::Text(t) => {
            out.push_str(&format!("data = {}\n", py_str(t)));
            args.push("data=data".into());
        }
        PreparedBody::Bytes(b) => {
            out.push_str(&format!("data = {}.encode()\n", py_str(&String::from_utf8_lossy(b))));
            args.push("data=data".into());
        }
        PreparedBody::Form(fields) => {
            let f: Vec<String> = fields.iter().map(|(k, v)| format!("    {}: {},", py_str(k), py_str(v))).collect();
            out.push_str(&format!("data = {{\n{}\n}}\n", f.join("\n")));
            args.push("data=data".into());
        }
        PreparedBody::Multipart(fields) => {
            let data: Vec<String> = fields.iter().filter(|f| !f.2).map(|(k, v, _)| format!("    {}: {},", py_str(k), py_str(v))).collect();
            let files: Vec<String> = fields.iter().filter(|f| f.2).map(|(k, v, _)| format!("    {}: open({}, \"rb\"),", py_str(k), py_str(v))).collect();
            out.push_str(&format!("data = {{\n{}\n}}\nfiles = {{\n{}\n}}\n", data.join("\n"), files.join("\n")));
            args.push("data=data".into());
            args.push("files=files".into());
        }
    }
    out.push_str(&format!(
        "\nresponse = requests.request({}, {})\nprint(response.status_code)\nprint(response.text)\n",
        py_str(&p.method),
        args.join(", ")
    ));
    out
}

fn to_fetch(p: &PreparedRequest) -> String {
    let js = py_str; // JSON string literals are valid JS literals.
    let mut out = String::new();
    let mut opts = vec![format!("  method: {}", js(&p.method))];
    let headers: Vec<String> = p
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(p.body, PreparedBody::Multipart(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("    {}: {},", js(k), js(v)))
        .collect();
    if !headers.is_empty() {
        opts.push(format!("  headers: {{\n{}\n  }}", headers.join("\n")));
    }
    match &p.body {
        PreparedBody::None => {}
        PreparedBody::Text(t) => opts.push(format!("  body: {}", js(t))),
        PreparedBody::Bytes(b) => opts.push(format!("  body: {}", js(&String::from_utf8_lossy(b)))),
        PreparedBody::Form(fields) => {
            let pairs: Vec<String> = fields.iter().map(|(k, v)| format!("[{}, {}]", js(k), js(v))).collect();
            opts.push(format!("  body: new URLSearchParams([{}])", pairs.join(", ")));
        }
        PreparedBody::Multipart(fields) => {
            out.push_str("const form = new FormData();\n");
            for (k, v, is_file) in fields {
                if *is_file {
                    out.push_str(&format!("// form.append({}, fileInput.files[0]); // {}\n", js(k), v));
                } else {
                    out.push_str(&format!("form.append({}, {});\n", js(k), js(v)));
                }
            }
            out.push('\n');
            opts.push("  body: form".into());
        }
    }
    out.push_str(&format!(
        "const response = await fetch({}, {{\n{},\n}});\nconsole.log(response.status, await response.text());\n",
        js(&p.url),
        opts.join(",\n")
    ));
    out
}

fn to_httpie(p: &PreparedRequest) -> String {
    let mut parts = vec![format!("http {} {}", p.method, sh_quote(&p.url))];
    for (k, v) in &p.headers {
        parts.push(sh_quote(&format!("{k}:{v}")));
    }
    let body_suffix = match &p.body {
        PreparedBody::Text(t) => format!(" <<< {}", sh_quote(t)),
        _ => String::new(),
    };
    match &p.body {
        PreparedBody::Form(fields) => {
            parts.insert(1, "--form".into());
            for (k, v) in fields {
                parts.push(sh_quote(&format!("{k}={v}")));
            }
        }
        PreparedBody::Multipart(fields) => {
            parts.insert(1, "--multipart".into());
            for (k, v, is_file) in fields {
                let sep = if *is_file { "@" } else { "=" };
                parts.push(sh_quote(&format!("{k}{sep}{v}")));
            }
        }
        _ => {}
    }
    format!("{}{}", parts.join(" \\\n  "), body_suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::prepare;
    use crate::vars::VarScope;

    #[test]
    fn split_handles_quotes_and_continuations() {
        let w = shell_split("curl 'a b' \"c \\\"d\\\"\" \\\n  e").unwrap();
        assert_eq!(w, vec!["curl", "a b", "c \"d\"", "e"]);
        assert_eq!(shell_split("curl 'oops"), Err(CurlError::UnterminatedQuote));
    }

    #[test]
    fn parses_browser_copy_as_curl() {
        let cmd = r#"curl 'https://api.test/v1/users?page=2' \
  -H 'accept: application/json' \
  -H 'content-type: application/json' \
  --data-raw '{"name":"Budi"}' \
  --compressed"#;
        let r = parse_curl(cmd).unwrap();
        assert_eq!(r.method, HttpMethod::Post);
        assert_eq!(r.url, "https://api.test/v1/users");
        assert_eq!(r.params, vec![KeyValue::new("page", "2")]);
        assert_eq!(r.headers.len(), 2);
        assert_eq!(
            r.body,
            Body::Raw {
                content: "{\"name\":\"Budi\"}".into(),
                language: RawLanguage::Json
            }
        );
    }

    #[test]
    fn parses_form_auth_and_get() {
        let r = parse_curl("curl -u admin:pw -F name=x -F file=@/tmp/a.png http://h/up").unwrap();
        assert_eq!(r.method, HttpMethod::Post);
        assert!(matches!(r.auth, Auth::Basic { .. }));
        match r.body {
            Body::Multipart { fields } => {
                assert!(fields[1].is_file);
                assert_eq!(fields[1].value, "/tmp/a.png");
            }
            other => panic!("unexpected {other:?}"),
        }
        let r = parse_curl("curl -G http://h/s -d q=rust --data-urlencode lang=id").unwrap();
        assert_eq!(r.method, HttpMethod::Get);
        assert_eq!(r.params.len(), 2);
        let r = parse_curl("curl --request=DELETE --url=http://h/x/1").unwrap();
        assert_eq!(r.method, HttpMethod::Delete);
        assert_eq!(r.url, "http://h/x/1");
    }

    #[test]
    fn rejects_non_curl() {
        assert_eq!(parse_curl("wget http://x"), Err(CurlError::NotCurl));
        assert_eq!(parse_curl("curl -s"), Err(CurlError::NoUrl));
    }

    #[test]
    fn curl_roundtrip() {
        let mut req = Request::new(HttpMethod::Put, "https://api.test/items/1");
        req.headers.push(KeyValue::new("X-Trace", "it's me"));
        req.body = Body::Raw {
            content: "{\"a\":1}".into(),
            language: RawLanguage::Json,
        };
        let prepared = prepare(&req, &VarScope::new(), None).unwrap();
        let cmd = to_curl(&prepared);
        let back = parse_curl(&cmd).unwrap();
        assert_eq!(back.method, HttpMethod::Put);
        assert_eq!(back.url, "https://api.test/items/1");
        assert!(back.headers.iter().any(|h| h.value == "it's me"));
        assert_eq!(back.body, req.body);
        for lang in SnippetLang::ALL {
            assert!(snippet(lang, &prepared).contains("api.test"));
        }
    }
}
