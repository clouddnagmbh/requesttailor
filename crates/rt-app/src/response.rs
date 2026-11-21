// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Keeps full response bodies on the Rust side, so a 100 MB body never crosses into the UI
//! at once. The UI renders a virtual list and asks for the lines it shows.
//!
//! Owner: wave 3, strand C.

use rt_core::send::SendReport;
use serde::{Deserialize, Serialize};

use crate::AppError;

/// What the UI gets back from sending: the report with a short body preview, plus a handle
/// to the full body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResponse {
    /// `report.response.body` holds at most the first `PREVIEW_BYTES` bytes.
    pub report: SendReport,
    /// `None` if there was no response.
    pub response_id: Option<u64>,
    /// Body size in bytes.
    pub size: u64,
    /// Number of lines (`\n`-separated) of the full body; 0 for an empty body.
    pub line_count: u64,
    /// Whether the body looks like text (valid UTF-8 in the first `PREVIEW_BYTES`).
    pub is_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineSlice {
    /// Index of the first returned line.
    pub start: u64,
    /// Lines without their trailing `\n`; invalid UTF-8 replaced by U+FFFD. Very long lines
    /// are cut at `MAX_LINE_CHARS` and flagged in `cut`.
    pub lines: Vec<String>,
    /// Indexes (relative to `start`) of lines that were cut.
    pub cut: Vec<u32>,
}

/// Bodies kept, by id. Ids are never reused within one store.
#[derive(Debug, Default)]
pub struct ResponseStore {
    _private: (),
}

impl ResponseStore {
    pub const PREVIEW_BYTES: usize = 64 * 1024;
    pub const MAX_LINE_CHARS: usize = 10_000;

    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a report whose body is complete (or truncated at the send limit), keeps the
    /// body and returns the report with a preview.
    pub fn insert(&mut self, report: SendReport) -> StoredResponse {
        let _ = report;
        todo!("wave 3, strand C")
    }

    pub fn lines(&self, id: u64, start: u64, count: u32) -> Result<LineSlice, AppError> {
        let _ = (id, start, count);
        todo!("wave 3, strand C")
    }

    /// Raw bytes for "save response to file" and hex views.
    pub fn bytes(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, AppError> {
        let _ = (id, offset, len);
        todo!("wave 3, strand C")
    }

    /// Applies a JSONPath expression to a JSON body and returns the result as pretty JSON.
    /// Bodies above a size limit are rejected with a clear message rather than parsed.
    pub fn filter_json(&self, id: u64, expression: &str) -> Result<String, AppError> {
        let _ = (id, expression);
        todo!("wave 3, strand C")
    }

    pub fn release(&mut self, id: u64) {
        let _ = id;
        todo!("wave 3, strand C")
    }
}
