// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts in `rt send` (wave 7): the sandbox (`rt_script::Sandbox`) runs `before-request`,
//! `after-response` and `tests` scripts. The CLI asks no trust question (the user started it
//! on purpose, e.g. in a pipeline); `--no-scripts` switches scripts off.

use rt_core::services::ScriptRunner;
use rt_script::Sandbox;

use crate::SendArgs;

pub struct CliScripts {
    sandbox: Option<Sandbox>,
}

impl CliScripts {
    pub fn new(args: &SendArgs) -> Self {
        Self {
            sandbox: (!args.no_scripts).then(Sandbox::default),
        }
    }

    pub fn runner(&self) -> Option<&dyn ScriptRunner> {
        self.sandbox.as_ref().map(|s| s as &dyn ScriptRunner)
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
