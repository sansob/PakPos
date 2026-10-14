//! AWS Signature Version 4 request signing.

use super::hex;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Resolved signing settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
    pub region: String,
    pub service: String,
}

/// What to sign.
pub struct SignInput<'a> {
    pub method: &'a str,
    pub url: &'a url::Url,
    pub headers: &'a [(String, String)],
    /// `None` signs the payload as `UNSIGNED-PAYLOAD` (multipart bodies).
    pub body: Option<&'a [u8]>,
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 encoding with AWS's unreserved set.
fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn canonical_uri(url: &url::Url, service: &str) -> String {
    let path = url.path();
    let path = if path.is_empty() { "/" } else { path };
    // S3 signs the path as sent; every other service encodes it once more.
    if service.eq_ignore_ascii_case("s3") {
        path.to_string()
    } else {
        encode(path, true)
    }
}

fn canonical_query(url: &url::Url) -> String {
    let mut pairs: Vec<(String, String)> =
        url.query_pairs().map(|(k, v)| (encode(&k, false), encode(&v, false))).collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

fn host_header(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("");
    match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    }
}

/// Headers to add to the request (`X-Amz-Date`, optional
/// `X-Amz-Security-Token` / `X-Amz-Content-Sha256`, and `Authorization`).
pub fn sign(input: &SignInput, creds: &Credentials, amz_date: &str) -> Vec<(String, String)> {
    let date = &amz_date[..8.min(amz_date.len())];
    let payload_hash = match input.body {
        Some(b) => hex(&Sha256::digest(b)),
        None => "UNSIGNED-PAYLOAD".to_string(),
    };

    let mut added = vec![("X-Amz-Date".to_string(), amz_date.to_string())];
    if !creds.session_token.is_empty() {
        added.push(("X-Amz-Security-Token".into(), creds.session_token.clone()));
    }
    if creds.service.eq_ignore_ascii_case("s3") {
        added.push(("X-Amz-Content-Sha256".into(), payload_hash.clone()));
    }

    // Sign host, content-type and every x-amz-* header.
    let mut signed: Vec<(String, String)> = vec![("host".into(), host_header(input.url))];
    for (k, v) in input.headers.iter().chain(added.iter()) {
        let name = k.trim().to_ascii_lowercase();
        if name == "content-type" || name.starts_with("x-amz-") {
            let value = v.split_whitespace().collect::<Vec<_>>().join(" ");
            match signed.iter_mut().find(|(n, _)| *n == name) {
                Some(existing) => {
                    existing.1.push(',');
                    existing.1.push_str(&value);
                }
                None => signed.push((name, value)),
            }
        }
    }
    signed.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = signed.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = signed.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        input.method.to_ascii_uppercase(),
        canonical_uri(input.url, &creds.service),
        canonical_query(input.url),
    );
    let scope = format!("{date}/{}/{}/aws4_request", creds.region, creds.service);
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex(&Sha256::digest(canonical_request.as_bytes())));

    let k_date = hmac(format!("AWS4{}", creds.secret_key).as_bytes(), date);
    let k_region = hmac(&k_date, &creds.region);
    let k_service = hmac(&k_region, &creds.service);
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex(&hmac(&k_signing, &string_to_sign));

    added.push((
        "Authorization".into(),
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            creds.access_key
        ),
    ));
    added
}

/// `20150830T123600Z` for the current time.
pub fn now_amz_date() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> Credentials {
        Credentials {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: String::new(),
            region: "us-east-1".into(),
            service: "service".into(),
        }
    }

    fn auth_of(headers: &[(String, String)]) -> &str {
        &headers.iter().find(|(k, _)| k == "Authorization").unwrap().1
    }

    #[test]
    fn aws_test_suite_get_vanilla() {
        let url = url::Url::parse("https://example.amazonaws.com/").unwrap();
        let input = SignInput { method: "GET", url: &url, headers: &[], body: Some(b"") };
        let h = sign(&input, &creds(), "20150830T123600Z");
        assert_eq!(
            auth_of(&h),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn query_is_sorted_and_encoded() {
        let url = url::Url::parse("https://example.amazonaws.com/?Param2=value2&Param1=value%201").unwrap();
        assert_eq!(canonical_query(&url), "Param1=value%201&Param2=value2");
        let mut c = creds();
        c.session_token = "tok".into();
        c.service = "s3".into();
        let input = SignInput { method: "PUT", url: &url, headers: &[], body: None };
        let h = sign(&input, &c, "20150830T123600Z");
        assert!(h.iter().any(|(k, v)| k == "X-Amz-Content-Sha256" && v == "UNSIGNED-PAYLOAD"));
        assert!(auth_of(&h).contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token"));
    }
}
