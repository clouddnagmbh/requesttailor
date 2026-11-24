// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What the strand modules add to the manifest (wave 18). The manifest itself is
//! `plugin.json`, embedded as bytes by `rt_plugin_sdk::plugin!`; this file gathers the
//! `contributions()` of the modules that own a part of it, and `tests/manifest.rs` fails when
//! `plugin.json` lacks anything they list, so a strand changes its own module and the
//! manifest in step.

use rt_plugin_sdk::api::Contributions;

/// The tabs, importers and paging rules the modules of wave 18 contribute, in a fixed order.
pub fn wave18() -> Contributions {
    let mut all = Contributions::default();
    for part in [
        crate::gateway::contributions(),
        crate::destinations::contributions(),
        crate::paging::contributions(),
    ] {
        all.tabs.extend(part.tabs);
        all.importers.extend(part.importers);
        all.paging.extend(part.paging);
    }
    all
}

/// What the modules of wave 19 contribute (the clean-up rules), in a fixed order.
pub fn wave19() -> Contributions {
    let mut all = Contributions::default();
    // One module so far; strands add theirs here, in a fixed order.
    all.compensations
        .extend(crate::compensations::contributions().compensations);
    all
}
