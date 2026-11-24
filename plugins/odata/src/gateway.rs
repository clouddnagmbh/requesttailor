// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gateway errors and `sap-statistics` (wave 18, strand J): decodes SAP Gateway error
//! bodies (V2 JSON and XML, V4) into a readable note and tab, and turns the `sap-statistics`
//! response header into `NotesReply.segments`. Interface: no contributions yet.

use rt_plugin_sdk::api::Contributions;

/// What this module adds to the manifest (tabs; the hooks are already on). Keep it in step with
/// `plugin.json`: `tests/manifest.rs` compares both.
pub fn contributions() -> Contributions {
    Contributions::default()
}
