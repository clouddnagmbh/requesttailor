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

/// Reads `token` (without a `Bearer ` prefix). Wave 18 interface: always `Unsupported` until
/// strand B.
pub fn parse(token: &str) -> Result<Jwt, JwtError> {
    let _ = token;
    Err(JwtError::Unsupported(
        "Reading tokens is not implemented yet (wave 18, strand B).".into(),
    ))
}
