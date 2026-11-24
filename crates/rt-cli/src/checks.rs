// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --checks` (wave 18, strand B): the passive response checks of
//! `rt_app::inspect`. One line per observation on stderr, the observations also in
//! `SendReport.checks`. Reads the response, sends nothing. [`attach`] is the hook `rt send`
//! calls on the finished report. Interface: refuses "not implemented yet".

use rt_core::send::SendReport;

use crate::SendArgs;

/// Called by `rt send` on the finished report (not for sessions). Fills `report.checks` and
/// prints the observations to stderr when `--checks` is on. `Err(message)`: usage error.
pub fn attach(args: &SendArgs, report: &mut SendReport) -> Result<(), String> {
    let _ = report;
    if args.checks {
        return Err("--checks is not implemented yet (wave 18, strand B).".into());
    }
    Ok(())
}
