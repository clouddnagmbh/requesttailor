// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The metadata cache in plugin storage. The format is an interface of the plugin (strands B,
//! C and D read it):
//! - `service::storage_key("metadata", root)`: `serde_json::to_value(model)`;
//! - `"metadata-roots"`: JSON array of the service roots with a cached model, oldest first.

use rt_plugin_sdk::{Error, Result, host};

use crate::edm::Model;
use crate::service::storage_key;

/// Storage key of the list of known roots.
pub const ROOTS_KEY: &str = "metadata-roots";

pub fn cached(service_root: &str) -> Result<Option<Model>> {
    match host::storage_get(storage_key("metadata", service_root))? {
        None => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|e| Error::new(format!("cached metadata of {service_root}: {e}"))),
    }
}

pub fn store(model: &Model) -> Result<()> {
    let root = &model.service_root;
    host::storage_set(storage_key("metadata", root), serde_json::to_value(model)?)?;
    let mut roots = known_roots()?;
    if !roots.contains(root) {
        roots.push(root.clone());
        host::storage_set(ROOTS_KEY, serde_json::to_value(&roots)?)?;
    }
    Ok(())
}

pub fn known_roots() -> Result<Vec<String>> {
    Ok(match host::storage_get(ROOTS_KEY)? {
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    })
}

/// Removes the cached model of `service_root` and the root from the known roots.
pub fn forget(service_root: &str) -> Result<()> {
    host::storage_delete(storage_key("metadata", service_root))?;
    let mut roots = known_roots()?;
    let before = roots.len();
    roots.retain(|r| r != service_root);
    if roots.len() != before {
        host::storage_set(ROOTS_KEY, serde_json::to_value(&roots)?)?;
    }
    Ok(())
}
