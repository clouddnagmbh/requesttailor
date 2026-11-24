// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The hook from the send path to the test-data journal (wave 19, strand C). The journal
//! itself is `rt_app::journal`; `rt_core` cannot depend on it, so `send_with` only asks
//! [`enabled`] (one atomic load, nothing else when the journal is off) and hands the masked
//! [`Exchange`](crate::observe::Exchange) of a finished send to [`record`].
//!
//! Strand C fills [`record`]: it keeps only successful writing requests (`effect = Write`,
//! status 2xx), copies a few fields and hands them to a background channel. The interface
//! commit leaves it empty, and the switch off, so the send path stays byte and time
//! identical to wave 18.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::observe::Exchange;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether a journal is listening (set by the shell when the user switches "Track test data"
/// on for this device; strand C).
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Switches the hook on or off (strand C calls it from `rt_app::journal`).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Called by `send_with` with the masked exchange of a finished send, only if [`enabled`].
/// Quick and non-blocking. Interface: does nothing.
pub fn record(exchange: &Exchange) {
    let _ = exchange;
}
