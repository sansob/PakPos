//! HTTP Digest authentication (RFC 7616, compatible with RFC 2617 servers).

use super::hex;
use md5::Md5;
use sha2::{Digest as _, Sha256, Sha512_256};

/// A parsed `WWW-Authenticate: Digest …` challenge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub algorithm: String,
    pub qop: Vec<String>,
}

/// Finds the Digest challenge among `WWW-Authenticate` header values.
pub fn find_challenge<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<Challenge> {
    values.into_iter().find_map(parse_challenge)
}

pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let rest = header.trim();
    let (scheme, params) = rest.split_once(char::is_whitespace)?;
    if !scheme.eq_ignore_ascii_case("digest") {
        return None;
    }
    let mut c = Challenge { algorithm: "MD5".into(), ..Default::default() };
    for (k, v) in parse_params(params) {
        match k.to_ascii_lowercase().as_str() {
            "realm" => c.realm = v,
            "nonce" => c.nonce = v,
            "opaque" => c.opaque = Some(v),
            "algorithm" => c.algorithm = v,
            "qop" => c.qop = v.split(',').map(|q| q.trim().to_ascii_lowercase()).filter(|q| !q.is_empty()).collect(),
            _ => {}
        }
    }
    (!c.nonce.is_empty()).then_some(c)
}

/// `key=value, key="quoted, value"` → pairs.
fn parse_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if *c == ',' || c.is_whitespace()) {
            chars.next();
        }
        let key: String = std::iter::from_fn(|| chars.next_if(|c| *c != '=' && *c != ',')).collect();
        if key.trim().is_empty() {
            break;
        }
        let mut value = String::new();
        if chars.next_if_eq(&'=').is_some() {
            if chars.next_if_eq(&'"').is_some() {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => value.extend(chars.next()),
                        '"' => break,
                        c => value.push(c),
                    }
                }
            } else {
                value = std::iter::from_fn(|| chars.next_if(|c| *c != ',')).collect::<String>().trim().to_string();
            }
        }
        out.push((key.trim().to_string(), value));
    }
    out
}

fn hash(algorithm: &str, data: &str) -> String {
    let base = algorithm.trim_end_matches("-sess").trim_end_matches("-SESS").to_ascii_uppercase();
    match base.as_str() {
        "SHA-256" => hex(&Sha256::digest(data.as_bytes())),
        "SHA-512-256" => hex(&Sha512_256::digest(data.as_bytes())),
        _ => hex(&Md5::digest(data.as_bytes())),
    }
}

/// Builds the `Authorization` header answering `challenge`.
///
/// `uri` is the request target (path and query); `body` is only used for
/// `qop=auth-int`, which is chosen when the server offers nothing else.
#[allow(clippy::too_many_arguments)] // mirrors the RFC's inputs one to one
pub fn authorization(
    challenge: &Challenge,
    username: &str,
    password: &str,
    method: &str,
    uri: &str,
    body: &[u8],
    cnonce: &str,
    nc: u32,
) -> String {
    let alg = &challenge.algorithm;
    let sess = alg.to_ascii_lowercase().ends_with("-sess");
    let mut ha1 = hash(alg, &format!("{username}:{}:{password}", challenge.realm));
    if sess {
        ha1 = hash(alg, &format!("{ha1}:{}:{cnonce}", challenge.nonce));
    }
    let qop = if challenge.qop.iter().any(|q| q == "auth") {
        Some("auth")
    } else if challenge.qop.iter().any(|q| q == "auth-int") {
        Some("auth-int")
    } else {
        None
    };
    let ha2 = match qop {
        Some("auth-int") => {
            let body_hash = hash(alg, &String::from_utf8_lossy(body));
            hash(alg, &format!("{method}:{uri}:{body_hash}"))
        }
        _ => hash(alg, &format!("{method}:{uri}")),
    };
    let nc = format!("{nc:08x}");
    let response = match qop {
        Some(q) => hash(alg, &format!("{ha1}:{}:{nc}:{cnonce}:{q}:{ha2}", challenge.nonce)),
        None => hash(alg, &format!("{ha1}:{}:{ha2}", challenge.nonce)),
    };
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut header = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={alg}, response=\"{response}\"",
        quote(username),
        quote(&challenge.realm),
        quote(&challenge.nonce),
        quote(uri),
    );
    if let Some(q) = qop {
        header.push_str(&format!(", qop={q}, nc={nc}, cnonce=\"{cnonce}\""));
    }
    if let Some(opaque) = &challenge.opaque {
        header.push_str(&format!(", opaque=\"{}\"", quote(opaque)));
    }
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc2617_example() {
        // The worked example from RFC 2617 section 3.5.
        let c = parse_challenge(
            r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
        )
        .unwrap();
        assert_eq!(c.qop, vec!["auth", "auth-int"]);
        let h = authorization(&c, "Mufasa", "Circle Of Life", "GET", "/dir/index.html", b"", "0a4f113b", 1);
        assert!(h.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{h}");
        assert!(h.contains("nc=00000001"));
        assert!(h.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));
    }

    #[test]
    fn picks_digest_among_several_challenges() {
        let c = find_challenge(["Basic realm=\"x\"", "Digest realm=\"r\", nonce=\"n\", algorithm=SHA-256"]).unwrap();
        assert_eq!(c.algorithm, "SHA-256");
        assert!(c.qop.is_empty());
        let h = authorization(&c, "u", "p", "GET", "/", b"", "c", 1);
        assert!(!h.contains("qop="));
        assert!(parse_challenge("Bearer realm=\"x\"").is_none());
    }
}
