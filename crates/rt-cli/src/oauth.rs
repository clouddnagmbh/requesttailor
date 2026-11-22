// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 for `rt send` (wave 5, strand C): tokens in the OS keychain (shared with the
//! desktop app for the same collection), the system browser plus a loopback listener for
//! the authorization code flow, the device code on stderr. `--no-prompt` fails instead.
//!
//! Interface commit: no cache, no prompts. Strand C owns this file.

use std::path::Path;

use rt_core::services::{AuthPrompt, TokenStore};

use crate::SendArgs;

pub struct CliAuth {
    _private: (),
}

impl CliAuth {
    pub fn new(args: &SendArgs, root: &Path) -> Self {
        let _ = (args, root);
        Self { _private: () }
    }

    pub fn tokens(&self) -> Option<&dyn TokenStore> {
        None
    }

    pub fn prompt(&self) -> Option<&dyn AuthPrompt> {
        None
    }
}
