// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Finding and describing tokens (wave 18, strand B). A token is read, never verified; its value
//! is returned to the caller that asked for it (the JWT view) and goes nowhere else: no file,
//! history, report, note or exchange.

use rt_core::jwt::{self, IssueKind, Jwt};
use rt_core::transport::HttpRequest;
use serde_json::Value;

use super::{ClaimKind, FoundToken, JwtClaim, JwtInfo, JwtWarning, JwtWarningKind, TokenLocation};

/// The claims shown first, in this order, with a short meaning.
const WELL_KNOWN: &[(&str, &str)] = &[
    ("iss", "Issuer: who created the token"),
    ("sub", "Subject: who the token is about"),
    ("aud", "Audience: who the token is meant for"),
    ("iat", "Issued at"),
    ("nbf", "Not before: invalid until this time"),
    ("exp", "Expires: invalid from this time"),
    ("scope", "Scopes the token grants"),
    ("scp", "Scopes the token grants"),
    (
        "azp",
        "Authorized party: the client the token was issued to",
    ),
];

/// Claims that hold a Unix time in seconds.
const TIME_CLAIMS: &[&str] = &["iat", "nbf", "exp", "auth_time", "updated_at"];

/// Describes a parsed token at `now` (Unix seconds).
pub fn describe(token: &Jwt, now: i64) -> JwtInfo {
    let object = token.claims.as_object();
    let mut claims = Vec::new();
    let mut taken = Vec::new();
    if let Some(object) = object {
        for (name, meaning) in WELL_KNOWN {
            if let Some(value) = object.get(*name) {
                claims.push(claim(name, value, Some(meaning)));
                taken.push(*name);
            }
        }
        for (name, value) in object {
            if !taken.contains(&name.as_str()) {
                claims.push(claim(name, value, None));
            }
        }
    }
    let warnings = token
        .issues(now)
        .into_iter()
        .map(|i| JwtWarning {
            kind: match i.kind {
                IssueKind::Expired => JwtWarningKind::Expired,
                IssueKind::NotYetValid => JwtWarningKind::NotYetValid,
                IssueKind::ExpiresSoon => JwtWarningKind::ExpiresSoon,
                IssueKind::AlgorithmNone => JwtWarningKind::AlgorithmNone,
            },
            text: i.text,
        })
        .collect();
    JwtInfo {
        header: token.header.clone(),
        algorithm: token.algorithm().map(str::to_owned),
        claims,
        warnings,
        expires_at: token.time_claim("exp"),
        signature_verified: false,
    }
}

fn claim(name: &str, value: &Value, meaning: Option<&str>) -> JwtClaim {
    let kind = if TIME_CLAIMS.contains(&name) && value.is_number() {
        ClaimKind::Time
    } else if value.is_array() || (matches!(name, "scope") && value.is_string()) {
        ClaimKind::List
    } else if value.is_string() {
        ClaimKind::Text
    } else {
        ClaimKind::Other
    };
    JwtClaim {
        name: name.to_owned(),
        value: value.clone(),
        kind,
        meaning: meaning.map(str::to_owned),
    }
}

/// The tokens of a resolved request: the bearer token (`auth` if it came from the request's
/// auth block) and `access_token` in the query.
pub fn in_request(request: &HttpRequest, from_auth_block: bool, now: i64) -> Vec<FoundToken> {
    use rt_core::inspect::{RequestTokenPlace, request_tokens};
    request_tokens(request)
        .into_iter()
        .filter_map(|(place, value)| {
            let parsed = jwt::parse(&value).ok()?;
            let location = match place {
                RequestTokenPlace::Authorization if from_auth_block => TokenLocation::Auth,
                RequestTokenPlace::Authorization => TokenLocation::Authorization,
                RequestTokenPlace::Query => TokenLocation::Query,
            };
            Some(FoundToken {
                location,
                token: describe(&parsed, now),
            })
        })
        .collect()
}

/// Fields of a JSON body that hold a token.
const BODY_FIELDS: &[&str] = &["access_token", "id_token", "token"];
const MAX_FOUND: usize = 16;
const MAX_DEPTH: usize = 6;

/// The tokens of a response: `access_token`/`id_token`/`token` fields of a JSON body (given
/// the body's first bytes; a body cut short is not parsed) and `Set-Cookie` values.
pub fn in_response(
    headers: &[(String, String)],
    body: &[u8],
    body_complete: bool,
    now: i64,
) -> Vec<FoundToken> {
    let mut out = Vec::new();
    for (name, value) in headers {
        if !name.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let first = value.split(';').next().unwrap_or("");
        let (cookie, content) = first.split_once('=').unwrap_or(("", first));
        if let Ok(parsed) = jwt::parse(content.trim()) {
            out.push(FoundToken {
                location: TokenLocation::Cookie {
                    name: cookie.trim().to_owned(),
                },
                token: describe(&parsed, now),
            });
        }
    }
    let looks_json = body
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| matches!(b, b'{' | b'['));
    if body_complete
        && looks_json
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        walk(&value, "$", 0, now, &mut out);
    }
    out.truncate(MAX_FOUND);
    out
}

fn walk(value: &Value, path: &str, depth: usize, now: i64, out: &mut Vec<FoundToken>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FOUND {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                let here = format!("{path}.{key}");
                match v {
                    Value::String(s) if BODY_FIELDS.contains(&key.as_str()) => {
                        if let Ok(parsed) = jwt::parse(s) {
                            out.push(FoundToken {
                                location: TokenLocation::Body { path: here },
                                token: describe(&parsed, now),
                            });
                        }
                    }
                    _ => walk(v, &here, depth + 1, now, out),
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, &format!("{path}[{i}]"), depth + 1, now, out);
            }
        }
        _ => {}
    }
}
