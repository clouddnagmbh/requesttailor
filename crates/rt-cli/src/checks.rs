// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --checks` (wave 18, strand B): the passive response checks of
//! `rt_app::inspect`. One line per observation on stderr, the observations also in
//! `SendReport.checks`. Reads the response, sends nothing, blocks nothing: the exit code does
//! not change and a pipeline never breaks on an observation. Rules the collection switched off
//! (`extensions.requesttailor.checks.ignore` in `opencollection.yml`) stay quiet. [`attach`] is
//! the hook `rt send` calls on the finished report.
//!
//! Without `--checks` the report has no `checks` and stderr has no lines; MCP reports never
//! carry them (`rt mcp` does not call this).

use std::io::Write;

use rt_core::inspect::{Observation, Severity};
use rt_core::send::SendReport;
use rt_format::{COLLECTION_FILE, FileSet};

use crate::SendArgs;

/// Called by `rt send` on the finished report (not for sessions). With `--checks` it fills
/// `report.checks` and prints the observations to `stderr`. `Err(message)`: usage error.
pub fn attach(
    args: &SendArgs,
    report: &mut SendReport,
    stderr: &mut dyn Write,
) -> Result<(), String> {
    if !args.checks {
        return Ok(());
    }
    let mut ignore = collection_ignore(args);
    // Wave 19: the device's security settings (the file the desktop app writes) switch the
    // passive security rules off as a group or one by one.
    let data = rt_plugin::host::dir::data_dir();
    ignore.extend(rt_app::security::ignored_by(
        &rt_app::security::read_settings(data.as_deref()),
    ));
    report.checks = rt_app::inspect::check_report(report, &ignore);
    for observation in &report.checks {
        let _ = writeln!(stderr, "{}", line(observation));
    }
    Ok(())
}

/// One line: `check warn chk/hsts-missing (response headers): why Fix: how`.
pub fn line(o: &Observation) -> String {
    let severity = match o.severity {
        Severity::Info => "info",
        Severity::Warn => "warn",
    };
    format!(
        "check {severity} {} ({}): {} Fix: {}",
        o.id, o.location, o.why, o.fix
    )
}

/// The rule ids the request's collection switches off; nothing if the file cannot be read.
fn collection_ignore(args: &SendArgs) -> Vec<String> {
    let Ok((root, _)) = crate::find_root(&args.file) else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(root.join(COLLECTION_FILE)) else {
        return Vec::new();
    };
    let files = FileSet {
        files: [(COLLECTION_FILE.to_owned(), bytes)].into(),
    };
    rt_format::checks_config::configured(&files).ignore
}
