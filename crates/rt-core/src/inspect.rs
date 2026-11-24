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

use crate::services::Note;
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

/// Notes about the resolved request before it goes out (e.g. "the bearer token expired 12
/// minutes ago"). Called by `send_with` right before the wire. Wave 18 interface: no notes
/// until strand B. Never put a token value into a note.
pub fn pre_send_notes(request: &HttpRequest) -> Vec<Note> {
    let _ = request;
    Vec::new()
}
