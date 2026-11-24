// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Keeps full response bodies on the Rust side, so a 100 MB body never crosses into the UI
//! at once. The UI renders a virtual list and asks for the lines it shows.
//!
//! Owner: wave 3, strand C.
//!
//! Memory: the body is kept once, as bytes. A UTF-8 report body is taken over without a copy
//! (`String::into_bytes`); a base64 body is decoded once and the base64 text dropped. Text
//! bodies get a line index of one `u32` per line (`u64` above 4 GiB), about 6.5 MB for a
//! 100 MB body of 64-byte lines. Lines are decoded only when asked for.
//!
//! Lines: a body is split at `\n`; a `\r` right before it belongs to the line break. A final
//! `\n` ends the last line and does not start a new one, so `"a\nb\n"` has two lines and the
//! empty body has none. Binary bodies (`is_text == false`) have no line index:
//! `line_count` is 0 and [`ResponseStore::lines`] returns an empty slice. Read them with
//! [`ResponseStore::bytes`].

use std::collections::HashMap;

use base64::Engine as _;
use rt_core::send::{ReportBody, SendReport};
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
    /// Wave 17: `false` if the collection switched exchange observers off
    /// (`extensions.requesttailor.observe.enabled: false`); then `AppExtension.onExchange`
    /// is not called for this send. Omitted from the JSON when `true`.
    #[serde(default = "observed", skip_serializing_if = "is_observed")]
    pub observe: bool,
}

fn observed() -> bool {
    true
}

fn is_observed(observe: &bool) -> bool {
    *observe
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
    entries: HashMap<u64, Entry>,
    /// Last id handed out; ids start at 1.
    last_id: u64,
}

impl ResponseStore {
    pub const PREVIEW_BYTES: usize = 64 * 1024;
    pub const MAX_LINE_CHARS: usize = 10_000;
    /// Largest body [`ResponseStore::filter_json`] parses. A parsed JSON value takes several
    /// times its text size; above this the UI shows the raw lines only.
    const FILTER_LIMIT_BYTES: usize = 32 * 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a report whose body is complete (or truncated at the send limit), keeps the
    /// body and returns the report with a preview.
    pub fn insert(&mut self, mut report: SendReport) -> StoredResponse {
        let Some(response) = report.response.as_mut() else {
            return StoredResponse {
                report,
                response_id: None,
                size: 0,
                line_count: 0,
                is_text: false,
                observe: true,
            };
        };
        let placeholder = ReportBody::Utf8(String::new());
        let body: Vec<u8> = match std::mem::replace(&mut response.body, placeholder) {
            ReportBody::Utf8(s) => s.into_bytes(),
            // rt-core produced the base64 itself, so it is valid; a broken one would be a bug
            // there, and an empty body is the least surprising answer.
            ReportBody::Base64(b) => base64::engine::general_purpose::STANDARD
                .decode(b)
                .unwrap_or_default(),
        };
        let text_prefix = utf8_prefix(&body[..body.len().min(Self::PREVIEW_BYTES)]);
        let is_text = text_prefix.is_some();
        response.body = match text_prefix {
            Some(prefix) => ReportBody::Utf8(prefix.to_owned()),
            None => ReportBody::Base64(
                base64::engine::general_purpose::STANDARD
                    .encode(&body[..body.len().min(Self::PREVIEW_BYTES)]),
            ),
        };
        let index = if is_text {
            LineIndex::build(&body)
        } else {
            LineIndex::Narrow(Vec::new())
        };
        let size = body.len() as u64;
        let line_count = index.len() as u64;
        self.last_id += 1;
        let id = self.last_id;
        self.entries.insert(
            id,
            Entry {
                body,
                index,
                report: report.clone(),
            },
        );
        StoredResponse {
            report,
            response_id: Some(id),
            size,
            line_count,
            is_text,
            observe: true,
        }
    }

