// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Finding tokens for the passive security rules (wave 19, strand R). Linear in the bytes read:
//! a byte search for the start of a token (`eyJ`), no parsing of the whole body. A token is read
//! (never verified) and only its form goes on; the text stays inside the rule that asked.

use memchr::memmem;
use rt_core::jwt::{self, Jwt};

use crate::inspect::ResponseView;

/// At most this many candidates are looked at per haystack.
const MAX_CANDIDATES: usize = 16;
/// A candidate longer than this is not a token worth reading.
const MAX_CANDIDATE_LEN: usize = 8192;

/// A token found in a request or a response.
pub(super) struct Found<'a> {
    /// Where, for the finding: `body access_token`, `header Set-Cookie (session)`.
    pub place: String,
    /// The text of the token; only the rules read it and never repeat it.
    pub text: &'a str,
    pub jwt: Jwt,
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'
}

/// The token-shaped strings of `haystack` that [`jwt::parse`] accepts, with their offset.
fn scan(haystack: &[u8]) -> Vec<(usize, &str, Jwt)> {
    let mut out = Vec::new();
    let mut seen = 0;
    for at in memmem::find_iter(haystack, b"eyJ") {
        if at > 0 && is_token_byte(haystack[at - 1]) {
            continue;
        }
        seen += 1;
        if seen > MAX_CANDIDATES {
            break;
        }
        let rest = &haystack[at..];
        let len = rest
            .iter()
            .take(MAX_CANDIDATE_LEN)
            .position(|&b| !is_token_byte(b))
            .unwrap_or(rest.len().min(MAX_CANDIDATE_LEN));
        let Ok(text) = std::str::from_utf8(&rest[..len]) else {
            continue;
        };
        // Two dots; an `alg: none` token ends in a dot and has an empty signature.
        if text.matches('.').count() == 2
            && let Ok(parsed) = jwt::parse(text)
        {
            out.push((at, text, parsed));
        }
    }
    out
}

/// The JSON key before the value at `at` (`"access_token":"eyJ…` gives `access_token`).
fn key_before(body: &[u8], at: usize) -> Option<String> {
    let mut end = at;
    let skip = |end: &mut usize, set: &[u8]| {
        while *end > 0 && set.contains(&body[*end - 1]) {
            *end -= 1;
        }
    };
    skip(&mut end, b"\" \t\r\n");
    if end == 0 || body[end - 1] != b':' {
        return None;
    }
    end -= 1;
    skip(&mut end, b" \t\r\n");
    if end == 0 || body[end - 1] != b'"' {
        return None;
    }
    end -= 1;
    let start = body[..end].iter().rposition(|&b| b == b'"')? + 1;
    let key = std::str::from_utf8(&body[start..end]).ok()?;
    (!key.is_empty() && key.len() <= 64).then(|| key.to_owned())
}

/// The tokens a response carries: any header value (`Set-Cookie` by cookie name) and the body.
pub(super) fn in_response<'a>(view: &ResponseView<'a>) -> Vec<Found<'a>> {
    let mut out = Vec::new();
    for (name, value) in view.headers {
        let value: &'a str = value;
        for (_, text, parsed) in scan(value.as_bytes()) {
            let place = if name.eq_ignore_ascii_case("set-cookie") {
                let cookie = value.split('=').next().unwrap_or("").trim();
                format!("header Set-Cookie ({cookie})")
            } else {
                format!("header {name}")
            };
            out.push(Found {
                place,
                text,
                jwt: parsed,
            });
        }
    }
    for (at, text, parsed) in scan(view.body) {
        let place = match key_before(view.body, at) {
            Some(key) => format!("body {key}"),
            None => "body".to_owned(),
        };
        out.push(Found {
            place,
            text,
            jwt: parsed,
        });
    }
    out
}

/// The bearer token of the request as it was sent. A masked value (a secret) is no token.
pub(super) fn in_request<'a>(view: &ResponseView<'a>) -> Vec<Found<'a>> {
    let mut out = Vec::new();
    for (name, value) in view.request_headers {
        if !name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        let value: &'a str = value;
        let v = value.trim();
        let Some(token) = v
            .get(..7)
            .filter(|p| p.eq_ignore_ascii_case("bearer "))
            .map(|_| v[7..].trim())
        else {
            continue;
        };
        if let Ok(parsed) = jwt::parse(token) {
            out.push(Found {
                place: "request header Authorization".to_owned(),
                text: token,
                jwt: parsed,
            });
        }
    }
    out
}
