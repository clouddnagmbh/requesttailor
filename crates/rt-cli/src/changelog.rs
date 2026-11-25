// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt changelog <rev>..<rev> [<collection>] [--out <FILE>]`: the API changelog
//! (`rt_app::contract::changelog`) of the OpenAPI spec and the collection between two
//! revisions, read from Git without a checkout (`rt-git`), written as Markdown to stdout.
//!
//! Wave 12, strand F. Since wave 18 (strand H) the changes of a GraphQL schema and of the
//! Protobuf files of `config.protobuf` follow as the sections `## GraphQL` and `## Protobuf`
//! (`rt_app::contract::SchemaSection`), before the sections of a distribution. A distribution appends its own sections
//! ([`crate::Distribution::changelog`], since wave 13). The spec is the one `opencollection.yml` names
//! (`extensions.requesttailor.contract.spec`), else `openapi.yaml`, `openapi.yml` or
//! `openapi.json` next to the collection or at the repository root, on each side of the range. A GraphQL schema is
//! `extensions.requesttailor.contract.graphql`, else `schema.graphql`, `schema.gql` or
//! `schema.graphqls` next to the collection or at the repository root.
//! Since wave 16 a distribution can offer other writers ([`crate::Distribution::
//! changelog_writers`]): `rt changelog <range> --<flag> [--<option> <value>]...` gives them
//! the facts of the range ([`ChangelogFacts`]) instead of writing the Markdown itself; their
//! help follows the help of `rt changelog`. Without a flag nothing changes.
//! Exit codes: 0 written, 2 usage, a revision that does not exist or a spec that cannot be
//! read.

use std::io::Write;
use std::path::PathBuf;

use rt_app::AppError;
use rt_app::contract::{
    ChangelogFacts, ChangelogSection, ChangelogWriter, SchemaKind, SchemaSection, Side,
    changelog_facts, changelog_with,
};
use rt_git::{Options, Repo};

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt changelog <rev>..<rev> [<collection>] [--out <FILE>]\n";

/// The lines `rt changelog --help` adds for the writers of a distribution.
pub fn writers_help(writers: &[&dyn ChangelogWriter]) -> String {
    if writers.is_empty() {
        return String::new();
    }
    let mut text = String::from("\nOther ways to write the changelog (choose one):\n");
    for w in writers {
        text.push_str(w.help());
    }
    text
}

/// The section in `rt --help`.
pub const HELP: &str = "rt changelog writes an API changelog in Markdown for a range of Git revisions:\nthe breaking and other changes of the OpenAPI spec (with the requests they\naffect) and the changes of the collection. The spec is named in opencollection.yml\n(extensions.requesttailor.contract.spec) or is openapi.yaml/.yml/.json next to\nthe collection or at the repository root. The changes of a GraphQL schema\n(extensions.requesttailor.contract.graphql, else schema.graphql next to the\ncollection or at the repository root) and of the Protobuf files of\nconfig.protobuf follow in the sections GraphQL and Protobuf, with the requests\nthey affect. <collection> defaults to the current folder; --out writes to a file\ninstead of stdout.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub from: String,
    pub to: String,
    pub collection: PathBuf,
    pub out: Option<PathBuf>,
    /// The flag of the chosen writer (`ChangelogWriter::flag`), if any.
    pub writer: Option<String>,
    /// The writer's options in the order given: `(name, value)`, names without dashes.
    pub options: Vec<(String, String)>,
}

/// `Ok(None)`: `--help`. `writers`: the flags and options a distribution adds.
pub fn parse(
    parser: &mut lexopt::Parser,
    writers: &[&dyn ChangelogWriter],
) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut range: Option<String> = None;
    let mut collection: Option<PathBuf> = None;
    let mut out = None;
    let mut writer: Option<&dyn ChangelogWriter> = None;
    let mut options: Vec<(String, String)> = Vec::new();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("out") | Short('o') => out = Some(PathBuf::from(parser.value().map_err(err)?)),
            Long(name) if writers.iter().any(|w| w.flag() == name) => {
                let chosen = writers.iter().find(|w| w.flag() == name).unwrap();
                match writer {
                    Some(first) if first.flag() != name => {
                        return Err(format!(
                            "choose one way to write the changelog: --{} or --{name}",
                            first.flag()
                        ));
                    }
                    _ => writer = Some(*chosen),
                }
            }
            Long(name) if writers.iter().any(|w| w.options().contains(&name)) => {
                let name = name.to_owned();
                let value = parser.value().map_err(err)?.to_string_lossy().into_owned();
                if options.iter().any(|(n, _)| *n == name) {
                    return Err(format!("--{name} is given twice"));
                }
                options.push((name, value));
            }
            Value(v) if range.is_none() => range = Some(v.to_string_lossy().into_owned()),
            Value(v) if collection.is_none() => collection = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    // Options may come before their flag, so they are checked once the writer is known.
    for (name, _) in &options {
        match writer {
            Some(w) if w.options().contains(&name.as_str()) => {}
            Some(w) => {
                return Err(format!("--{name} is not an option of --{}", w.flag()));
            }
            None => {
                let owner = writers
                    .iter()
                    .find(|w| w.options().contains(&name.as_str()))
                    .map_or(String::new(), |w| format!(" (use it with --{})", w.flag()));
                return Err(format!("--{name} needs a writer flag{owner}"));
            }
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
        writer: writer.map(|w| w.flag().to_owned()),
        options,
    }))
}

/// Calls `f` with the repository and both sides of the range, read from Git without a checkout.
fn with_sides<T>(
    args: &Args,
    f: impl FnOnce(&Repo, &Side<'_>, &Side<'_>) -> Result<T, AppError>,
) -> Result<T, AppError> {
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
    f(
        &repo,
        &Side {
            files: &old_files,
            read: &read_old,
        },
        &Side {
            files: &new_files,
            read: &read_new,
        },
    )
}

/// The schema sections of this repository, then what a distribution appends.
fn with_schemas<T>(
    extra: &[&dyn ChangelogSection],
    f: impl FnOnce(&[&dyn ChangelogSection]) -> T,
) -> T {
    let graphql = SchemaSection::new(SchemaKind::Graphql);
    let protobuf = SchemaSection::new(SchemaKind::Protobuf);
    let mut all: Vec<&dyn ChangelogSection> = vec![&graphql, &protobuf];
    all.extend_from_slice(extra);
    f(&all)
}

fn render(args: &Args, sections: &[&dyn ChangelogSection]) -> Result<String, AppError> {
    with_schemas(sections, |all| {
        with_sides(args, |_, old, new| {
            changelog_with(old, new, &args.from, &args.to, all)
        })
    })
}

/// What a writer builds on: the changelog, its parts and the commits of the range.
fn facts(args: &Args, sections: &[&dyn ChangelogSection]) -> Result<ChangelogFacts, AppError> {
    with_schemas(sections, |all| {
        with_sides(args, |repo, old, new| {
            let commits = repo.changelog_commits(&args.from, &args.to)?;
            changelog_facts(old, new, &args.from, &args.to, all, commits)
        })
    })
}

/// `sections`: what a distribution appends ([`crate::Distribution::changelog`]).
pub fn run(
    args: Args,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    sections: &[&dyn ChangelogSection],
    writers: &[&dyn ChangelogWriter],
) -> i32 {
    let written = match &args.writer {
        None => render(&args, sections),
        Some(flag) => match writers.iter().find(|w| w.flag() == flag) {
            Some(writer) => facts(&args, sections).and_then(|f| writer.write(&f, &args.options)),
            None => Err(AppError::Shell(format!("no writer for --{flag}"))),
        },
    };
    let text = match written {
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
