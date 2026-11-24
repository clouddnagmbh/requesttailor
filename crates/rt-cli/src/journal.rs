// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt journal [list|revert <id> --with <request>|clear] [-C <collection>] [--json]` (wave
//! 19, strand C): the test-data journal of `rt_app::journal` (device state in the data
//! folder). `revert` sends the request you name with the entry's values filled in, under
//! the rules of any writing request (guard, production needs `--yes-production`).
//! Interface: parsed, runs "not implemented yet" (exit 2).

use std::io::Write;
use std::path::PathBuf;

pub const USAGE: &str = "  rt journal [list | revert <id> --with <request-file> | clear] [-C <collection>] [--json]\n            [--yes-production]\n";

pub const HELP: &str = "rt journal lists the successful writing requests a session sent (when \"Track test data\"\nis on for the collection) and puts one back with a request you choose: revert <id> --with\n<request-file> fills {{journal.id}}, {{journal.location}} and {{journal.etag}} from the\nentry and sends it, as any writing request: guard, audit log, and for production only with\n--yes-production. The journal keeps no bodies and no header values.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    List,
    Revert { id: u64, with: PathBuf },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub action: Action,
    pub collection: PathBuf,
    pub json: bool,
    pub yes_production: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        action: Action::List,
        collection: PathBuf::from("."),
        json: false,
        yes_production: false,
    };
    let mut word: Option<String> = None;
    let mut id: Option<u64> = None;
    let mut with: Option<PathBuf> = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('C') | Long("collection") => {
                args.collection = parser.value().map_err(err)?.into()
            }
            Long("json") => args.json = true,
            Long("yes-production") => args.yes_production = true,
            Long("with") => with = Some(parser.value().map_err(err)?.into()),
            Value(v) if word.is_none() => word = Some(v.string().map_err(err)?),
            Value(v) if id.is_none() => {
                id = Some(
                    v.string()
                        .map_err(err)?
                        .parse()
                        .map_err(|_| "journal: the entry id is a number".to_string())?,
                )
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.action = match word.as_deref() {
        None | Some("list") => Action::List,
        Some("clear") => Action::Clear,
        Some("revert") => Action::Revert {
            id: id.ok_or("journal revert: missing <id>")?,
            with: with.ok_or("journal revert: missing --with <request-file>")?,
        },
        Some(other) => {
            return Err(format!(
                "journal: unknown action {other:?} (list, revert, clear)"
            ));
        }
    };
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: journal is not implemented yet (wave 19, strand C)."
    );
    crate::EXIT_USAGE
}
