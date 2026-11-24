// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importing an exported BTP destination file (wave 18, strand K): the destination's URL,
//! authentication type and `sap-client` become an environment, without a network call and
//! without an access key. Secrets stay out of the file; the variable names are the shared
//! interface with the live import of a distribution that reads the destination service.
//! Interface: no contributions yet.

use rt_plugin_sdk::api::Contributions;

/// What this module adds to the manifest (the importer). Keep it in step with `plugin.json`.
pub fn contributions() -> Contributions {
    Contributions::default()
}
