// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt diff [<rev>..<rev> | <rev>] [--json]`: the semantic diff (`rt_app::diffs`) between revisions of the collection in the current folder, read with `rt-git`.
//!
//! Wave 12 interface placeholder (strand A): the command exists and refuses until the strand
//! implements it. Its usage line and help section live here, so the strand changes only this
//! file.

use std::ffi::OsString;
use std::io::Write;

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt diff [<rev>..<rev> | <rev>] [--json]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt diff shows what changed per request field between two revisions (default:\nthe saved files against HEAD; <rev>: that commit against its parent), as text\nor --json (CollectionDiff). Secret values never appear. (Wave 12.)\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt diff`, until the strand parses them.
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
    let _ = writeln!(stderr, "rt diff: arrives in wave 12");
    crate::EXIT_USAGE
}
