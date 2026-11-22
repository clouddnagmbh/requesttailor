// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt changelog <rev>..<rev>`: the API changelog (`rt_app::contract`, `rt_format::contract::changelog`) of the spec and the collection between two revisions.
//!
//! Wave 12 interface placeholder (strand F): the command exists and refuses until the strand
//! implements it. Its usage line and help section live here, so the strand changes only this
//! file.

use std::ffi::OsString;
use std::io::Write;

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt changelog <rev>..<rev> [--out <FILE>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt changelog writes an API changelog in Markdown for a range: breaking and\nother changes of the OpenAPI spec, and the changes of the collection. (Wave 12.)\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt changelog`, until the strand parses them.
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
    let _ = writeln!(stderr, "rt changelog: arrives in wave 12");
    crate::EXIT_USAGE
}
