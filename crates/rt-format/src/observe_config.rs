// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.observe` of `opencollection.yml` (wave 17): whether finished
//! sends may be handed to observers (`rt_core::observe`) for the collection. Only reading
//! lives here: the file is the team's decision, the app never writes it, and every writer of
//! the file leaves the key byte for byte as it is.
//!
//! Delivery is off only if the file says exactly `enabled: false`; anything else (a missing
//! key, `true`, an odd value) leaves it on.
//!
//! OpenCollection check for wave 17: `extensions` is the one place the schema allows
//! unknown keys (`opencollection.yml`), so this key needs no schema change.

use rt_core::model::ObserveCollectionConfig;

use crate::yaml::Node;

pub(crate) fn read(root: &Node) -> ObserveCollectionConfig {
    let enabled = root
        .get("extensions")
        .and_then(|e| e.get("requesttailor"))
        .and_then(|r| r.get("observe"))
        .and_then(|a| a.get("enabled"))
        .and_then(Node::as_bool);
    ObserveCollectionConfig {
        enabled: enabled != Some(false),
    }
}
