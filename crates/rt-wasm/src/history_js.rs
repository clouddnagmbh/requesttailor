// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! History helpers for the web shell (`rt_app::history`). Owner: wave 4, strand E.
//!
//! The web shell keeps the entries in IndexedDB; recording, filtering and pruning run here,
//! in the same code as on the desktop, so both shells list and prune identically.
//!
//! ```text
//! new HistoryLog(entries)                     entries: HistoryEntry[] from storage
//! record(collection, name, method, at, report) -> { entry: HistoryEntry, dropped: number[] }
//! list(collection, query, limit) -> HistoryEntry[]
//! clear(collection) -> number[]               ids of the removed entries
//! ```
//!
//! The shell stores `entry` and deletes the `dropped` ids (pruned beyond `HISTORY_LIMIT`).
//! Stored entries that do not parse are skipped.

use rt_app::{History, HistoryEntry};
use rt_core::send::SendReport;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{from_js, int, to_js};

#[derive(Serialize)]
struct Recorded {
    entry: HistoryEntry,
    dropped: Vec<u64>,
}

/// This device's history in memory (`rt_app::History`).
#[wasm_bindgen]
pub struct HistoryLog {
    inner: History,
}

#[wasm_bindgen]
impl HistoryLog {
    #[wasm_bindgen(constructor)]
    pub fn new(entries: JsValue) -> HistoryLog {
        let entries = js_sys::Array::from(&entries)
            .iter()
            .filter_map(|e| from_js::<HistoryEntry>(&e, "history entry").ok())
            .collect();
        HistoryLog {
            inner: History::load(entries),
        }
    }

    /// Records one send (`report`: the `SendReport` of the result) at `at` (Unix ms).
    pub fn record(
        &mut self,
        collection: &str,
        name: &str,
        method: &str,
        at: f64,
        report: JsValue,
    ) -> Result<JsValue, JsValue> {
        let report: SendReport = from_js(&report, "report")?;
        let at = if at.is_finite() { at as i64 } else { 0 };
        let entry = self.inner.record(collection, name, method, at, &report);
        let dropped = self.inner.prune().into_iter().map(|e| e.id).collect();
        to_js(&Recorded { entry, dropped })
    }

    pub fn list(&self, collection: &str, query: &str, limit: f64) -> Result<JsValue, JsValue> {
        to_js(&self.inner.list(collection, query, int(limit)? as usize))
    }

    pub fn clear(&mut self, collection: &str) -> Result<JsValue, JsValue> {
        let removed: Vec<u64> = self
            .inner
            .entries()
            .iter()
            .filter(|e| e.collection == collection)
            .map(|e| e.id)
            .collect();
        self.inner.clear(collection);
        to_js(&removed)
    }
}
