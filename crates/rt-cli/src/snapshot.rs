// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt snapshot update [<path>...] [--all] [-e <NAME>] [--var <NAME=VALUE>]...` (wave 17,
//! strand B): writes the snapshot files (`rt_app::snapshots`) of requests anew, from a fresh
//! response. Only on this explicit command: `rt run` and `rt send` compare, they never write.
//!
//! The command is registered with the interface (arguments parse, `--help` lists it); the
//! work arrives with strand B, until then it answers with a usage error.

use std::io::Write;
use std::path::PathBuf;

use crate::EXIT_USAGE;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotArgs {
    /// Request files or folders; empty: the collection of the current folder.
    pub paths: Vec<PathBuf>,
    /// `--all`: every request that has a snapshot assertion.
    pub all: bool,
    pub environment: Option<String>,
    /// `--var NAME=VALUE`, repeatable.
    pub vars: Vec<(String, String)>,
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<SnapshotArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = SnapshotArgs::default();
    let mut action = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("all") => args.all = true,
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("var") => {
                let v = parser.value().map_err(err)?.string().map_err(err)?;
                let (k, v) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--var {v}: expected NAME=VALUE"))?;
                args.vars.push((k.to_owned(), v.to_owned()));
            }
            Value(v) if action.is_none() => action = Some(v.string().map_err(err)?),
            Value(v) => args.paths.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    match action.as_deref() {
        Some("update") => Ok(Some(args)),
        Some(other) => Err(format!("unknown snapshot command `{other}` (try `update`)")),
        None => Err("rt snapshot needs a command: `update`".into()),
    }
}

pub fn run(_args: SnapshotArgs, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(
        stderr,
        "rt: snapshot update is not implemented yet (wave 17, strand B)"
    );
    EXIT_USAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<SnapshotArgs>, String> {
        parse(&mut lexopt::Parser::from_args(args.iter().copied()))
    }

    #[test]
    fn update_takes_paths_and_options() {
        let a = parse_args(&["update", "users/get.yml", "--all", "-e", "Local"])
            .unwrap()
            .unwrap();
        assert_eq!(a.paths, vec![PathBuf::from("users/get.yml")]);
        assert!(a.all);
        assert_eq!(a.environment.as_deref(), Some("Local"));
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["freeze"]).is_err());
    }
}
