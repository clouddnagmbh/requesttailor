// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Time segments reported by a server (wave 18): a general extension point. A plugin's
//! `response` hook (`NotesReply.segments`) or, one day, the transport names parts of the time
//! the server spent, e.g. "gateway", "application", "database". The segments live in
//! [`crate::send::SendReport::segments`] and are shown as their own line under the timing bar.
//!
//! They are **never** part of the client's own phase sum (DNS, connect, TLS, server wait,
//! download): the numbers come from the server and its clock is not the client's. A segment
//! longer than the time to first byte is not cut; the UI shows a hint instead.

use serde::{Deserialize, Serialize};

/// One part of the time the server says it spent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimingSegment {
    /// Stable id within the report, e.g. `"db"`.
    pub id: String,
    /// What the user reads, e.g. `"Database"`.
    pub label: String,
    /// Milliseconds, as the server reported them.
    pub ms: u64,
    /// `Some("server")`: inside the wait for the server (time to first byte); `None`: not
    /// attributed to a client phase. Since wave 19 it may also be the `id` of another segment
    /// of the same report (a tree: a database call inside the application). An id that is not
    /// in the report counts as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Wave 19: where the segment starts, in milliseconds after the server began handling the
    /// request (the first span). `None`: unknown, the UI lays it out after its siblings. The
    /// server's clock is not the client's, so this is never an offset on the client's bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_ms: Option<f64>,
}

/// The value of [`TimingSegment::parent`] for segments inside the wait for the server.
pub const SEGMENT_PARENT_SERVER: &str = "server";

// `offset_ms` is a JSON number, which is never NaN, so equality is total.
impl Eq for TimingSegment {}
