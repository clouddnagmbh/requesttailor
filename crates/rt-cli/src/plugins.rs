// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins for `rt send` (wave 5, strand A): the installed plugins (the directory the desktop
//! app installs into) plus `--plugin` files, run by `rt_plugin::host::PluginHost`.
//!
//! Interface commit: no plugins. Strand A owns this file.

use std::path::Path;

use rt_core::services::RequestHooks;

use crate::SendArgs;

pub struct CliPlugins {
    _private: (),
}

impl CliPlugins {
    /// `None` when there is nothing to run.
    pub fn new(args: &SendArgs, root: &Path) -> Result<Option<Self>, String> {
        let _ = root;
        if args.plugins.is_empty() {
            return Ok(None);
        }
        Err("plugins are not available yet".to_owned())
    }

    pub fn hooks(&self) -> &dyn RequestHooks {
        unreachable!("interface commit: never constructed")
    }
}
