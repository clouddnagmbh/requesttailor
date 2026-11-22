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
//! Owner: wave 4, strand E. Changes to the public types are interface changes.

use rt_core::send::{SendError, SendReport};
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

/// The history of this device, in memory. Strand E implements recording, search and
/// pruning; until then it records nothing.
#[derive(Debug, Default, Clone)]
pub struct History {
    entries: Vec<HistoryEntry>,
    next: u64,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restores persisted entries (any order; duplicates by `id` are dropped).
    pub fn load(entries: Vec<HistoryEntry>) -> Self {
        let next = entries.iter().map(|e| e.id).max().unwrap_or(0);
        Self { entries, next }
    }

    /// Records one send and returns the entry to persist.
    pub fn record(
        &mut self,
        collection: &str,
        name: &str,
        method: &str,
        at: i64,
        report: &SendReport,
    ) -> HistoryEntry {
        self.next += 1;
        HistoryEntry {
            id: self.next,
            at,
            collection: collection.to_owned(),
            request_path: report.request_path.clone(),
            name: name.to_owned(),
            environment: report.environment.clone(),
            method: method.to_owned(),
            url: report.request.as_ref().map(|r| r.url.clone()),
            status: report.response.as_ref().map(|r| r.status),
            error: report.error.clone(),
            duration_ms: report
                .response
                .as_ref()
                .map(|r| r.total.as_secs_f64() * 1000.0),
            size: report.response.as_ref().map(|r| r.size),
        }
    }

    /// Entries of `collection` matching `query` (empty: all), newest first, at most `limit`.
    pub fn list(&self, collection: &str, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let _ = (collection, query, limit);
        Vec::new()
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
