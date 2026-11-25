// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts in `rt send` (wave 7): the sandbox (`rt_script::Sandbox`) runs `before-request`,
//! `after-response` and `tests` scripts. The CLI asks no trust question (the user started it
//! on purpose, e.g. in a pipeline); `--no-scripts` switches scripts off.

use std::io::{BufReader, Stderr, Stdin};

use rt_core::services::ScriptRunner;
use rt_script::Sandbox;
use rt_script::debug::DebugRunner;

use crate::SendArgs;
use crate::debug_flag::Terminal;

pub struct CliScripts {
    sandbox: Option<Sandbox>,
    /// Wave 19: `rt send --debug`: the request's scripts stop in the terminal.
    debug: Option<DebugRunner<Terminal<BufReader<Stdin>, Stderr>>>,
}

impl CliScripts {
    pub fn new(args: &SendArgs) -> Self {
        Self {
            sandbox: (!args.no_scripts).then(Sandbox::default),
            debug: (!args.no_scripts && args.debug.active()).then(|| {
                DebugRunner::new(
                    Terminal::new(BufReader::new(std::io::stdin()), std::io::stderr()),
                    args.debug.breakpoints.clone(),
                )
            }),
        }
    }

    pub fn runner(&self) -> Option<&dyn ScriptRunner> {
        match &self.debug {
            Some(debug) => Some(debug),
            None => self.sandbox.as_ref().map(|s| s as &dyn ScriptRunner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scripts_switches_the_runner_off() {
        assert!(CliScripts::new(&SendArgs::default()).runner().is_some());
        let args = SendArgs {
            no_scripts: true,
            ..SendArgs::default()
        };
        assert!(CliScripts::new(&args).runner().is_none());
    }
}
