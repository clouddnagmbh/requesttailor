// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar as the UI lists it (wave 5, strand D). One jar per collection and
//! environment, persisted by the shell next to the history (not in collection files: cookies
//! are session state and often credentials). Changes here are interface changes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieInfo {
    pub name: String,
    /// Shown in the cookie view only; never recorded in the history or given to agents.
    pub value: String,
    pub domain: String,
    pub path: String,
    /// Unix milliseconds; `None` for a session cookie.
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}
