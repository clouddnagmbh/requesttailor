// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The hook from the send path to the test-data journal (wave 19, strand C). The journal
//! itself is `rt_app::journal`; `rt_core` cannot depend on it, so `send_with` only asks
//! [`enabled`] (one atomic load, nothing else when the journal is off) and hands the masked
//! [`Exchange`](crate::observe::Exchange) of a finished send to [`record`].
//!
//! [`record`] keeps only successful writing requests (`effect = Write`, status 2xx) and hands
//! the exchange to the [`JournalSink`] a shell installed with [`set_sink`]. The sink copies a
//! few fields and passes them on to a background writer; the send never waits. Without a
//! sink the switch stays off and the send path costs one atomic load.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::guard::Effect;
use crate::observe::Exchange;

static ENABLED: AtomicBool = AtomicBool::new(false);
static SINK: RwLock<Option<Arc<dyn JournalSink>>> = RwLock::new(None);

/// What the shell's journal does with a finished, successful, writing exchange. Quick and
/// non-blocking: copy what is needed and return (a background channel does the writing).
pub trait JournalSink: Send + Sync {
    fn record(&self, exchange: &Exchange);
}

/// Whether a journal is listening (set by the shell when the user switches "Track test data"
/// on for this session).
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Switches the hook on or off without touching the sink.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Installs (or, with `None`, removes) the sink and switches the hook on or off with it.
pub fn set_sink(sink: Option<Arc<dyn JournalSink>>) {
    let on = sink.is_some();
    if let Ok(mut slot) = SINK.write() {
        *slot = sink;
    }
    ENABLED.store(on, Ordering::Relaxed);
}

/// Called by `send_with` with the masked exchange of a finished send, only if [`enabled`].
/// `effect` is what the request as sent does (`rt_core::guard::effect`). Only a successful
/// (2xx) writing request without an error reaches the sink.
pub fn record(exchange: &Exchange, effect: Effect) {
    if effect != Effect::Write || exchange.report.error.is_some() {
        return;
    }
    let ok = exchange
        .report
        .response
        .as_ref()
        .is_some_and(|r| (200..300).contains(&r.status));
    if !ok {
        return;
    }
    let sink = SINK.read().ok().and_then(|slot| slot.clone());
    if let Some(sink) = sink {
        sink.record(exchange);
    }
}
