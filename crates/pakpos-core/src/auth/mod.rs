//! Auth schemes that need more than a static header: HTTP Digest and NTLM
//! (challenge / response) and AWS Signature Version 4 (request signing).

pub mod digest;
pub mod ntlm;
pub mod sigv4;

use rand::Rng;

/// Credentials for schemes answered after the server's `401` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChallengeAuth {
    Digest { username: String, password: String },
    Ntlm { username: String, password: String, domain: String, workstation: String },
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::rng().fill(&mut out[..]);
    out
}
