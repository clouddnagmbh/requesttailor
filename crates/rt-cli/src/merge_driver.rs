// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt merge-driver %O %A %B %P`: Git merge driver; merges one collection file with `rt_format::merge` and writes the result to `%A` (exit 0 clean, 1 with conflicts).
//!
//! Wave 12 interface placeholder (strand B): the command exists and refuses until the strand
//! implements it. Its usage line and help section live here, so the strand changes only this
//! file.

use std::ffi::OsString;
use std::io::Write;

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt merge-driver <base> <ours> <theirs> <path>\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt merge-driver merges collection files on the model, as Git merge driver:\n  git config merge.requesttailor.driver \"rt merge-driver %O %A %B %P\"\n  echo \"*.yml merge=requesttailor\" >> .gitattributes\nA conflict only where both sides changed the same field. (Wave 12.)\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt merge-driver`, until the strand parses them.
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
    let _ = writeln!(stderr, "rt merge-driver: arrives in wave 12");
    crate::EXIT_USAGE
}
