// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: websocket` items: `websocket.{url, headers, message, auth}` and `settings.{timeout, keepAliveInterval}` (message variants). Wave 8, strand A.

use rt_core::model::{RequestEntry, RequestSpec};

use super::Result;
use crate::read::Builder;
use crate::yaml::Document;

/// Strand A. Until then the item is skipped, as before wave 8.
pub(crate) fn read(_builder: &mut Builder<'_>, _path: &str) -> Option<RequestEntry> {
    None
}

/// Strand A.
pub(crate) fn new_text(spec: &RequestSpec) -> String {
    format!("info:\n  name: {:?}\n  type: websocket\n", spec.name)
}

/// Strand A.
pub(crate) fn apply(_doc: &Document, _path: &str, _spec: &RequestSpec) -> Result<Document> {
    Err("websocket items can be edited from wave 8, strand A on".into())
}
