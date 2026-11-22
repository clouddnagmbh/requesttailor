// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request history: every send of the app, on this device, searchable.
//!
//! The shells record each send ([`History::record`]) and persist the entries (desktop: a
//! JSON Lines file in the app's data folder; web: IndexedDB) by storing what
//! [`HistoryEntry`] serializes to. An entry holds no request or response body and no header
//! values: the URL is the masked one from the report (secrets as `***`), so the history file
//! never contains a secret value (product principle 3).
//!
//! Error messages of failed sends can quote what was about to go on the wire (an invalid URL,
//! an invalid header value) unmasked, so [`History::record`] keeps only the parts of a message
//! that cannot carry request data (see [`scrub_error`]).
//!
//! After each `record` the shells call [`History::prune`]: it drops the oldest entries beyond
//! [`HISTORY_LIMIT`] and returns them, so the shell knows whether to append one entry or to
//! rewrite its store.
//!
//! Owner: wave 4, strand E. Changes to the public types are interface changes.

use rt_core::send::{MASK, SendError, SendReport};
use rt_core::transport::TransportError;
use serde::{Deserialize, Serialize};

/// Entries kept per device; the oldest are dropped first.
pub const HISTORY_LIMIT: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unique on this device, increasing.
    pub id: u64,
    /// When it was sent, Unix milliseconds.
    pub at: i64,
    /// `CollectionInfo.id` of the collection it was sent from.
    pub collection: String,
    pub request_path: String,
    /// Request name at the time of sending.
    pub name: String,
    pub environment: Option<String>,
    pub method: String,
    /// The URL as sent, secrets masked; `None` if preparation failed.
    pub url: Option<String>,
    /// `None` if there was no response.
    pub status: Option<u16>,
    /// Why the send failed, if it did.
    pub error: Option<SendError>,
    /// Total time in milliseconds; `None` without a response.
    pub duration_ms: Option<f64>,
    /// Response body size in bytes.
    pub size: Option<u64>,
}

