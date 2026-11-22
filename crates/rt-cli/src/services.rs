// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The services `rt send` passes to `rt_core::send::send_with` (wave 5), built from the
//! arguments. Each part lives in its strand's module; this file only puts them together.

use std::path::Path;
use std::rc::Rc;

use rt_core::send::{REPORT_VERSION, SendError, SendReport};
use rt_core::services::Services;

use crate::SendArgs;
use crate::cookies::CliCookies;
use crate::oauth::CliAuth;
use crate::plugins::CliPlugins;

/// Owns the parts; [`CliServices::services`] lends them to `send_with`.
pub struct CliServices {
    plugins: Option<CliPlugins>,
    auth: CliAuth,
    /// Shared with the plugins: `http` with `session: true` uses the same jar (wave 6).
    cookies: Option<Rc<CliCookies>>,
}

impl CliServices {
    /// `root` is the collection root. `Err` is a message for the report (e.g. a plugin that
    /// does not load).
    pub fn new(args: &SendArgs, root: &Path) -> Result<Self, String> {
        let cookies = CliCookies::new(args)?.map(Rc::new);
        Ok(Self {
            plugins: CliPlugins::new(args, root, cookies.clone())?,
            auth: CliAuth::new(args, root),
            cookies,
        })
    }

    pub fn services(&self) -> Services<'_> {
        Services {
            tokens: self.auth.tokens(),
            prompt: self.auth.prompt(),
            cookies: self.cookies.as_deref().map(CliCookies::jar),
            hooks: self.plugins.as_ref().map(CliPlugins::hooks),
        }
    }
}

/// A report for a send that failed before `send_with` could run.
pub fn failed(request_path: &str, args: &SendArgs, error: SendError) -> SendReport {
    SendReport {
        version: REPORT_VERSION,
        request_path: request_path.to_owned(),
        environment: args.environment.clone(),
        request: None,
        response: None,
        variables: Vec::new(),
        unresolved: Vec::new(),
        error: Some(error),
        notes: Vec::new(),
    }
}
