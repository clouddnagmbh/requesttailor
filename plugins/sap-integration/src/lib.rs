// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! SAP Integration Suite (wave 19, strand U), built only on the public plugin API
//! (`crates/rt-plugin/API.md`). The core knows nothing about it; what this plugin misses in the
//! API is listed in `API-WUENSCHE.md`.
//!
//! - [`collection`]: the `service-key` importer: BTP service keys (API plan and runtime plan)
//!   into a collection and an environment. The client secrets stay out of every file.
//! - [`requests`]: the requests it writes: trigger an iFlow (keeps the id of the message
//!   processing log) and read the log (status as an assertion).
//! - [`tabs`]: request tab `integration` and response tab `integration-log`.
//! - Hook `response`: notes in plain words ([`notes`]).
//!
//! The plugin asks for no rights: it sends nothing itself. Every call goes through the app's
//! own send, with its auth, guard and masking.

pub mod collection;
pub mod mpl;
pub mod notes;
pub mod requests;
pub mod servicekey;
pub mod tabs;
pub mod yaml;

use rt_plugin_sdk::api::{
    ImportReply, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse, ViewReply,
};
use rt_plugin_sdk::{Action, Error, Plugin, Result, Tab};

/// The manifest, embedded into the `rt-plugin` custom section by [`rt_plugin_sdk::plugin!`].
pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct Integration;

// Library users (feature `plugin` off) get the parsers without the plugin's exports.
#[cfg(feature = "plugin")]
rt_plugin_sdk::plugin!(Integration, manifest = MANIFEST);

impl Plugin for Integration {
    fn response(
        &self,
        request: PluginRequest,
        response: PluginResponse,
        _context: PluginContext,
    ) -> Result<NotesReply> {
        Ok(notes::respond(&request, &response))
    }

    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        tabs::render(tab)
    }

    fn action(&self, action: Action) -> Result<ViewReply> {
        tabs::action(action)
    }

    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        match importer {
            "service-key" => collection::detect(&files),
            other => Err(Error::new(format!("unknown importer {other}"))),
        }
    }

    fn import(&self, importer: &str, files: Vec<PluginFile>) -> Result<ImportReply> {
        match importer {
            "service-key" => collection::import(&files),
            other => Err(Error::new(format!("unknown importer {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_manifest_is_json_with_the_plugin_id() {
        let value: serde_json::Value = serde_json::from_slice(super::MANIFEST).unwrap();
        assert_eq!(value["api"], 1);
        assert_eq!(value["id"], "sap-integration");
    }
}
