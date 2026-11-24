// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt verify-consumers <provider-collection> --base-url <url> [--consumer N]... [--out
//! text|json|github] [--fetch] [--fail-fast] [--trust-scripts] [--allow-writes]` (wave 19,
//! strand F): runs the consumer collections listed in the provider's `opencollection.yml`
//! (`extensions.requesttailor.consumers`) against the built provider at `--base-url` and
//! names the consumer and request that breaks. Exit `EXIT_CONSUMERS` (8) when one breaks.
//! Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

/// Exit code of `rt verify-consumers` when at least one consumer breaks (wave 19).
pub const EXIT_CONSUMERS: i32 = 8;

pub const USAGE: &str = "  rt verify-consumers <provider-collection> --base-url <url> [--consumer <name>]...\n            [--out text|json|github] [--fetch] [--fail-fast] [--trust-scripts] [--allow-writes]\n";

pub const HELP: &str = "rt verify-consumers runs the consumer collections your provider lists\n(extensions.requesttailor.consumers in opencollection.yml: a local path or a Git URL, an\nenvironment, variables) against the provider you built, at --base-url, and reports which\nconsumer and request breaks and how. Remote consumers are cloned shallowly (natively\nonly; a token goes in RT_GIT_TOKEN). Scripts run only with --trust-scripts; writing\nrequests only with --allow-writes and never against production. Exit code 8 when a\nconsumer breaks. No broker, no upload.\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
    /// GitHub Actions workflow commands.
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub provider: PathBuf,
    pub base_url: String,
    pub only: Vec<String>,
    pub format: Format,
    pub fetch: bool,
    pub fail_fast: bool,
    pub trust_scripts: bool,
    pub allow_writes: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut provider = None;
    let mut args = Args {
        provider: PathBuf::from("."),
        base_url: String::new(),
        only: Vec::new(),
        format: Format::Text,
        fetch: false,
        fail_fast: false,
        trust_scripts: false,
        allow_writes: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("base-url") => {
                args.base_url = parser.value().map_err(err)?.string().map_err(err)?
            }
            Long("consumer") => args
                .only
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("out") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "github" => Format::Github,
                    other => {
                        return Err(format!("unknown output {other:?} (text, json or github)"));
                    }
                };
            }
            Long("fetch") => args.fetch = true,
            Long("keep-going") => args.fail_fast = false,
            Long("fail-fast") => args.fail_fast = true,
            Long("trust-scripts") => args.trust_scripts = true,
            Long("allow-writes") => args.allow_writes = true,
            Value(v) if provider.is_none() => provider = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if let Some(p) = provider {
        args.provider = p;
    }
    if args.base_url.is_empty() {
        return Err("verify-consumers: --base-url is required".into());
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: verify-consumers is not implemented yet (wave 19, strand F)."
    );
    crate::EXIT_USAGE
}
