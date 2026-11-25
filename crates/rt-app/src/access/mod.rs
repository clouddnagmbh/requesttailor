// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two-user access check of one request (wave 19, strand D): `Backend.access.check`,
//! `rt access check`. A *profile* is an **environment** (two environments A and B with their
//! own token, cookie and secrets); there is no new auth logic. `check` runs the request under
//! A (the reference) and, if the response holds an object (a path parameter, an id in the
//! body), the same access under B, with A's ids set as variables in B's run (from `Location`,
//! an `actions` variable or a path parameter the request takes from variables; without a
//! recognisable id the check says "no object reference found" and does not guess).
//!
//! Findings: [`AccessVerdict::BrokenObjectAccess`] (B got a 2xx with A's object: same
//! structure or the same id in the body), [`AccessVerdict::BrokenFunctionAccess`] (B got 2xx
//! on an operation that must be denied; only if [`AccessOptions::b_must_be_denied`]),
//! [`AccessVerdict::Inconclusive`] (404 for both, different errors), [`AccessVerdict::Ok`]
//! (403/404/401 for B). Nothing is *proved*: the text says "B received object 4711 that A
//! created" with **masked evidence** (status, header names, JSON paths and shortened masked
//! values, at most [`EVIDENCE_LIMIT`] bytes each, no personal data: paths and types, not
//! values).
//!
//! Rules: reading requests (`effect = Read`) run without a question; a writing request runs
//! only with [`AccessOptions::allow_writes`] (explicit, per run, off by default, in the
//! audit log) and only where neither profile is production (`rt_app::agents::is_production`),
//! else [`AccessVerdict::Skipped`] with `skipped (writes)` or `skipped (production)`. The
//! guard is asked per request; no brute force, no id enumeration (only ids A itself saw),
//! the rate is limited (default [`DEFAULT_RATE`] requests per second, and the rate limits the
//! host reports are waited for, never retried). One audit entry per check.

use serde::{Deserialize, Serialize};

mod check;
mod evidence;

pub use check::{AccessEnv, NO_REFERENCE, SKIPPED_PRODUCTION, SKIPPED_WRITES, check, check_with};

/// Requests per second when [`AccessOptions::rate`] is `0`.
pub const DEFAULT_RATE: u32 = 5;

/// Longest evidence value, in bytes.
pub const EVIDENCE_LIMIT: usize = 512;

/// The two profiles: environment names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessProfile {
    /// The reference ("user A").
    pub a: String,
    /// The one that must not see A's objects ("user B").
    pub b: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccessOptions {
    /// The user confirmed that this check may send writing requests (non-production only).
    #[serde(default)]
    pub allow_writes: bool,
    /// B must be denied on this operation (switches on `broken-function-access`).
    #[serde(default)]
    pub b_must_be_denied: bool,
    /// Requests per second (`0`: the default of 5).
    #[serde(default)]
    pub rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AccessVerdict {
    Ok,
    BrokenObjectAccess,
    BrokenFunctionAccess,
    Inconclusive,
    /// Not run: `reason` is `skipped (writes)`, `skipped (production)`, `no object reference
    /// found` or what the guard said.
    Skipped {
        reason: String,
    },
}

/// One piece of masked evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessEvidence {
    /// `A` or `B`.
    pub profile: String,
    pub status: Option<u16>,
    /// Header names only.
    #[serde(default)]
    pub header_names: Vec<String>,
    /// JSON paths and shortened, masked values (≤ [`EVIDENCE_LIMIT`] bytes).
    #[serde(default)]
    pub values: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessFinding {
    pub verdict: AccessVerdict,
    /// "B received object 4711 that A created".
    pub message: String,
    pub evidence: Vec<AccessEvidence>,
}

/// The result of `check` for one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessCheck {
    pub path: String,
    pub verdict: AccessVerdict,
    pub findings: Vec<AccessFinding>,
    /// Status of A's and B's response, if they were sent.
    pub status_a: Option<u16>,
    pub status_b: Option<u16>,
}

impl AccessVerdict {
    /// `rt access check` exits with `EXIT_TESTS` for these.
    pub fn is_broken(&self) -> bool {
        matches!(
            self,
            AccessVerdict::BrokenObjectAccess | AccessVerdict::BrokenFunctionAccess
        )
    }
}
