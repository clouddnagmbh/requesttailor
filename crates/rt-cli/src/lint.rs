// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt lint [<folder>] [--format text|json|github] [--min-score N] [--fail-on warn|error]
//! [--rule <id>=<level>]...` (wave 18, strand F): the collection lint of `rt_app::lint` for
//! pipelines. Exit `EXIT_LINT` (7) when a finding reaches `--fail-on` or the score is
//! below `--min-score`. Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

use rt_app::lint::LintLevel;

/// Exit code of `rt lint` when it found what it was told to fail on (wave 18).
pub const EXIT_LINT: i32 = 7;

pub const USAGE: &str = "  rt lint [<folder>] [--format text|json|github] [--min-score <N>]\n          [--fail-on warn|error] [--rule <id>=<level>]...\n";

pub const HELP: &str = "rt lint checks a collection for requests without tests, hard-coded hosts, unused\nvariables and duplicate names and prints a health score from 0 to 100. Exit code 7\nwhen a finding reaches --fail-on or the score is below --min-score. Settings live in\nopencollection.yml (extensions.requesttailor.lint); --rule id=level (off, info, warn,\nerror) overrides them. It reads files and sends nothing.\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
    /// GitHub Actions workflow commands (`::warning file=…::…`).
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub folder: PathBuf,
    pub format: Format,
    pub min_score: Option<u8>,
    pub fail_on: Option<LintLevel>,
    /// `--rule id=level`, repeatable.
    pub rules: Vec<(String, LintLevel)>,
}

fn level(text: &str) -> Result<LintLevel, String> {
    match text {
        "off" => Ok(LintLevel::Off),
        "info" => Ok(LintLevel::Info),
        "warn" => Ok(LintLevel::Warn),
        "error" => Ok(LintLevel::Error),
        other => Err(format!(
            "unknown level {other:?} (off, info, warn or error)"
        )),
    }
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        folder: PathBuf::from("."),
        format: Format::Text,
        min_score: None,
        fail_on: None,
        rules: Vec::new(),
    };
    let mut folder = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("format") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "github" => Format::Github,
                    other => {
                        return Err(format!("unknown format {other:?} (text, json or github)"));
                    }
                };
            }
            Long("min-score") => {
                args.min_score = Some(parser.value().map_err(err)?.parse().map_err(err)?);
            }
            Long("fail-on") => {
                let value = parser.value().map_err(err)?.string().map_err(err)?;
                match level(&value)? {
                    l @ (LintLevel::Warn | LintLevel::Error) => args.fail_on = Some(l),
                    _ => return Err("--fail-on takes warn or error".into()),
                }
            }
            Long("rule") => {
                let value = parser.value().map_err(err)?.string().map_err(err)?;
                let (id, lvl) = value
                    .split_once('=')
                    .ok_or_else(|| format!("--rule expects ID=LEVEL, got {value:?}"))?;
                args.rules.push((id.to_owned(), level(lvl)?));
            }
            Value(v) if folder.is_none() => folder = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
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
        "rt: lint is not implemented yet (wave 18, strand F)."
    );
    crate::EXIT_USAGE
}
