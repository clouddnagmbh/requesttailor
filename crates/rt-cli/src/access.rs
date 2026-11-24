// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt access check <request> --as <env-a> --as <env-b> [--allow-writes] [--b-must-be-denied]
//! [--json]` (wave 19, strand D): the two-user access check of `rt_app::access`. A profile is
//! an environment. Exit `EXIT_TESTS` (4) when a finding is `broken-object-access` or
//! `broken-function-access`. Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str = "  rt access check <request-file> --as <env-a> --as <env-b> [--allow-writes]\n            [--b-must-be-denied] [--json]\n";

pub const HELP: &str = "rt access check sends one request as user A (an environment) and, if the answer holds\nan object, the same access as user B, and reports broken object or function access with\nmasked evidence. Reading requests run as they are; a writing request only with\n--allow-writes and never against a production environment. It uses only ids user A saw.\nExit code 4 when access is broken. Use it only with accounts you own.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    /// The two environments, A first.
    pub profiles: Vec<String>,
    pub allow_writes: bool,
    pub b_must_be_denied: bool,
    pub json: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    match parser.next().map_err(err)? {
        Some(Short('h') | Long("help")) => return Ok(None),
        Some(Value(v)) if v == "check" => {}
        _ => {
            return Err(
                "access: expected `rt access check <request-file> --as <A> --as <B>`".into(),
            );
        }
    }
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        profiles: Vec::new(),
        allow_writes: false,
        b_must_be_denied: false,
        json: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("as") => args
                .profiles
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("allow-writes") => args.allow_writes = true,
            Long("b-must-be-denied") => args.b_must_be_denied = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("access check: missing <request-file>")?;
    if args.profiles.len() != 2 {
        return Err("access check: give exactly two environments with --as".into());
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: access check is not implemented yet (wave 19, strand D)."
    );
    crate::EXIT_USAGE
}
