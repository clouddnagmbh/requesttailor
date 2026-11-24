// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! JWT inspector and passive response checks (wave 18, strand B). The token is read, never
//! verified; its value is shown and never written to a file, history, report, note, lint
//! result or exchange (claim names and deadlines only). The checks look at responses the
//! user already got and send nothing. Interface: types only.
//!
//! Rules of the checks (ids `chk/…`, severities `info` and `warn`) and where a token is
//! looked for are in `CLAUDE.md`.

use serde::{Deserialize, Serialize};

pub use rt_core::inspect::{Observation, ObservationId, Severity};

/// A token decoded for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtInfo {
    pub header: serde_json::Value,
    /// `alg` of the header, if there is one.
    pub algorithm: Option<String>,
    /// The claims, well-known ones first (`iss`, `sub`, `aud`, `iat`, `nbf`, `exp`,
    /// `scope`/`scp`, `azp`), then the others in the token's order.
    pub claims: Vec<JwtClaim>,
    pub warnings: Vec<JwtWarning>,
    /// Unix seconds of `exp`, for the countdown.
    pub expires_at: Option<i64>,
    /// Always `false`: nothing verifies the signature.
    pub signature_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    Text,
    /// A Unix time in seconds (`iat`, `nbf`, `exp`, `auth_time`).
    Time,
    List,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtClaim {
    pub name: String,
    pub value: serde_json::Value,
    pub kind: ClaimKind,
    /// A short explanation of a well-known claim.
    pub meaning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JwtWarningKind {
    Expired,
    NotYetValid,
    /// Expires in under 30 seconds.
    ExpiresSoon,
    /// `alg: none`.
    AlgorithmNone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtWarning {
    pub kind: JwtWarningKind,
    /// Claim name and deadline, never a value: `token expired 12 min ago`.
    pub text: String,
}

/// Where a token was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TokenLocation {
    /// `Authorization: Bearer …` of the resolved request.
    Authorization,
    /// The request's auth block.
    Auth,
    /// The query parameter `access_token`.
    Query,
    /// A field of a JSON response body (`access_token`, `id_token`, `token`).
    Body { path: String },
    /// A `Set-Cookie` of the response.
    Cookie { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundToken {
    pub location: TokenLocation,
    pub token: JwtInfo,
}

/// Where a check is switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckScope {
    /// This device (stays out of the repository).
    Device,
    /// The collection (`extensions.requesttailor.checks.ignore` in `opencollection.yml`).
    Collection,
}

/// Request-side input of `InspectApi.tokens`: which request to look at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectTarget {
    /// Request path in the active collection.
    pub path: String,
    #[serde(default)]
    pub environment: Option<String>,
    /// A response to look at as well (`None`: the request only).
    #[serde(default)]
    pub response_id: Option<u64>,
}
