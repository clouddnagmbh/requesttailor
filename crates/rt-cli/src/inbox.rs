// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt inbox [--port N] [--status CODE] [--json]` (wave 18, strand E): the local webhook
//! inbox in the foreground. Loopback only, nothing is written to a file; one line per request
//! on stdout (`--json`: one `CapturedRequest` per line). Stops with Ctrl-C. Interface:
//! parsed, runs "not implemented yet" (exit 2).

use std::io::Write;

use rt_app::inbox::{DEFAULT_PORT, InboxSettings};

pub const USAGE: &str = "  rt inbox [--port <PORT>] [--status <CODE>] [--json]\n";

pub const HELP: &str = "rt inbox listens on 127.0.0.1 (default port 47855) and prints every request it gets,\nanswering with a fixed reply (default 200, no body). It keeps requests in memory only\nand has no tunnel: to receive calls from the internet, point your own tool at this\nport. Stop with Ctrl-C.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub settings: InboxSettings,
    pub json: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        settings: InboxSettings {
            port: DEFAULT_PORT,
            ..InboxSettings::default()
        },
        json: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('p') | Long("port") => {
                args.settings.port = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("status") => {
                args.settings.status = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("json") => args.json = true,
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: inbox is not implemented yet (wave 18, strand E)."
    );
    crate::EXIT_USAGE
}
