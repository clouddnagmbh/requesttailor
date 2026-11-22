// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt run` (wave 7, strand E owns this file): runs a collection or folder with
//! `rt_core::runner`, for pipelines. Reporters: live text on stderr, `--reporter json` (the
//! run's steps and summary) and `--reporter junit` (one `<testsuite>` per request, one
//! `<testcase>` per test and assertion), written to `--out <FILE>` or stdout.
//!
//! ```text
//! rt run [<folder-or-collection>] [-e NAME] [--var NAME=VALUE]... [--data FILE.csv|json]
//!        [--iterations N] [--delay MS] [--bail] [--reporter text|json|junit]... [--out FILE]
//!        [--no-scripts] [--plugin FILE.wasm]... [--no-plugins] [--no-prompt]
//! ```
//!
//! Exit codes: `EXIT_OK` all passed, `EXIT_TESTS` a request, test or assertion failed,
//! `EXIT_USAGE` usage error. Interface stub: parses nothing yet.

use std::io::Write;
use std::path::PathBuf;

use crate::EXIT_USAGE;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunArgs {
    /// A folder inside a collection or the collection root; default: the current folder.
    pub path: Option<PathBuf>,
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<RunArgs>, String> {
    let _ = parser;
    Err("rt run is not available yet".into())
}

pub fn run(args: RunArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(stderr, "rt: rt run is not available yet");
    EXIT_USAGE
}