    pub fn lines(&self, id: u64, start: u64, count: u32) -> Result<LineSlice, AppError> {
        let entry = self.entry(id)?;
        let total = entry.index.len() as u64;
        let first = start.min(total);
        let end = first.saturating_add(u64::from(count)).min(total);
        let mut lines = Vec::with_capacity((end - first) as usize);
        let mut cut = Vec::new();
        for (k, i) in (first..end).enumerate() {
            let (text, was_cut) = decode_line(entry.line(i as usize), Self::MAX_LINE_CHARS);
            if was_cut {
                cut.push(k as u32);
            }
            lines.push(text);
        }
        Ok(LineSlice { start, lines, cut })
    }

    /// Raw bytes for "save response to file" and hex views.
    pub fn bytes(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, AppError> {
        let body = &self.entry(id)?.body;
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(body.len());
        let to = from.saturating_add(len as usize).min(body.len());
        Ok(body[from..to].to_vec())
    }

    /// Applies a JSONPath expression to a JSON body and returns the result as pretty JSON.
    /// Bodies above a size limit are rejected with a clear message rather than parsed.
    ///
    /// The expression is RFC 9535 JSONPath; the result is the JSON array of all matched
    /// values (`[]` when nothing matches). Errors are `AppError::Filter` with a message for
    /// display: "invalid JSONPath expression: ...", "response body is not valid JSON: ..."
    /// or "response body is too large to filter ...".
    pub fn filter_json(&self, id: u64, expression: &str) -> Result<String, AppError> {
        let body = &self.entry(id)?.body;
        let path = serde_json_path::JsonPath::parse(expression)
            .map_err(|e| AppError::Filter(format!("invalid JSONPath expression: {e}")))?;
        if body.len() > Self::FILTER_LIMIT_BYTES {
            return Err(AppError::Filter(format!(
                "response body is too large to filter ({} MB; the limit is {} MB)",
                body.len().div_ceil(1024 * 1024),
                Self::FILTER_LIMIT_BYTES / (1024 * 1024)
            )));
        }
        let value: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| AppError::Filter(format!("response body is not valid JSON: {e}")))?;
        let matches = path.query(&value).all();
        serde_json::to_string_pretty(&matches).map_err(|e| AppError::Shell(e.to_string()))
    }

    /// The report of a kept response, with the body preview (wave 8: the AI assistant reads
    /// what a response looked like from here, `crate::ai`).
    pub fn report(&self, id: u64) -> Result<&SendReport, AppError> {
        Ok(&self.entry(id)?.report)
    }

    pub fn release(&mut self, id: u64) {
        self.entries.remove(&id);
    }

    fn entry(&self, id: u64) -> Result<&Entry, AppError> {
        self.entries.get(&id).ok_or(AppError::ResponseNotFound(id))
    }
}

/// One kept body.
struct Entry {
    body: Vec<u8>,
    index: LineIndex,
    /// The report as returned by `insert` (body cut to the preview).
    report: SendReport,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("size", &self.body.len())
            .field("lines", &self.index.len())
            .finish()
    }
}

impl Entry {
    /// Line `i` without its line break (`\n` or `\r\n`).
    fn line(&self, i: usize) -> &[u8] {
        let start = self.index.get(i);
        let end = if i + 1 < self.index.len() {
            // The `\n` before the next line.
            self.index.get(i + 1) - 1
        } else if self.body.last() == Some(&b'\n') {
            self.body.len() - 1
        } else {
            return &self.body[start..];
        };
        let line = &self.body[start..end];
        line.strip_suffix(b"\r").unwrap_or(line)
    }
}

/// Byte offset of the start of every line. `u32` offsets as long as they fit.
enum LineIndex {
    Narrow(Vec<u32>),
    Wide(Vec<u64>),
}

impl LineIndex {
    fn build(body: &[u8]) -> Self {
        if body.is_empty() {
            return LineIndex::Narrow(Vec::new());
        }
        // Line starts: 0 and every position after a `\n`, except the end of the body.
        let starts = std::iter::once(0).chain(
            memchr::memchr_iter(b'\n', body)
                .map(|p| p + 1)
                .filter(|&p| p < body.len()),
        );
        if u32::try_from(body.len()).is_ok() {
            let mut v: Vec<u32> = starts.map(|p| p as u32).collect();
            v.shrink_to_fit();
            LineIndex::Narrow(v)
        } else {
            let mut v: Vec<u64> = starts.map(|p| p as u64).collect();
            v.shrink_to_fit();
            LineIndex::Wide(v)
        }
    }