/// The history of this device, in memory: entries oldest first (by `id`).
#[derive(Debug, Default, Clone)]
pub struct History {
    entries: Vec<HistoryEntry>,
    next: u64,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restores persisted entries (any order; duplicates by `id` are dropped, the last one
    /// wins). Keeps at most [`HISTORY_LIMIT`], the newest.
    pub fn load(mut entries: Vec<HistoryEntry>) -> Self {
        // Stable sort, then keep the last of each run of equal ids.
        entries.sort_by_key(|e| e.id);
        let mut unique: Vec<HistoryEntry> = Vec::with_capacity(entries.len());
        for e in entries {
            match unique.last_mut() {
                Some(last) if last.id == e.id => *last = e,
                _ => unique.push(e),
            }
        }
        let next = unique.last().map_or(0, |e| e.id);
        let mut history = Self {
            entries: unique,
            next,
        };
        history.prune();
        history
    }

    /// Records one send and returns the entry to persist. Call [`History::prune`] afterwards.
    pub fn record(
        &mut self,
        collection: &str,
        name: &str,
        method: &str,
        at: i64,
        report: &SendReport,
    ) -> HistoryEntry {
        self.next += 1;
        let entry = HistoryEntry {
            id: self.next,
            at,
            collection: collection.to_owned(),
            request_path: report.request_path.clone(),
            name: name.to_owned(),
            environment: report.environment.clone(),
            method: method.to_owned(),
            url: report.request.as_ref().map(|r| r.url.clone()),
            status: report.response.as_ref().map(|r| r.status),
            error: report.error.as_ref().map(scrub_error),
            duration_ms: report
                .response
                .as_ref()
                .map(|r| r.total.as_secs_f64() * 1000.0),
            size: report.response.as_ref().map(|r| r.size),
        };
        self.entries.push(entry.clone());
        entry
    }

    /// Drops the oldest entries beyond [`HISTORY_LIMIT`] and returns them (oldest first).
    pub fn prune(&mut self) -> Vec<HistoryEntry> {
        let excess = self.entries.len().saturating_sub(HISTORY_LIMIT);
        self.entries.drain(..excess).collect()
    }

    /// Entries of `collection` matching `query` (empty: all), newest first, at most `limit`.
    ///
    /// `query` is split at whitespace; every word must match (case-insensitive) the name, the
    /// method, the URL, the request path, the environment or the error, or the status: `500`
    /// (or a prefix like `50`) or a class like `4xx`.
    pub fn list(&self, collection: &str, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .rev()
            .filter(|e| e.collection == collection)
            .filter(|e| words.iter().all(|w| matches_word(e, w)))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Forgets the entries of `collection`.
    pub fn clear(&mut self, collection: &str) {
        self.entries.retain(|e| e.collection != collection);
    }

    /// All entries, oldest first (for persisting the whole history).
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }
}

/// Whether the lowercase `word` matches `entry`.
fn matches_word(entry: &HistoryEntry, word: &str) -> bool {
    let contains = |text: &str| text.to_lowercase().contains(word);
    if contains(&entry.name)
        || contains(&entry.method)
        || contains(&entry.request_path)
        || entry.url.as_deref().is_some_and(contains)
        || entry.environment.as_deref().is_some_and(contains)
    {
        return true;
    }
    if let Some(status) = entry.status {
        let status = status.to_string();
        let bytes = word.as_bytes();
        let class = bytes.len() == 3 && bytes[0].is_ascii_digit() && &word[1..] == "xx";
        if (class && status.as_bytes()[0] == bytes[0])
            || (word.bytes().all(|b| b.is_ascii_digit()) && status.starts_with(word))
        {
            return true;
        }
    }
    match &entry.error {
        Some(error) => word == "error" || word == "failed" || contains(&error_text(error)),
        None => false,
    }
}

/// The error's kinds and message as one searchable text, e.g. `transport connect refused`.
fn error_text(error: &SendError) -> String {
    match error {
        SendError::NotFound(m) => format!("not-found {m}"),
        SendError::Prepare(m) => format!("prepare {m}"),
        SendError::Transport(t) => {
            let kind = serde_json::to_value(t).unwrap_or_default();
            let kind = kind["kind"].as_str().unwrap_or_default().to_owned();
            format!("transport {kind} {t}")
        }
    }
}

/// The error with every part removed that may quote the request (URL, header values) before
/// masking: the core's messages for invalid URLs and header values contain them verbatim.
///
/// - Invalid URLs keep only the reason (`missing host`), not the URL.
/// - `invalid value for header NAME: "…"` keeps only the header name.
/// - Anything that looks like a URL (`scheme://…`) in any other message becomes `***`.
pub fn scrub_error(error: &SendError) -> SendError {
    match error {
        SendError::NotFound(path) => SendError::NotFound(path.clone()),
        SendError::Prepare(m) => SendError::Prepare(scrub_prepare(m)),
        SendError::Transport(t) => SendError::Transport(match t {
            TransportError::InvalidUrl(m) => TransportError::InvalidUrl(url_reason(m)),
            TransportError::Dns(m) => TransportError::Dns(scrub_urls(m)),
            TransportError::Connect(m) => TransportError::Connect(scrub_urls(m)),
            TransportError::Tls(m) => TransportError::Tls(scrub_urls(m)),
            TransportError::Blocked(m) => TransportError::Blocked(scrub_urls(m)),
            TransportError::Other(m) => TransportError::Other(scrub_other(m)),
            TransportError::Timeout => TransportError::Timeout,
            TransportError::TooManyRedirects(n) => TransportError::TooManyRedirects(*n),
        }),
    }
}

const INVALID_URL_AFTER_VARS: &str = "invalid URL after variable substitution";

fn scrub_prepare(message: &str) -> String {
    if message.starts_with(INVALID_URL_AFTER_VARS) {
        INVALID_URL_AFTER_VARS.to_owned()
    } else {
        scrub_urls(message)
    }
}

/// `"{url}: {why}"` → `why` (the reason never contains the URL).
fn url_reason(message: &str) -> String {
    match message.rsplit_once(": ") {
        Some((_, why)) => scrub_urls(why),
        None => String::new(),
    }
}

fn scrub_other(message: &str) -> String {
    const HEADER: &str = "invalid value for header ";
    if let Some(rest) = message.strip_prefix(HEADER) {
        let name = rest.split(':').next().unwrap_or_default();
        return format!("{HEADER}{name}");
    }
    scrub_urls(message)
}

/// Replaces every `scheme://…` run (up to whitespace or a quote) with [`MASK`].
fn scrub_urls(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(at) = rest.find("://") {
        // The scheme: ASCII letters, digits, `+`, `-`, `.` before `://`.
        let start = rest[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .map_or(0, |i| i + 1);
        let end = rest[at + 3..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
            .map_or(rest.len(), |i| at + 3 + i);
        if end == at + 3 {
            // A bare `scheme://` (e.g. in "only http:// URLs") quotes nothing.
            out.push_str(&rest[..end]);
        } else {
            out.push_str(&rest[..start]);
            out.push_str(MASK);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_scrubbed() {
        assert_eq!(
            scrub_urls("error sending http://h/x?k=s3cr3t now"),
            "error sending *** now"
        );
        assert_eq!(scrub_urls("\"https://a/b\": c"), "\"***\": c");
        assert_eq!(scrub_urls("no url"), "no url");
        assert_eq!(scrub_urls("://x"), "***");
        assert_eq!(scrub_urls("only http:// URLs"), "only http:// URLs");
    }

    #[test]
    fn invalid_url_keeps_only_the_reason() {
        assert_eq!(url_reason("ftp://h/s3cr3t: only http"), "only http");
        assert_eq!(url_reason("s3cr3t"), "");
    }

    #[test]
    fn invalid_header_value_keeps_only_the_name() {
        assert_eq!(
            scrub_other("invalid value for header Authorization: \"Bearer s3\\ncr3t\""),
            "invalid value for header Authorization"
        );
    }
}
