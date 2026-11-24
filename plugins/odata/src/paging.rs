// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging rules of OData (wave 18, strand J): `@odata.nextLink` (V4) and `d.__next` (V2) as
//! data in the manifest (`Contributions.paging`), so the core follows pages without knowing
//! OData. Interface: no rule yet.

use rt_plugin_sdk::api::Contributions;

/// What this module adds to the manifest (the paging rules). Keep it in step with
/// `plugin.json`.
pub fn contributions() -> Contributions {
    Contributions::default()
}
