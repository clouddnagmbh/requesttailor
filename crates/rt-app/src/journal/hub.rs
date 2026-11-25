// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The journals of the open collections of this process, and the sink that hands the send path's
//! exchanges to them (wave 19, strand C). A journal is found by the collection name the send
//! reports; while no journal is switched on the sink is not installed at all, so a send costs
//! one atomic load.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rt_core::journal::JournalSink;
use rt_core::observe::Exchange;

use super::store::{Journal, JournalStorage};

fn registry() -> &'static Mutex<HashMap<String, Arc<Journal>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Journal>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

struct HubSink;

impl JournalSink for HubSink {
    fn record(&self, exchange: &Exchange) {
        let Some(name) = &exchange.collection else {
            return;
        };
        let journal = registry()
            .lock()
            .ok()
            .and_then(|map| map.get(name).cloned());
        if let Some(journal) = journal {
            journal.record(exchange);
        }
    }
}

/// The journal of the collection `name`; opened with `storage()` on first use.
pub fn journal_for(name: &str, storage: impl FnOnce() -> Arc<dyn JournalStorage>) -> Arc<Journal> {
    let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = map.get(name) {
        return Arc::clone(found);
    }
    let journal = Journal::open(name, storage());
    map.insert(name.to_owned(), Arc::clone(&journal));
    journal
}

/// Installs the sink while any journal is on, removes it when none is: call after changing a
/// journal's "Track test data".
pub fn refresh() {
    let any = registry()
        .lock()
        .map(|map| map.values().any(|j| j.enabled()))
        .unwrap_or(false);
    if any {
        rt_core::journal::set_sink(Some(Arc::new(HubSink)));
    } else {
        rt_core::journal::set_sink(None);
    }
}

/// Forgets a journal (tests; a shell that closes a collection).
pub fn forget(name: &str) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(name);
    refresh();
}
