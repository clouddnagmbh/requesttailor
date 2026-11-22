// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Full-text search over the open collection, including unsaved changes.
//!
//! Owner: wave 4, strand E. Changes to the public types are interface changes.

use serde::{Deserialize, Serialize};

use crate::Workspace;

/// Hits returned at most.
pub const SEARCH_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    /// Request name.
    pub name: String,
    pub method: String,
    pub field: SearchField,
    /// The matching text with some context, on one line.
    pub snippet: String,
    /// Matches in `snippet` as `[start, end)` UTF-16 offsets (JavaScript string indexes).
    pub ranges: Vec<(u32, u32)>,
}

/// Where a hit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchField {
    Name,
    Url,
    Param,
    Header,
    Body,
    Auth,
    Variable,
}

/// Requests of `workspace` matching `query` (case-insensitive), best first, at most `limit`.
pub fn search(workspace: &Workspace, query: &str, limit: usize) -> Vec<SearchHit> {
    let _ = (workspace, query, limit);
    Vec::new()
}
