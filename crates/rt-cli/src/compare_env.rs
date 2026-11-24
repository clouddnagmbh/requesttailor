// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt compare-env <folder> --env A --env B [--env C]... [--json] [--ignore-env-values]
//! [--fail-on-diff]` (wave 18, strand A): the same reading requests in two or more
//! environments, compared as `rt_app::envcompare` does it for the app. Exit 0 also when the
//! environments differ; with `--fail-on-diff` exit 1 when they do. Interface: parsed, runs
//! "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str = "  rt compare-env [<folder>] --env <A> --env <B>... [--json] [--ignore-env-values]\n            [--fail-on-diff]\n";

pub const HELP: &str = "rt compare-env sends the reading requests of a collection or folder in two or\nmore environments (the first --env is the reference) and lists what differs: status,\nheaders and JSON body. Writing requests are skipped and named. Secret values never\nappear. Exit code 0 even with differences; --fail-on-diff exits 1 for them.\n";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// A folder of the collection (default: the current folder).
    pub folder: PathBuf,
    pub environments: Vec<String>,
    pub json: bool,
    pub ignore_env_values: bool,
    pub fail_on_diff: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        folder: PathBuf::from("."),
        ..Args::default()
    };
    let mut folder = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('e') | Long("env") => {
                args.environments
                    .push(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("json") => args.json = true,
            Long("ignore-env-values") => args.ignore_env_values = true,
            Long("fail-on-diff") => args.fail_on_diff = true,
            Value(v) if folder.is_none() => folder = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if args.environments.len() < 2 {
        return Err("compare-env needs at least two --env".into());
    }
    if let Some(f) = folder {
        args.folder = f;
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: compare-env is not implemented yet (wave 18, strand A)."
    );
    crate::EXIT_USAGE
}
