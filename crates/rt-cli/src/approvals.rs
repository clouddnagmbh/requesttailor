// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt approvals [--json]`, `rt approve <id>`, `rt deny <id>`: answer what `rt mcp` waits for (`rt_app::agents`, files in the data folder).
//!
//! Wave 8, strand C. The data folder is the desktop app's (`RT_DATA_DIR` replaces it), so
//! the app's banner, these commands and every running `rt mcp` see the same approvals.

use std::ffi::OsString;
use std::io::Write;

use rt_app::agents::{self, ApprovalDecision, PendingApproval};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    pub help: bool,
    /// `--json` (`rt approvals`).
    pub json: bool,
    /// The approval id (`rt approve`, `rt deny`).
    pub id: Option<String>,
}

pub fn help() -> String {
    "\
rt approvals: writing requests of coding agents (rt mcp) waiting for approval

Usage:
  rt approvals [--json]     List what waits (oldest first); --json prints the
                            PendingApproval list
  rt approve <id>           Let the request go out
  rt deny <id>              Refuse it; nothing is sent

The desktop app shows the same requests in a banner. An approval that nobody
answers fails after rt mcp's --approval-timeout, and nothing is sent.
"
    .to_owned()
}

/// Never `Ok(None)`: `--help` is [`Args::help`].
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => args.help = true,
            Long("json") => args.json = true,
            Value(v) if args.id.is_none() => {
                args.id = Some(v.into_string().map_err(|v: OsString| {
                    format!("not an approval id: {}", v.to_string_lossy())
                })?);
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

fn data_dir(stderr: &mut dyn Write) -> Option<std::path::PathBuf> {
    let dir = rt_plugin::host::dir::data_dir();
    if dir.is_none() {
        let _ = writeln!(stderr, "rt: no data folder (set RT_DATA_DIR)");
    }
    dir
}

fn line(p: &PendingApproval) -> String {
    format!(
        "{}  {} {}  ({}, {}, {})\n    {}: {}\n",
        p.id,
        p.method,
        p.url,
        p.environment.as_deref().unwrap_or("no environment"),
        p.collection_name,
        p.agent,
        p.request_path,
        p.reason,
    )
}

/// `rt approvals`.
pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if args.help {
        let _ = stdout.write_all(help().as_bytes());
        return crate::EXIT_OK;
    }
    if let Some(id) = &args.id {
        let _ = writeln!(
            stderr,
            "rt: unexpected argument {id:?}; use rt approve <id> or rt deny <id>"
        );
        return crate::EXIT_USAGE;
    }
    let Some(dir) = data_dir(stderr) else {
        return crate::EXIT_USAGE;
    };
    let list = match agents::pending(&dir) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(stderr, "rt: cannot read approvals: {e}");
            return crate::EXIT_USAGE;
        }
    };
    if args.json {
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string(&list).expect("approvals serialize")
        );
    } else if list.is_empty() {
        let _ = writeln!(stdout, "Nothing waits for approval.");
    } else {
        for p in &list {
            let _ = stdout.write_all(line(p).as_bytes());
        }
    }
    crate::EXIT_OK
}

/// `rt approve <id>` and `rt deny <id>`.
pub fn decide(
    args: Args,
    decision: ApprovalDecision,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    if args.help {
        let _ = stdout.write_all(help().as_bytes());
        return crate::EXIT_OK;
    }
    if args.json {
        let _ = writeln!(stderr, "rt: --json is only for rt approvals");
        return crate::EXIT_USAGE;
    }
    let Some(id) = args.id else {
        let _ = writeln!(stderr, "rt: missing <id>; see rt approvals");
        return crate::EXIT_USAGE;
    };
    let Some(dir) = data_dir(stderr) else {
        return crate::EXIT_USAGE;
    };
    match agents::decide_as(&dir, &id, decision, "cli") {
        Ok(()) => {
            let word = match decision {
                ApprovalDecision::Approved => "Approved",
                ApprovalDecision::Denied => "Denied",
            };
            let _ = writeln!(stdout, "{word} {id}.");
            crate::EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(stderr, "rt: {e}");
            crate::EXIT_USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Args, String> {
        let mut p = lexopt::Parser::from_args(args.iter().copied());
        parse(&mut p).map(Option::unwrap)
    }

    #[test]
    fn parses() {
        assert_eq!(
            parse_args(&["--json"]).unwrap(),
            Args {
                json: true,
                ..Args::default()
            }
        );
        assert_eq!(parse_args(&["abc"]).unwrap().id.as_deref(), Some("abc"));
        assert!(parse_args(&["--help"]).unwrap().help);
        assert!(parse_args(&["a", "b"]).is_err());
        assert!(parse_args(&["--nope"]).is_err());
    }

    #[test]
    fn decide_needs_an_id() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            decide(
                Args::default(),
                ApprovalDecision::Approved,
                &mut out,
                &mut err
            ),
            crate::EXIT_USAGE
        );
        assert!(String::from_utf8(err).unwrap().contains("missing <id>"));
    }
}
