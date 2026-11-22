// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt changelog <rev>..<rev> [<collection>] [--out <FILE>]`: the API changelog
//! (`rt_app::contract::changelog`) of the OpenAPI spec and the collection between two
//! revisions, read from Git without a checkout (`rt-git`), written as Markdown to stdout.
//!
//! Wave 12, strand F. A distribution appends its own sections
//! ([`crate::Distribution::changelog`], since wave 13). The spec is the one `opencollection.yml` names
//! (`extensions.requesttailor.contract.spec`), else `openapi.yaml`, `openapi.yml` or
//! `openapi.json` next to the collection or at the repository root, on each side of the range.
//! Exit codes: 0 written, 2 usage, a revision that does not exist or a spec that cannot be
//! read.

use std::io::Write;
use std::path::PathBuf;

use rt_app::AppError;
use rt_app::contract::{ChangelogSection, Side, changelog_with};
use rt_git::{Options, Repo};

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt changelog <rev>..<rev> [<collection>] [--out <FILE>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt changelog writes an API changelog in Markdown for a range of Git revisions:\nthe breaking and other changes of the OpenAPI spec (with the requests they\naffect) and the changes of the collection. The spec is named in opencollection.yml\n(extensions.requesttailor.contract.spec) or is openapi.yaml/.yml/.json next to\nthe collection or at the repository root. <collection> defaults to the current\nfolder; --out writes to a file instead of stdout.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub from: String,
    pub to: String,
    pub collection: PathBuf,
    pub out: Option<PathBuf>,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut range: Option<String> = None;
    let mut collection: Option<PathBuf> = None;
    let mut out = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("out") | Short('o') => out = Some(PathBuf::from(parser.value().map_err(err)?)),
            Value(v) if range.is_none() => range = Some(v.to_string_lossy().into_owned()),
            Value(v) if collection.is_none() => collection = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    let range = range.ok_or("rt changelog needs a range: <rev>..<rev>")?;
    let (from, to) = match range.split_once("..") {
        Some((a, b)) if !a.is_empty() && !b.is_empty() && !b.starts_with('.') => (a, b),
        _ => return Err(format!("{range:?} is not a range: use <rev>..<rev>")),
    };
    Ok(Some(Args {
        from: from.to_owned(),
        to: to.to_owned(),
        collection: collection.unwrap_or_else(|| PathBuf::from(".")),
        out,
    }))
}

fn render(args: &Args, sections: &[&dyn ChangelogSection]) -> Result<String, AppError> {
    let repo = Repo::require(&args.collection, &Options::default())?;
    // Resolve both once, so a typo fails before anything is read.
    let old = repo.tree_at(&args.from)?;
    let new = repo.tree_at(&args.to)?;
    let (old_files, new_files) = (old.to_file_set(), new.to_file_set());
    let read_old = |path: &str| -> Result<Option<Vec<u8>>, AppError> {
        Ok(repo.show(&args.from, path)?.map(String::into_bytes))
    };
    let read_new = |path: &str| -> Result<Option<Vec<u8>>, AppError> {
        Ok(repo.show(&args.to, path)?.map(String::into_bytes))
    };
    changelog_with(
        &Side {
            files: &old_files,
            read: &read_old,
        },
        &Side {
            files: &new_files,
            read: &read_new,
        },
        &args.from,
        &args.to,
        sections,
    )
}

/// `sections`: what a distribution appends ([`crate::Distribution::changelog`]).
pub fn run(
    args: Args,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    sections: &[&dyn ChangelogSection],
) -> i32 {
    let text = match render(&args, sections) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(stderr, "rt changelog: {e}");
            return crate::EXIT_USAGE;
        }
    };
    match &args.out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, text) {
                let _ = writeln!(stderr, "rt changelog: cannot write {}: {e}", path.display());
                return crate::EXIT_USAGE;
            }
        }
        None => {
            let _ = stdout.write_all(text.as_bytes());
        }
    }
    crate::EXIT_OK
}
