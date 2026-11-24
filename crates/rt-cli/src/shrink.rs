// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt shrink <request> [--env E] [--predicate P] [--keep PART]... [--max-probes N]
//! [--max-seconds N] [--simplify-values] [--allow-writes] [--json]` (wave 19, strand E): cut
//! a failing request down to the smallest one that still fails (`rt_app::shrink`). Exit 0
//! when a minimal request was found, 1 when the failure did not reproduce, `EXIT_BLOCKED`
//! (5) when the guard refused. Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

use rt_app::shrink::Predicate;

pub const USAGE: &str = "  rt shrink <request-file> [-e <env>] [--predicate <P>] [--keep <part>]...\n            [--max-probes <N>] [--max-seconds <N>] [--simplify-values] [--allow-writes] [--json]\n";

pub const HELP: &str = "rt shrink sends variations of a failing request and prints the smallest one that still\nfails: fewer headers, query parameters, cookies and body fields. --predicate is\nstatus=500, 5xx, assertion[=NAME], body~PATTERN, header=NAME, no-header=NAME or last (the\ndefault: the failure of the last response). --keep header:Authorization keeps a part.\nAt most 100 probes (hard 500) and 120 s (hard 600). A writing request only runs with\n--allow-writes and never against a production environment. Exit 0 found, 1 not\nreproducible, 5 blocked by the guard.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub predicate: Predicate,
    pub keep: Vec<String>,
    pub max_probes: u32,
    pub max_seconds: u32,
    pub simplify_values: bool,
    pub allow_writes: bool,
    pub json: bool,
}

/// `status=500`, `5xx`, `assertion`, `assertion=NAME`, `body~PATTERN`, `header=NAME`,
/// `no-header=NAME`, `last`.
pub fn parse_predicate(text: &str) -> Result<Predicate, String> {
    let bad = || {
        format!(
            "unknown predicate {text:?} (status=N, 5xx, assertion[=NAME], body~PATTERN, header=NAME, no-header=NAME, last)"
        )
    };
    if text == "last" {
        return Ok(Predicate::LastFailure);
    }
    if text == "5xx" {
        return Ok(Predicate::ServerError);
    }
    if text == "assertion" {
        return Ok(Predicate::AssertionFails { assertion: None });
    }
    if let Some(name) = text.strip_prefix("assertion=") {
        return Ok(Predicate::AssertionFails {
            assertion: Some(name.to_owned()),
        });
    }
    if let Some(code) = text.strip_prefix("status=") {
        return Ok(Predicate::Status {
            status: code.parse().map_err(|_| bad())?,
        });
    }
    if let Some(pattern) = text.strip_prefix("body~") {
        return Ok(Predicate::BodyMatches {
            pattern: pattern.to_owned(),
        });
    }
    if let Some(name) = text.strip_prefix("header=") {
        return Ok(Predicate::HeaderPresent {
            name: name.to_owned(),
        });
    }
    if let Some(name) = text.strip_prefix("no-header=") {
        return Ok(Predicate::HeaderMissing {
            name: name.to_owned(),
        });
    }
    Err(bad())
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        environment: None,
        vars: Vec::new(),
        predicate: Predicate::LastFailure,
        keep: Vec::new(),
        max_probes: 0,
        max_seconds: 0,
        simplify_values: false,
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
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("predicate") => {
                args.predicate =
                    parse_predicate(&parser.value().map_err(err)?.string().map_err(err)?)?
            }
            Long("keep") => args
                .keep
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("max-probes") => {
                args.max_probes = parser.value().map_err(err)?.parse().map_err(err)?
            }
            Long("max-seconds") => {
                args.max_seconds = parser.value().map_err(err)?.parse().map_err(err)?
            }
            Long("simplify-values") => args.simplify_values = true,
            Long("allow-writes") => args.allow_writes = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("shrink: missing <request-file>")?;
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: shrink is not implemented yet (wave 19, strand E)."
    );
    crate::EXIT_USAGE
}
