// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.trace` of `opencollection.yml` (wave 19): whether `send_with`
//! puts a W3C `traceparent` on requests that have none. Only reading lives here, like
//! `observe_config`: the file is the team's decision, the app never writes it, and every
//! writer leaves the key byte for byte as it is. Off unless the file says exactly
//! `inject: true`.

use rt_core::model::TraceCollectionConfig;

use crate::yaml::Node;

pub(crate) fn read(root: &Node) -> TraceCollectionConfig {
    let inject = root
        .get("extensions")
        .and_then(|e| e.get("requesttailor"))
        .and_then(|r| r.get("trace"))
        .and_then(|a| a.get("inject"))
        .and_then(Node::as_bool);
    TraceCollectionConfig {
        inject: inject == Some(true),
    }
}
