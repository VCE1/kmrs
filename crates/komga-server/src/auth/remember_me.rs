//! Remember-me: aligned with Spring `TokenBasedRememberMeServices`, so a cookie issued by
//! either server keeps working after switching between them.
//! token = base64NoPad(urlenc(username) ":" expiryMillis ":" algorithm ":" hex(digest(...)))
//! where digest covers `username ":" expiryMillis ":" password ":" key`. Tokens from
//! before Spring Security 6 have no algorithm part and use MD5; new tokens use SHA-256,
//! Komga's current default.
//! Cookie name `komga-remember-me`, path=/; validity is determined by SERVER_SETTINGS.REMEMBER_ME_DURATION (days).

use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine;
use komga_core::model::user::KomgaUser;
use sha2::{Digest, Sha256};

pub const REMEMBER_ME_COOKIE: &str = "komga-remember-me";
pub const REMEMBER_ME_PARAM: &str = "remember-me";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Md5,
    Sha256,
}

impl Algorithm {
    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha256 => "SHA256",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "MD5" => Some(Self::Md5),
            "SHA256" => Some(Self::Sha256),
            _ => None,
        }
    }

    fn signature(self, email: &str, expiry_millis: u64, password: &str, key: &str) -> String {
        let data = format!("{email}:{expiry_millis}:{password}:{key}");
        match self {
            Self::Md5 => format!("{:x}", md5::compute(data.as_bytes())),
            Self::Sha256 => hex::encode(Sha256::digest(data.as_bytes())),
        }
    }
}

/// A decoded cookie, before it is checked against a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberMeToken {
    pub email: String,
    expiry_millis: u64,
    algorithm: Algorithm,
    signature: String,
}

pub fn encode_token(user: &KomgaUser, key: &str, expiry_millis: u64) -> String {
    let algorithm = Algorithm::Sha256;
    let signature = algorithm.signature(&user.email, expiry_millis, &user.password, key);
    let parts = [
        user.email.clone(),
        expiry_millis.to_string(),
        algorithm.name().to_string(),
        signature,
    ];
    // Spring URL-encodes each part, joins them with ':' and drops the base64 padding
    let joined = parts
        .iter()
        .map(|part| form_urlencoded::byte_serialize(part.as_bytes()).collect::<String>())
        .collect::<Vec<_>>()
        .join(":");
    B64.encode(joined)
}

pub fn parse_token(token: &str) -> Option<RememberMeToken> {
    let decoded = String::from_utf8(B64.decode(token.trim_end_matches('=')).ok()?).ok()?;
    let parts: Vec<String> = decoded.split(':').map(percent_decode).collect();
    let (email, expiry, algorithm, signature) = match parts.as_slice() {
        [email, expiry, signature] => (email, expiry, Algorithm::Md5, signature),
        [email, expiry, algorithm, signature] => {
            (email, expiry, Algorithm::parse(algorithm)?, signature)
        }
        _ => return None,
    };
    Some(RememberMeToken {
        email: email.clone(),
        expiry_millis: expiry.parse().ok()?,
        algorithm,
        signature: signature.clone(),
    })
}

/// Undoes Spring's per-part URL encoding. `+` stays as is: an email never holds a space,
/// and cookies kmrs issued before the parts were encoded carry a raw `+` in the email.
fn percent_decode(part: &str) -> String {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| part.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `token` is a live cookie for `user`: not expired, and signed with this
/// server's key over the user's current password hash.
pub fn verify(token: &RememberMeToken, user: &KomgaUser, key: &str, now_millis: u64) -> bool {
    if token.expiry_millis < now_millis {
        return false;
    }
    let expected = token
        .algorithm
        .signature(&user.email, token.expiry_millis, &user.password, key);
    token.signature == expected
}

#[cfg(test)]
mod tests {
    use super::*;
    use komga_core::model::user::*;

    fn user() -> KomgaUser {
        KomgaUser {
            id: "u1".into(),
            email: "a+b@c.d".into(),
            password: "$2a$10$hash".into(),
            roles: Default::default(),
            shared_all_libraries: true,
            shared_libraries_ids: Default::default(),
            restrictions: ContentRestrictions::default(),
            created_date: time::OffsetDateTime::now_utc(),
            last_modified_date: time::OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn roundtrip_issues_sha256() {
        let token = encode_token(&user(), "secret", 1_900_000_000_000);
        let parsed = parse_token(&token).unwrap();
        assert_eq!(parsed.email, "a+b@c.d");
        assert_eq!(parsed.algorithm, Algorithm::Sha256);
        assert!(verify(&parsed, &user(), "secret", 1_000_000));
    }

    #[test]
    fn rejects_wrong_key_expired_and_changed_password() {
        let parsed = parse_token(&encode_token(&user(), "secret", 1_900_000_000_000)).unwrap();
        assert!(!verify(&parsed, &user(), "other", 1_000_000));
        assert!(!verify(&parsed, &user(), "secret", 2_000_000_000_000));
        let changed = KomgaUser {
            password: "$2a$10$other".into(),
            ..user()
        };
        assert!(!verify(&parsed, &changed, "secret", 1_000_000));
    }

    #[test]
    fn spring_cookie_formats_verify() {
        let u = user();
        let expiry = 1_900_000_000_000u64;
        // Komga today: four URL-encoded parts with the algorithm, base64 without padding
        let sha = Algorithm::Sha256.signature(&u.email, expiry, &u.password, "secret");
        let komga = B64.encode(format!("a%2Bb%40c.d:{expiry}:SHA256:{sha}"));
        assert!(verify(
            &parse_token(&komga).unwrap(),
            &u,
            "secret",
            1_000_000
        ));
        // before Spring Security 6, and kmrs before this change: three parts, MD5, padded
        let md5 = Algorithm::Md5.signature(&u.email, expiry, &u.password, "secret");
        let legacy =
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{expiry}:{md5}", u.email));
        let parsed = parse_token(&legacy).unwrap();
        assert_eq!(parsed.algorithm, Algorithm::Md5);
        assert!(verify(&parsed, &u, "secret", 1_000_000));
    }

    #[test]
    fn rejects_malformed_and_unknown_algorithm() {
        assert!(parse_token("not base64!").is_none());
        assert!(parse_token(&B64.encode("a:1")).is_none());
        assert!(parse_token(&B64.encode("a:1:SHA512:sig")).is_none());
    }
}
