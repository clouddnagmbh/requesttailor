// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt bisect --good <rev> --bad <rev> [<folder>]`: bisect with the runner (`rt_app::bisect`), each candidate read with `rt-git` without a checkout.
//!
//! ```text
//! rt bisect --good <rev> --bad <rev> [-e NAME] [--var NAME=VALUE]... [--json]
//!           [--no-scripts] [--no-plugins] [--no-prompt] [--body-limit BYTES] [<folder>]
//! ```
//!
//! The candidates are the commits on the first-parent chain from `--bad` back to `--good`
//! (`--good` excluded). Each one is run from Git's object store, no checkout: the requests of
//! that commit, with today's environments and secrets (the working folder's and the
//! keychain's). A commit is bad when a request, test or assertion fails, and skipped when the
//! collection cannot be read there.
//!
//! Progress goes to stderr (`tested <id> good|bad|skip`), the result to stdout. Exit codes:
//! `EXIT_OK` the first bad commit was found, `EXIT_TESTS` there is none (`--bad` passes, or
//! only skipped commits are left), `EXIT_USAGE` usage error (unknown revision, environment or
//! folder, not a collection or repository), `EXIT_PREPARE` secrets or plugins could not be set
//! up, `EXIT_TRANSPORT` no network stack.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_app::bisect::{BisectOutcome, BisectStep, BisectVerdict, search, test_revision};
use rt_app::git::GitTree;
use rt_app::runner::RunSnapshot;
use rt_core::model::Item;
use rt_core::send::SendOptions;
use rt_format::{COLLECTION_FILE, FileSet};

