// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! External secrets for `rt send` (wave 5, strand E): the values of the selected
//! environment's `external_secrets` from the manager's own CLI or API (1Password `op`,
//! HashiCorp Vault, AWS, Azure, GCP), put into the environment layer like keychain values.
//!
//! Interface commit: returns the collection unchanged. Strand E owns this file.

use rt_core::model::Collection;

/// The collection with the external secrets of `environment` filled in.
pub async fn resolve(
    collection: &Collection,
    environment: Option<&str>,
) -> Result<Collection, String> {
    let _ = environment;
    Ok(collection.clone())
}
