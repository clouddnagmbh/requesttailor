// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Clean-up rules of OData (wave 19, strand H) as data in the manifest
//! (`Contributions.compensations`): where the key of a created entity is (`Location`,
//! `d.<Key>`, `@odata.id`), how `EntitySet(<key>)` is made, where the ETag for `If-Match` is
//! (response header or `__metadata.etag`), the CSRF token to fetch (`x-csrf-token: fetch`),
//! and drafts as "not reversible". No WASM call per response; the app executes. Interface:
//! no rule yet.

use rt_plugin_sdk::api::Contributions;

/// What this module adds to the manifest (the compensation rules). Keep it in step with
/// `plugin.json`.
pub fn contributions() -> Contributions {
    Contributions::default()
}
