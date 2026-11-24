// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt plugin sources add|remove|list | search <text> | install <id|url> [--source S] [--yes
//! --sha256 X] | update [--check] | verify | remove <id> | list` (wave 18, strand I): the
//! plugin catalogue without a server (`rt_app::catalogue`). Nothing is requested before the
//! user runs one of these. `--yes` only together with `--sha256`. Interface: parsed, runs
//! "not implemented yet" (exit 2).

use std::io::Write;

pub const USAGE: &str = "  rt plugin sources add|remove|list | search <text> | install <id|url> [--source <S>]\n            [--yes --sha256 <HEX>] | update [--check] | verify | remove <id> | list\n";

pub const HELP: &str = "rt plugin installs plugins (WebAssembly only) from sources you add yourself: an HTTPS\nlink to a plugins.json or a Git repository that has one. There is no built-in index\nand nothing is requested until you run a command. Before installing, the SHA-256 of\nthe download must equal the index's, and the rights shown are read from the plugin\nitself; --yes skips the question only together with --sha256.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SourcesAdd(String),
    SourcesRemove(String),
    SourcesList,
    Search(String),
    Install {
        target: String,
        source: Option<String>,
        yes: bool,
        sha256: Option<String>,
    },
    Update {
        check: bool,
    },
    Verify,
    Remove(String),
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub action: Action,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut words: Vec<String> = Vec::new();
    let (mut source, mut yes, mut sha256, mut check) = (None, false, None, false);
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("source") => source = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("yes") => yes = true,
            Long("sha256") => sha256 = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("check") => check = true,
            Value(v) => words.push(v.string().map_err(err)?),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if yes && sha256.is_none() {
        return Err("--yes needs --sha256 <HEX>: install what you have verified".into());
    }
    let mut it = words.into_iter();
    let one = |it: &mut std::vec::IntoIter<String>, what: &str| {
        it.next().ok_or_else(|| format!("plugin: missing {what}"))
    };
    let action = match it.next().as_deref() {
        Some("sources") => match it.next().as_deref() {
            Some("add") => Action::SourcesAdd(one(&mut it, "the source URL")?),
            Some("remove") => Action::SourcesRemove(one(&mut it, "the source URL")?),
            Some("list") => Action::SourcesList,
            _ => return Err("plugin sources: expected add, remove or list".into()),
        },
        Some("search") => Action::Search(one(&mut it, "the search text")?),
        Some("install") => Action::Install {
            target: one(&mut it, "the plugin id or URL")?,
            source,
            yes,
            sha256,
        },
        Some("update") => Action::Update { check },
        Some("verify") => Action::Verify,
        Some("remove") => Action::Remove(one(&mut it, "the plugin id")?),
        Some("list") => Action::List,
        Some(other) => return Err(format!("plugin: unknown command {other:?}")),
        None => return Err("plugin: missing command".into()),
    };
    if let Some(extra) = it.next() {
        return Err(format!("plugin: unexpected argument {extra:?}"));
    }
    Ok(Some(Args { action }))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let _ = (args, stdout);
    let _ = writeln!(
        stderr,
        "rt: plugin is not implemented yet (wave 18, strand I)."
    );
    crate::EXIT_USAGE
}
