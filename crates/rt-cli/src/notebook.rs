// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt notebook run <file.notebook.md> [-e <env>] [--var NAME=VALUE]... [--block <id>]
//! [--allow-writes] [--json]` (wave 19, strand W): runs the request blocks of a notebook
//! (`rt_app::notebook`) and prints what each block produced. Exit `EXIT_TESTS` (4) when a
//! request failed. Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str = "  rt notebook run <file.notebook.md> [-e <env>] [--var <NAME=VALUE>]... [--block <id>]\n            [--allow-writes] [--json]\n";

pub const HELP: &str = "rt notebook run runs the request blocks of an API notebook (a .notebook.md file next to\nyour requests), passing variables from block to block, and prints each response. Reading\nrequests run as they are; writing ones only with --allow-writes and never against\nproduction. Outputs are not saved.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub block: Option<String>,
    pub allow_writes: bool,
    pub json: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    match parser.next().map_err(err)? {
        Some(Short('h') | Long("help")) => return Ok(None),
        Some(Value(v)) if v == "run" => {}
        _ => return Err("notebook: expected `rt notebook run <file.notebook.md>`".into()),
    }
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        environment: None,
        vars: Vec::new(),
        block: None,
        allow_writes: false,
        json: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or("--var expects NAME=VALUE")?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("block") => args.block = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("allow-writes") => args.allow_writes = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("notebook run: missing <file.notebook.md>")?;
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: notebook is not implemented yet (wave 19, strand W)."
    );
    crate::EXIT_USAGE
}
