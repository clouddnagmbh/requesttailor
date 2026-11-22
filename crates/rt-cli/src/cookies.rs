// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar for `rt send --cookie-jar FILE` (wave 5, strand D).
//!
//! Interface commit: no jar. Strand D owns this file.

use rt_core::services::CookieJar;

use crate::SendArgs;

pub struct CliCookies {
    _private: (),
}

impl CliCookies {
    /// `None` without `--cookie-jar`.
    pub fn new(args: &SendArgs) -> Result<Option<Self>, String> {
        match &args.cookie_jar {
            None => Ok(None),
            Some(_) => Err("--cookie-jar is not available yet".to_owned()),
        }
    }

    pub fn jar(&self) -> &dyn CookieJar {
        unreachable!("interface commit: never constructed")
    }
}
