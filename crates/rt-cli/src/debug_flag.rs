// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --debug [--break <line>]...` (wave 19, strand Y): the text mode of the script
//! debugger (`rt_script::debug`): stops the request's scripts at the breakpoints (and at
//! the first statement without any), prints the variables and reads commands (`c`, `n`,
//! `s`, `o`, `p <expr>`, `q`) from stdin. Interface: the flags refuse "not implemented yet".

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DebugArgs {
    /// `--debug`.
    pub debug: bool,
    /// `--break <line>`, repeatable.
    pub breakpoints: Vec<u32>,
}

pub const HELP: &str = "      --debug               Debug the request's scripts in the terminal (stops at --break lines)\n      --break <LINE>        A breakpoint for --debug; repeatable\n";

/// Called by `rt send` before it starts. Strand Y replaces it.
pub fn check(args: &DebugArgs) -> Result<(), String> {
    if args.debug || !args.breakpoints.is_empty() {
        return Err("--debug is not implemented yet (wave 19, strand Y).".into());
    }
    Ok(())
}
