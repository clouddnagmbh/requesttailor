// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt bisect --good <rev> --bad <rev> [<folder>]`: bisect with the runner (`rt_app::bisect`), each candidate read with `rt-git` without a checkout.
//!
//! Wave 12 interface placeholder (strand C): the command exists and refuses until the strand
//! implements it. Its usage line and help section live here, so the strand changes only this
//! file.

use std::ffi::OsString;
use std::io::Write;

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt bisect --good <rev> --bad <rev> [<folder>] [-e <NAME>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt bisect finds the first commit between --good and --bad at which a test,\nan assertion or a request of the collection (or <folder>) fails, running each\ncandidate from the Git tree without a checkout. (Wave 12.)\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt bisect`, until the strand parses them.
    pub raw: Vec<OsString>,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    let raw: Vec<OsString> = parser.raw_args().map_err(|e| e.to_string())?.collect();
    if raw.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(None);
    }
    Ok(Some(Args { raw }))
}

pub fn run(_args: Args, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(stderr, "rt bisect: arrives in wave 12");
    crate::EXIT_USAGE
}
