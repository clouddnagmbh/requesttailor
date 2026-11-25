// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Passive inspection of what is sent and what comes back (wave 18, strand B). The types live
//! here because [`crate::send::SendReport::checks`] carries them; the app side is
//! `rt_app::inspect`.
//!
//! An [`Observation`] is not a vulnerability: it says what the response (or request) shows,
//! why that may matter and how to fix it. Nothing here sends anything. Token values never
//! appear in an observation, a note or a report: claim names and deadlines only.

use serde::{Deserialize, Serialize};

use crate::services::{Note, NoteLevel};
use crate::transport::HttpRequest;

/// Stable id of a rule, `chk/…` (e.g. `chk/hsts-missing`), or `jwt/…` for token notes.
pub type ObservationId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
}

/// One finding of the passive response checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub severity: Severity,
    /// Where it was seen, e.g. `header Set-Cookie`, `body`.
    pub location: String,
    /// One sentence: why it matters.
    pub why: String,
    /// One sentence: how to fix it.
    pub fix: String,
}

/// Where a token was found in a resolved request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTokenPlace {
    /// `Authorization: Bearer …`.
    Authorization,
    /// The query parameter `access_token`.
    Query,
}

/// The token-shaped values of a resolved request: the bearer token of `Authorization` and the
/// query parameter `access_token`. Only candidates; [`crate::jwt::parse`] decides.
pub fn request_tokens(request: &HttpRequest) -> Vec<(RequestTokenPlace, String)> {
    let mut out = Vec::new();
    for (name, value) in &request.headers {
        if name.eq_ignore_ascii_case("authorization") {
            let v = value.trim();
            if v.get(..7)
                .is_some_and(|p| p.eq_ignore_ascii_case("bearer "))
            {
                out.push((RequestTokenPlace::Authorization, v[7..].trim().to_owned()));
            }
        }
    }
    if let Some((_, query)) = request.url.split_once('?') {
        let query = query.split('#').next().unwrap_or("");
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("access_token=") {
                out.push((RequestTokenPlace::Query, percent_decode(value)));
            }
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Notes about the resolved request before it goes out (e.g. "the bearer token expired 12
/// minutes ago"). Called by `send_with` right before the wire. Warns about an expired, not
/// yet valid or nearly expired token (`Authorization: Bearer`, query `access_token`); it never
/// blocks, and a token value never appears in a note (claim name and deadline only).
pub fn pre_send_notes(request: &HttpRequest) -> Vec<Note> {
    notes_at(request, crate::jwt::now_secs())
}

/// [`pre_send_notes`] at a given time (Unix seconds), for tests.
pub fn notes_at(request: &HttpRequest, now: i64) -> Vec<Note> {
    use crate::jwt::IssueKind;
    let mut notes = Vec::new();
    for (place, token) in request_tokens(request) {
        let Ok(jwt) = crate::jwt::parse(&token) else {
            continue;
        };
        let place = match place {
            RequestTokenPlace::Authorization => "Authorization header",
            RequestTokenPlace::Query => "access_token parameter",
        };
        for issue in jwt.issues(now) {
            if issue.kind == IssueKind::AlgorithmNone {
                continue;
            }
            notes.push(Note {
                source: "inspect".into(),
                level: NoteLevel::Warning,
                text: format!("{place}: {}", issue.text),
            });
        }
    }
    notes
}
