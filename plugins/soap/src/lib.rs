// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! SOAP and WSDL (wave 19, strand T). Strand T: WSDL import, envelope editor and response view.
//!
//! Interface commit: the crate builds into a plugin that contributes nothing yet; the strand
//! fills in the manifest (`plugin.json`) and the hooks. Domain knowledge lives here, never in
//! the core (CLAUDE.md, product principle 6).

use rt_plugin_sdk::Plugin;

/// The manifest, embedded into the `rt-plugin` custom section by [`rt_plugin_sdk::plugin!`].
pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct Soap;

rt_plugin_sdk::plugin!(Soap, manifest = MANIFEST);

impl Plugin for Soap {}

#[cfg(test)]
mod tests {
    #[test]
    fn the_manifest_is_json_with_the_plugin_id() {
        let value: serde_json::Value = serde_json::from_slice(super::MANIFEST).unwrap();
        assert_eq!(value["api"], 1);
        assert!(value["id"].is_string());
    }
}
