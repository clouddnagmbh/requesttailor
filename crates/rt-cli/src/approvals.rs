// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt approvals [--json]`, `rt approve <id>`, `rt deny <id>`: answer what `rt mcp` waits for (`rt_app::agents`, files in the data folder).
//!
//! Wave 8 interface placeholder (strand C): the command exists and refuses until the strand
//! implements it.

use std::ffi::OsString;
use std::io::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The raw arguments after `rt approvals`, until the strand parses them.
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
    let _ = writeln!(stderr, "rt approvals: arrives in wave 8");
    crate::EXIT_USAGE
}
