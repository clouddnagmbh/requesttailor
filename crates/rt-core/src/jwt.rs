// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading a JSON Web Token (wave 18, strand B): header and claims as JSON, **no signature
//! check** (the UI says "signature not verified"). The token value is shown but never written
//! to a file, a history, a report or a note; only claim names and deadlines appear there.
//!
//! Interface: the types and `parse`, which says "not implemented yet" until strand B.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A decoded token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Jwt {
    /// The JOSE header, e.g. `{"alg": "RS256", "typ": "JWT"}`.
    pub header: serde_json::Value,
    /// The claims; an object for every well-formed token.
    pub claims: serde_json::Value,
    /// Length of the signature part in bytes after decoding; the signature itself is dropped.
    pub signature_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum JwtError {
    /// Not three dot-separated Base64url parts.
    #[error("not a JSON Web Token")]
    Malformed,
    /// A part is not Base64url or not JSON.
    #[error("the token cannot be decoded: {0}")]
    Decode(String),
    /// Larger than [`MAX_TOKEN_LEN`].
    #[error("the token is larger than {MAX_TOKEN_LEN} bytes")]
    TooLarge,
    #[error("{0}")]
    Unsupported(String),
}

/// Tokens longer than this (bytes) are refused.
pub const MAX_TOKEN_LEN: usize = 64 * 1024;

/// Reads `token` (a leading `Bearer ` is tolerated). Three dot-separated Base64url parts: the
/// header and the claims must be JSON objects; the signature is only measured, never checked.
pub fn parse(token: &str) -> Result<Jwt, JwtError> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let token = token.trim();
    let token = token
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("bearer "))
        .map_or(token, |_| token[7..].trim_start());
    if token.len() > MAX_TOKEN_LEN {
        return Err(JwtError::TooLarge);
    }
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(JwtError::Malformed);
    };
    if head.is_empty() || body.is_empty() {
        return Err(JwtError::Malformed);
    }
    let decode = |part: &str, what: &str| -> Result<Vec<u8>, JwtError> {
        URL_SAFE_NO_PAD
            .decode(part.trim_end_matches('='))
            .map_err(|_| JwtError::Decode(format!("the {what} is not Base64url")))
    };
    let object = |part: &str, what: &str| -> Result<serde_json::Value, JwtError> {
        let bytes = decode(part, what)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| JwtError::Decode(format!("the {what} is not JSON")))?;
        if value.is_object() {
            Ok(value)
        } else {
            Err(JwtError::Decode(format!("the {what} is not a JSON object")))
        }
    };
    let header = object(head, "header")?;
    let claims = object(body, "claims")?;
    let signature_len = decode(sig, "signature")?.len();
    Ok(Jwt {
        header,
        claims,
        signature_len,
    })
}

impl Jwt {
    /// `alg` of the header, if it is text.
    pub fn algorithm(&self) -> Option<&str> {
        self.header.get("alg").and_then(|v| v.as_str())
    }

    /// A claim holding a Unix time in seconds (`exp`, `nbf`, `iat`), as whole seconds.
    pub fn time_claim(&self, name: &str) -> Option<i64> {
        let v = self.claims.get(name)?;
        v.as_i64().or_else(|| {
            let f = v.as_f64()?;
            (f.is_finite() && f.abs() < 1e15).then_some(f.floor() as i64)
        })
    }

    /// `alg: none` in any spelling.
    pub fn is_unsigned(&self) -> bool {
        self.algorithm()
            .is_some_and(|a| a.eq_ignore_ascii_case("none"))
    }
}

/// Whether `token` is an HS256 token signed with exactly `candidate` as the shared secret
/// (wave 19, strand R: the passive check for guessable secrets tries a short list of common
/// ones). Pure computation on the token already in hand; says nothing about the secret but
/// yes or no, and a token of another algorithm is always `false`.
pub fn hs256_secret_matches(token: &str, candidate: &str) -> bool {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use hmac::{Hmac, Mac};

    let token = token.trim();
    let Some((signing_input, signature)) = token.rsplit_once('.') else {
        return false;
    };
    let is_hs256 = parse(token)
        .ok()
        .is_some_and(|t| t.algorithm() == Some("HS256"));
    if !is_hs256 {
        return false;
    }
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<sha2::Sha256>::new_from_slice(candidate.as_bytes()) else {
        return false;
    };
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&signature).is_ok()
}

/// Tokens that expire in fewer seconds than this count as "expires soon".
pub const EXPIRES_SOON_SECS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    Expired,
    NotYetValid,
    ExpiresSoon,
    AlgorithmNone,
}

/// A problem with a token at one point in time. `text` names the claim and the deadline,
/// never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    pub text: String,
}

/// `12 s`, `12 min`, `3 h`, `2 d`: a span for people, rounded down.
pub fn span(seconds: i64) -> String {
    let s = seconds.unsigned_abs();
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        3600..=86_399 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86_400),
    }
}

impl Jwt {
    /// What is wrong with the token at `now` (Unix seconds): expired, not yet valid (`nbf`),
    /// expiring in under [`EXPIRES_SOON_SECS`], `alg: none`.
    pub fn issues(&self, now: i64) -> Vec<Issue> {
        let mut out = Vec::new();
        if let Some(exp) = self.time_claim("exp") {
            if exp <= now {
                out.push(Issue {
                    kind: IssueKind::Expired,
                    text: format!("token expired {} ago (exp)", span(now - exp)),
                });
            } else if exp - now < EXPIRES_SOON_SECS {
                out.push(Issue {
                    kind: IssueKind::ExpiresSoon,
                    text: format!("token expires in {} (exp)", span(exp - now)),
                });
            }
        }
        if let Some(nbf) = self.time_claim("nbf")
            && nbf > now
        {
            out.push(Issue {
                kind: IssueKind::NotYetValid,
                text: format!("token is not valid yet, nbf is in {}", span(nbf - now)),
            });
        }
        if self.is_unsigned() {
            out.push(Issue {
                kind: IssueKind::AlgorithmNone,
                text: "token header says alg none: it is not signed".into(),
            });
        }
        out
    }
}

/// Unix seconds now (`web_time`, so it works in the browser).
pub fn now_secs() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