    fn len(&self) -> usize {
        match self {
            LineIndex::Narrow(v) => v.len(),
            LineIndex::Wide(v) => v.len(),
        }
    }

    fn get(&self, i: usize) -> usize {
        match self {
            LineIndex::Narrow(v) => v[i] as usize,
            LineIndex::Wide(v) => v[i] as usize,
        }
    }

    #[cfg(test)]
    fn heap_bytes(&self) -> usize {
        match self {
            LineIndex::Narrow(v) => v.capacity() * 4,
            LineIndex::Wide(v) => v.capacity() * 8,
        }
    }
}

/// The bytes as `&str` if they are valid UTF-8, allowing a character cut at the end (the
/// preview window may split one); that partial character is left out.
fn utf8_prefix(bytes: &[u8]) -> Option<&str> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&bytes[..e.valid_up_to()]).ok(),
        Err(_) => None,
    }
}

/// Decodes a line (invalid UTF-8 as U+FFFD), cut at `max` characters. Only the first
/// `4 * max` bytes are looked at: no character is longer than 4 bytes, and every invalid
/// sequence of up to 3 bytes becomes one U+FFFD, so that window always holds `max`
/// characters if the line has more. A character split at the window's end is the
/// `max + 1`-th or later and is dropped by the cut.
fn decode_line(line: &[u8], max: usize) -> (String, bool) {
    let window = &line[..line.len().min(max.saturating_mul(4))];
    let text = String::from_utf8_lossy(window);
    match text.char_indices().nth(max) {
        Some((at, _)) => (text[..at].to_owned(), true),
        None => (text.into_owned(), window.len() < line.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_is_compact_and_body_is_not_copied() {
        let body = "0123456\n".repeat(1000);
        let ptr = body.as_ptr();
        let mut store = ResponseStore::new();
        let report = SendReport {
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            notes: Vec::new(),
            version: 1,
            request_path: "a.yml".into(),
            environment: None,
            request: None,
            response: Some(rt_core::send::ReceivedResponse {
                status: 200,
                version: rt_core::transport::HttpVersion::Http11,
                headers: vec![],
                url: String::new(),
                redirects: vec![],
                body: ReportBody::Utf8(body),
                size: 8000,
                truncated: false,
                timing: rt_core::transport::Timing {
                    dns: None,
                    connect: None,
                    tls: None,
                    ttfb: std::time::Duration::ZERO,
                },
                total: std::time::Duration::ZERO,
            }),
            variables: vec![],
            unresolved: vec![],
            error: None,
            checks: Vec::new(),
            rate_limit: None,
            segments: Vec::new(),
        };
        let id = store.insert(report).response_id.unwrap();
        let entry = store.entry(id).unwrap();
        // The report's string became the stored body without a copy.
        assert_eq!(entry.body.as_ptr(), ptr);
        assert_eq!(entry.index.len(), 1000);
        assert_eq!(entry.index.heap_bytes(), 4000);
    }

    #[test]
    fn decode_line_cuts_at_characters() {
        assert_eq!(decode_line(b"abc", 2), ("ab".into(), true));
        assert_eq!(decode_line(b"ab", 2), ("ab".into(), false));
        assert_eq!(decode_line("€€€".as_bytes(), 2), ("€€".into(), true));
        // A four-byte character split at the window: dropped by the cut.
        let mut line = b"a".to_vec();
        line.extend_from_slice("😀😀😀".as_bytes());
        assert_eq!(decode_line(&line, 2), ("a😀".into(), true));
        assert_eq!(
            decode_line(b"\xff\xfe", 5),
            ("\u{fffd}\u{fffd}".into(), false)
        );
    }

    #[test]
    fn wide_index_answers_like_narrow() {
        let v = vec![0u64, 4, 8];
        let index = LineIndex::Wide(v);
        assert_eq!((index.len(), index.get(2)), (3, 8));
    }
}
