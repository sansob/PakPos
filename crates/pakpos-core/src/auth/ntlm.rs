//! NTLMv2 over HTTP (MS-NLMP), without signing or sealing.
//!
//! The handshake is: negotiate (type 1) → server challenge (type 2) →
//! authenticate (type 3), all on one keep-alive connection.

use hmac::{Hmac, KeyInit, Mac};
use md4::{Digest, Md4};
use md5::Md5;

type HmacMd5 = Hmac<Md5>;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";
const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NEGOTIATE_NTLM: u32 = 0x0000_0200;
const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
const NEGOTIATE_128: u32 = 0x2000_0000;
const NEGOTIATE_56: u32 = 0x8000_0000;

const FLAGS: u32 = NEGOTIATE_UNICODE
    | REQUEST_TARGET
    | NEGOTIATE_NTLM
    | NEGOTIATE_ALWAYS_SIGN
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_128
    | NEGOTIATE_56;

/// Seconds between 1601-01-01 and 1970-01-01.
const FILETIME_EPOCH_OFFSET: u64 = 11_644_473_600;

/// The server's type 2 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    pub target_name: Vec<u8>,
    pub target_info: Vec<u8>,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut mac = HmacMd5::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// `NTOWFv2(password, user, domain)`.
pub fn ntowf_v2(password: &str, username: &str, domain: &str) -> [u8; 16] {
    let nt_hash = Md4::digest(utf16le(password));
    hmac_md5(&nt_hash, &[&utf16le(&format!("{}{domain}", username.to_uppercase()))])
}

/// Splits `DOMAIN\user` or `user@domain` when no domain was given.
pub fn split_user(username: &str, domain: &str) -> (String, String) {
    if !domain.is_empty() {
        return (username.to_string(), domain.to_string());
    }
    if let Some((d, u)) = username.split_once('\\') {
        return (u.to_string(), d.to_string());
    }
    (username.to_string(), String::new())
}

/// `Authorization` value for the first leg.
pub fn negotiate_header() -> String {
    use base64::Engine;
    let mut msg = Vec::with_capacity(32);
    msg.extend_from_slice(SIGNATURE);
    msg.extend_from_slice(&1u32.to_le_bytes());
    msg.extend_from_slice(&FLAGS.to_le_bytes());
    msg.extend_from_slice(&[0u8; 16]); // empty domain + workstation
    format!("NTLM {}", b64().encode(msg))
}

/// Finds and decodes `WWW-Authenticate: NTLM <base64>`.
pub fn find_challenge<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<Challenge> {
    use base64::Engine;
    values.into_iter().find_map(|v| {
        let (scheme, data) = v.trim().split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("ntlm") {
            return None;
        }
        parse_challenge(&b64().decode(data.trim()).ok()?)
    })
}

fn sec_buf(msg: &[u8], at: usize) -> Option<Vec<u8>> {
    let len = u16::from_le_bytes(msg.get(at..at + 2)?.try_into().ok()?) as usize;
    let off = u32::from_le_bytes(msg.get(at + 4..at + 8)?.try_into().ok()?) as usize;
    if len == 0 {
        return Some(Vec::new());
    }
    msg.get(off..off + len).map(<[u8]>::to_vec)
}

pub fn parse_challenge(msg: &[u8]) -> Option<Challenge> {
    if msg.get(..8)? != SIGNATURE || u32::from_le_bytes(msg.get(8..12)?.try_into().ok()?) != 2 {
        return None;
    }
    let flags = u32::from_le_bytes(msg.get(20..24)?.try_into().ok()?);
    let server_challenge: [u8; 8] = msg.get(24..32)?.try_into().ok()?;
    let target_name = sec_buf(msg, 12)?;
    let target_info = if flags & NEGOTIATE_TARGET_INFO != 0 { sec_buf(msg, 40)? } else { Vec::new() };
    Some(Challenge { flags, server_challenge, target_name, target_info })
}

/// The `MsvAvTimestamp` pair in the target info, if the server sent one.
fn av_timestamp(target_info: &[u8]) -> Option<[u8; 8]> {
    let mut i = 0;
    while i + 4 <= target_info.len() {
        let id = u16::from_le_bytes([target_info[i], target_info[i + 1]]);
        let len = u16::from_le_bytes([target_info[i + 2], target_info[i + 3]]) as usize;
        let value = target_info.get(i + 4..i + 4 + len)?;
        match id {
            0 => return None,
            7 if len == 8 => return value.try_into().ok(),
            _ => {}
        }
        i += 4 + len;
    }
    None
}

fn filetime_now() -> [u8; 8] {
    let since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let ticks =
        (since_unix.as_secs() + FILETIME_EPOCH_OFFSET) * 10_000_000 + u64::from(since_unix.subsec_nanos() / 100);
    ticks.to_le_bytes()
}

