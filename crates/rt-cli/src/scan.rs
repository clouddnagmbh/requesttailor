// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt scan [<path>...]` (wave 5, strand E): the secret scanner of `rt_app::secret_scan` for
//! pipelines and as a Git pre-commit hook. Exit code 0 when clean, 1 with findings.
//!
//! Interface commit: parses its arguments and finds nothing. Strand E owns this file.

use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanArgs {
    /// Files or folders; empty: the current folder.
    pub paths: Vec<PathBuf>,
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<ScanArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut paths = Vec::new();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Value(v) => paths.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(ScanArgs { paths }))
}

pub fn run(args: ScanArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout, stderr);
    crate::EXIT_OK
}
