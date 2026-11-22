// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins as the UI sees them (wave 5). The API types themselves are `rt_plugin::api`; the
//! shells keep a `rt_plugin::host::PluginHost` and persist installed modules (desktop and CLI
//! share one plugin directory, the web shell keeps them in browser storage).
//!
//! Owner in wave 5: strand A (install, permissions, enable). Tabs are rendered by the UI from
//! `rt_plugin::api::View` (strand B). Changes here are interface changes.

use serde::{Deserialize, Serialize};

pub use rt_plugin::api::{
    ActionButton, Block, Contributions, FieldKind, FormField, ImporterContribution, Manifest,
    Permissions, TabContribution, TabPlace, TabTarget, TreeItem, View, ViewReply,
};

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub manifest: Manifest,
    /// The user granted `manifest.permissions` at install time (all or nothing).
    pub granted: bool,
    pub enabled: bool,
}
