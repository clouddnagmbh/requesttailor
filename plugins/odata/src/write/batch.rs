// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `$batch` bodies: multipart/mixed (V2 and V4) with changesets, and their responses.

use crate::edm::Version;

/// One operation inside a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub method: String,
    /// Relative to the service root, e.g. `Books(1)`.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// A batch: reads outside changesets, and changesets (all-or-nothing groups of writes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Read(Part),
    Changeset(Vec<Part>),
}

/// The request body and its `Content-Type` (with boundary).
pub fn build(items: &[Item], version: Version, boundary: &str) -> (String, String) {
    let _ = (items, version, boundary);
    (String::new(), String::new())
}

/// One response of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// Index of the changeset it belongs to, `None` for a read.
    pub changeset: Option<usize>,
}

/// Parses a multipart batch response (`content_type` with its boundary).
pub fn parse_response(content_type: &str, body: &str) -> Vec<PartResponse> {
    let _ = (content_type, body);
    Vec::new()
}
