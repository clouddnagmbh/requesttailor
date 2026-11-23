// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.ai` of `opencollection.yml` (wave 16): whether the AI assistant
//! may be used for the collection. Only reading lives here: the file is the team's decision,
//! the app never writes it, and every writer of the file leaves the key byte for byte as it
//! is (tests below).
//!
//! The assistant is off only if the file says exactly `enabled: false`; anything else
//! (a missing key, `true`, an odd value) leaves it on.

use rt_core::model::AiCollectionConfig;

use crate::yaml::Node;

pub(crate) fn read(root: &Node) -> AiCollectionConfig {
    let enabled = root
        .get("extensions")
        .and_then(|e| e.get("requesttailor"))
        .and_then(|r| r.get("ai"))
        .and_then(|a| a.get("enabled"))
        .and_then(Node::as_bool);
    AiCollectionConfig {
        enabled: enabled != Some(false),
    }
}
