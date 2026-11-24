// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt security audit [<folder>] [--env <name>] [--out text|markdown|html|json|sarif] [--fail-on
//! <severity>] [--i-am-authorized]` (wave 19, strands R and S): the API security audit of a
//! collection. The passive checks (`rt_app::security`) run over the responses of a run; a
//! distribution may add active probes, which run only with `--i-am-authorized`, never
//! against production, and only against hosts of the collection. Exit `EXIT_AUDIT` (9) when
//! a finding reaches `--fail-on`. Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

use rt_app::security::{AuditFormat, Severity};

/// Exit code of `rt security audit` when a finding reaches `--fail-on` (wave 19).
pub const EXIT_AUDIT: i32 = 9;

pub const USAGE: &str = "  rt security audit [<folder>] [-e <env>] [--out text|markdown|html|json|sarif] [--fail-on <severity>]\n            [--allow-writes] [--i-am-authorized]\n";

pub const HELP: &str = "rt security audit checks the responses of a collection run against the OWASP API Security Top\n10: open CORS, weak tokens, missing security headers, no sign of rate limiting, TLS\nproblems and revealing error pages. It sends the requests of the collection as they are\n(reading ones; writing ones only with --allow-writes) and nothing else. Exit code 9 when a\nfinding reaches --fail-on (info, low, medium, high, critical). Never run against\nproduction; only against APIs you are allowed to test.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub folder: PathBuf,
    pub environment: Option<String>,
    pub format: AuditFormat,
    pub fail_on: Option<Severity>,
    pub allow_writes: bool,
    /// The user states they may test this API (needed by active probes of a distribution).
    pub authorized: bool,
}

pub fn severity(text: &str) -> Result<Severity, String> {
    match text {
        "info" => Ok(Severity::Info),
        "low" => Ok(Severity::Low),
        "medium" => Ok(Severity::Medium),
        "high" => Ok(Severity::High),
        "critical" => Ok(Severity::Critical),
        other => Err(format!(
            "unknown severity {other:?} (info, low, medium, high or critical)"
        )),
    }
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    // `rt security <subcommand>`: only `audit` so far; later subcommands get their own arm.
    match parser.next().map_err(err)? {
        Some(Short('h') | Long("help")) => return Ok(None),
        Some(Value(v)) if v == "audit" => {}
        _ => return Err("security: expected `rt security audit [<folder>]`".into()),
    }
    let mut folder = None;
    let mut args = Args {
        folder: PathBuf::from("."),
        environment: None,
        format: AuditFormat::Text,
        fail_on: None,
        allow_writes: false,
        authorized: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Long("out") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => AuditFormat::Text,
                    "markdown" => AuditFormat::Markdown,
                    "html" => AuditFormat::Html,
                    "json" => AuditFormat::Json,
                    "sarif" => AuditFormat::Sarif,
                    other => {
                        return Err(format!(
                            "unknown output {other:?} (text, markdown, html, json or sarif)"
                        ));
                    }
                };
            }
            Long("fail-on") => {
                args.fail_on = Some(severity(
                    &parser.value().map_err(err)?.string().map_err(err)?,
                )?)
            }
            Long("allow-writes") => args.allow_writes = true,
            Long("i-am-authorized") => args.authorized = true,
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
        "rt: security audit is not implemented yet (wave 19, strand R)."
    );
    crate::EXIT_USAGE
}