/// NTLMv2 responses: (LMv2, NTv2).
pub fn responses(
    challenge: &Challenge,
    nt_owf: &[u8; 16],
    client_challenge: [u8; 8],
    timestamp: [u8; 8],
) -> (Vec<u8>, Vec<u8>) {
    let mut temp = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(&timestamp);
    temp.extend_from_slice(&client_challenge);
    temp.extend_from_slice(&[0u8; 4]);
    temp.extend_from_slice(&challenge.target_info);
    temp.extend_from_slice(&[0u8; 4]);
    let nt_proof = hmac_md5(nt_owf, &[&challenge.server_challenge, &temp]);
    let mut nt = nt_proof.to_vec();
    nt.extend_from_slice(&temp);

    // With a server timestamp the LM response must be zeros (MS-NLMP 3.1.5.1.2).
    let lm = if av_timestamp(&challenge.target_info).is_some() {
        vec![0u8; 24]
    } else {
        let mut lm = hmac_md5(nt_owf, &[&challenge.server_challenge, &client_challenge]).to_vec();
        lm.extend_from_slice(&client_challenge);
        lm
    };
    (lm, nt)
}

/// `Authorization` value for the third leg.
pub fn authenticate_header(
    challenge: &Challenge,
    username: &str,
    password: &str,
    domain: &str,
    workstation: &str,
) -> String {
    use base64::Engine;
    let (user, domain) = split_user(username, domain);
    let nt_owf = ntowf_v2(password, &user, &domain);
    let timestamp = av_timestamp(&challenge.target_info).unwrap_or_else(filetime_now);
    let (lm, nt) = responses(challenge, &nt_owf, super::random_bytes::<8>(), timestamp);

    let fields: [Vec<u8>; 6] = [lm, nt, utf16le(&domain), utf16le(&user), utf16le(workstation), Vec::new()];
    let mut msg = Vec::new();
    msg.extend_from_slice(SIGNATURE);
    msg.extend_from_slice(&3u32.to_le_bytes());
    let header_len = 64u32;
    let mut offset = header_len;
    let mut payload = Vec::new();
    for f in &fields {
        let len = f.len() as u16;
        msg.extend_from_slice(&len.to_le_bytes());
        msg.extend_from_slice(&len.to_le_bytes());
        msg.extend_from_slice(&offset.to_le_bytes());
        offset += u32::from(len);
        payload.extend_from_slice(f);
    }
    let flags = FLAGS & (challenge.flags | NEGOTIATE_UNICODE);
    msg.extend_from_slice(&flags.to_le_bytes());
    msg.extend_from_slice(&payload);
    format!("NTLM {}", b64().encode(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors from MS-NLMP section 4.2.1 / 4.2.4.
    const SERVER_CHALLENGE: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    const CLIENT_CHALLENGE: [u8; 8] = [0xaa; 8];

    #[test]
    fn ntowf_v2_matches_spec() {
        assert_eq!(crate::auth::hex(&ntowf_v2("Password", "User", "Domain")), "0c868a403bfd7a93a3001ef22ef02e3f");
    }

    #[test]
    fn ntlmv2_response_matches_spec() {
        // AV pairs: NetBIOS domain "Domain", NetBIOS server "Server", EOL.
        let mut target_info = Vec::new();
        for (id, s) in [(2u16, "Domain"), (1u16, "Server")] {
            let v = utf16le(s);
            target_info.extend_from_slice(&id.to_le_bytes());
            target_info.extend_from_slice(&(v.len() as u16).to_le_bytes());
            target_info.extend_from_slice(&v);
        }
        target_info.extend_from_slice(&[0, 0, 0, 0]);
        let c = Challenge {
            flags: NEGOTIATE_TARGET_INFO,
            server_challenge: SERVER_CHALLENGE,
            target_name: utf16le("Server"),
            target_info,
        };
        let (lm, nt) = responses(&c, &ntowf_v2("Password", "User", "Domain"), CLIENT_CHALLENGE, [0u8; 8]);
        assert_eq!(crate::auth::hex(&lm[..16]), "86c35097ac9cec102554764a57cccc19");
        assert_eq!(crate::auth::hex(&nt[..16]), "68cd0ab851e51c96aabc927bebef6a1c");
    }

    #[test]
    fn messages_round_trip() {
        use base64::Engine;
        let neg = negotiate_header();
        let raw = b64().decode(neg.trim_start_matches("NTLM ")).unwrap();
        assert_eq!(&raw[..8], SIGNATURE);
        assert_eq!(raw.len(), 32);

        let mut t2 = Vec::new();
        t2.extend_from_slice(SIGNATURE);
        t2.extend_from_slice(&2u32.to_le_bytes());
        t2.extend_from_slice(&[0u8; 8]);
        t2.extend_from_slice(&FLAGS.to_le_bytes());
        t2.extend_from_slice(&SERVER_CHALLENGE);
        t2.extend_from_slice(&[0u8; 16]);
        let header = format!("NTLM {}", b64().encode(&t2));
        let c = find_challenge([header.as_str()]).unwrap();
        assert_eq!(c.server_challenge, SERVER_CHALLENGE);

        let auth = authenticate_header(&c, "DOMAIN\\user", "pw", "", "WS");
        let raw = b64().decode(auth.trim_start_matches("NTLM ")).unwrap();
        assert_eq!(u32::from_le_bytes(raw[8..12].try_into().unwrap()), 3);
        assert_eq!(sec_buf(&raw, 28).unwrap(), utf16le("DOMAIN"));
        assert_eq!(sec_buf(&raw, 36).unwrap(), utf16le("user"));
    }
}