use crate::{
    EXIT_OK, EXIT_PREPARE, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, SendArgs, secrets, services,
};

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt bisect --good <rev> --bad <rev> [<folder>] [-e <NAME>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt bisect finds the first commit between --good and --bad at which a test,\nan assertion or a request of the collection (or <folder>) fails, running each\ncandidate from the Git tree without a checkout, with today's environments and\nsecrets. Exit 0: found; 4: no failing commit. --json prints the result as JSON.\n";

/// Most commits looked at between `--good` and `--bad`.
const CANDIDATE_LIMIT: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub good: String,
    pub bad: String,
    /// A folder inside a collection or the collection root; default: the current folder.
    pub path: Option<PathBuf>,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub json: bool,
    pub no_scripts: bool,
    pub no_plugins: bool,
    pub prompt: bool,
    pub body_limit: usize,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let (mut good, mut bad) = (None, None);
    let mut args = Args {
        good: String::new(),
        bad: String::new(),
        path: None,
        environment: None,
        vars: Vec::new(),
        json: false,
        no_scripts: false,
        no_plugins: false,
        prompt: true,
        body_limit: SendOptions::default().body_limit,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("good") => good = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("bad") => bad = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("json") => args.json = true,
            Long("no-scripts") => args.no_scripts = true,
            Long("no-plugins") => args.no_plugins = true,
            Long("no-prompt") => args.prompt = false,
            Long("body-limit") => {
                args.body_limit = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Value(v) if args.path.is_none() => args.path = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.good = good.ok_or("rt bisect needs --good <rev>")?;
    args.bad = bad.ok_or("rt bisect needs --bad <rev>")?;
    Ok(Some(args))
}

/// The collection root above (or at) `dir` and the folder `dir` is, relative to the root.
fn locate(dir: &Path) -> Result<(PathBuf, Option<String>), String> {
    let dir =
        std::fs::canonicalize(dir).map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    if dir.is_file() {
        return Err(format!(
            "{} is a file: rt bisect takes a folder",
            dir.display()
        ));
    }
    let root = dir
        .ancestors()
        .find(|d| d.join(COLLECTION_FILE).is_file())
        .ok_or_else(|| {
            format!(
                "no {COLLECTION_FILE} in {} or any folder above",
                dir.display()
            )
        })?
        .to_path_buf();
    let mut parts = Vec::new();
    for c in dir.strip_prefix(&root).expect("ancestor").components() {
        match c {
            Component::Normal(p) => parts.push(
                p.to_str()
                    .ok_or_else(|| format!("path is not UTF-8: {}", dir.display()))?
                    .to_owned(),
            ),
            _ => return Err(format!("unexpected path {}", dir.display())),
        }
    }
    Ok((root, (!parts.is_empty()).then(|| parts.join("/"))))
}

fn has_folder(items: &[Item], path: &str) -> bool {
    items.iter().any(|i| match i {
        Item::Folder(f) => f.path == path || has_folder(&f.items, path),
        Item::Request(_) => false,
    })
}

fn short(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, folder) = match locate(args.path.as_deref().unwrap_or(Path::new("."))) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let model = doc.model();
    if let Some(f) = &folder
        && !has_folder(&model.items, f)
    {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("{f} is not a folder of the collection {}", root.display()),
        );
    }
    if let Some(env) = &args.environment
        && model.environment(env).is_none()
    {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("no environment {env:?} in {}", root.display()),
        );
    }
    let repo = match rt_git::Repo::open(&root, &rt_git::Options::default()) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return fail(
                stderr,
                EXIT_USAGE,
                &format!("{} is not in a Git repository", root.display()),
            );
        }
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    let candidates = match repo.commits(Some(&args.good), &args.bad, CANDIDATE_LIMIT) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    if candidates.is_empty() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!(
                "no commits between {} (good) and {} (bad)",
                args.good, args.bad
            ),
        );
    }

    let transport = match rt_core::transport::native::NativeTransport::new() {
        Ok(t) => t,
        Err(e) => return fail(stderr, EXIT_TRANSPORT, &e.to_string()),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return fail(
                stderr,
                EXIT_TRANSPORT,
                &format!("cannot start runtime: {e}"),
            );
        }
    };
    let send_args = SendArgs {
        environment: args.environment.clone(),
        vars: args.vars.clone(),
        body_limit: args.body_limit,
        no_plugins: args.no_plugins,
        no_scripts: args.no_scripts,
        prompt: args.prompt,
        ..SendArgs::default()
    };
    // Today's environments and secret values, as `rt run` resolves them.
    let collection = match runtime.block_on(secrets::resolve_all(
        model,
        args.environment.as_deref(),
        &args.vars,
        &crate::oauth::collection_id(&root),
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let cli = match services::CliServices::new(&send_args, &root) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let today = RunSnapshot {
        collection,
        environment: args.environment.clone(),
        vars: args.vars.clone(),
        files: FileSet::default(),
    };
    let options = SendOptions {
        body_limit: args.body_limit,
        iteration: None,
    };
    let services = cli.services();

    let mut progress = |step: &BisectStep| {
        let verdict = match step.verdict {
            BisectVerdict::Good => "good",
            BisectVerdict::Bad => "bad",
            BisectVerdict::Skip => "skip",
        };
        let _ = writeln!(
            stderr,
            "tested {} {verdict} ({} left) {}",
            short(&step.commit.oid),
            step.remaining,
            step.commit.summary
        );
        for failed in &step.failed {
            let _ = writeln!(stderr, "  {failed}");
        }
        let _ = stderr.flush();
    };
    let outcome = runtime.block_on(search(
        candidates,
        async |commit: &rt_app::git::GitCommit| {
            let tree: Result<GitTree, String> =
                repo.tree_at(&commit.oid).map_err(|e| e.to_string());
            test_revision(
                &today,
                tree,
                folder.as_deref(),
                &transport,
                &options,
                &services,
                &|| false,
            )
            .await
        },
        &mut progress,
        &|| false,
    ));
    report(&outcome, args.json, stdout, stderr)
}

fn report(
    outcome: &BisectOutcome,
    json: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    if json {
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(outcome).unwrap_or_default()
        );
    }
    let Some(first) = &outcome.first_bad else {
        let why = if outcome
            .steps
            .first()
            .is_some_and(|s| s.verdict == BisectVerdict::Good)
        {
            "the bad revision passes: nothing fails"
        } else {
            "no first failing commit could be told (only skipped commits are left, or the newest cannot be tested)"
        };
        let _ = writeln!(stderr, "rt bisect: {why}");
        return EXIT_TESTS;
    };
    if !json {
        let _ = writeln!(stdout, "first bad commit: {} {}", first.oid, first.summary);
        if let Some(step) = outcome.steps.iter().find(|s| s.commit.oid == first.oid) {
            for failed in &step.failed {
                let _ = writeln!(stdout, "  {failed}");
            }
        }
        let _ = writeln!(
            stdout,
            "tested {} of {} commits",
            outcome.steps.len(),
            outcome.candidates
        );
    }
    EXIT_OK
}
