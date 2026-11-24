// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt compare-env <folder> --env A --env B [--env C]... [--json] [--ignore-env-values]
//! [--fail-on-diff]` (wave 18, strand A): the same reading requests in two or more
//! environments, compared as `rt_app::envcompare` does it for the app. Exit 0 also when the
//! environments differ; with `--fail-on-diff` exit 1 when they do (status, headers, body,
//! tests, or a request that fails in one environment only).
//!
//! Writing requests are skipped and named. The first `--env` is the reference. Production
//! environments are sent to (reads only) with a banner on stderr, as `rt run` does. Secret
//! values come from the keychain or external secrets per environment, or `--var NAME=VALUE`
//! for all of them; they never appear in the output.

use std::io::Write;
use std::path::PathBuf;

use rt_app::compare::RequestComparison;
use rt_app::envcompare::{EnvCompareRequest, EnvComparison, EnvRun, compare_environments};
use rt_app::runner::RunSnapshot;
use rt_core::send::SendOptions;
use rt_format::FileSet;

use crate::{EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, EXIT_USAGE, secrets, services};

pub const USAGE: &str = "  rt compare-env [<folder>] --env <A> --env <B>... [--json] [--ignore-env-values]\n            [--fail-on-diff] [--var NAME=VALUE]...\n";

pub const HELP: &str = "rt compare-env sends the reading requests of a collection or folder in two or\nmore environments (the first --env is the reference) and lists what differs: status,\nheaders and JSON body. Writing requests are skipped and named. Secret values never\nappear. Exit code 0 even with differences; --fail-on-diff exits 1 for them.\n";

/// Exit code of `--fail-on-diff` when the environments differ.
pub const EXIT_DIFFERENT: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// A folder of the collection (default: the current folder).
    pub folder: PathBuf,
    pub environments: Vec<String>,
    pub json: bool,
    pub ignore_env_values: bool,
    pub fail_on_diff: bool,
    /// `--var NAME=VALUE`: runtime variables in every environment (also values for secrets).
    pub vars: Vec<(String, String)>,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        folder: PathBuf::from("."),
        ..Args::default()
    };
    let mut folder = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('e') | Long("env") => {
                args.environments
                    .push(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("var") => {
                let text = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = text
                    .split_once('=')
                    .ok_or_else(|| format!("--var needs NAME=VALUE, got {text:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("json") => args.json = true,
            Long("ignore-env-values") => args.ignore_env_values = true,
            Long("fail-on-diff") => args.fail_on_diff = true,
            Value(v) if folder.is_none() => folder = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if args.environments.len() < 2 {
        return Err("compare-env needs at least two --env".into());
    }
    if let Some(f) = folder {
        args.folder = f;
    }
    Ok(Some(args))
}

/// What differs in one request, in a few words.
fn describe(r: &RequestComparison) -> String {
    let mut parts = Vec::new();
    if let Some(s) = &r.status {
        let show = |v: Option<u16>| v.map_or("none".to_owned(), |v| v.to_string());
        parts.push(format!("status {} -> {}", show(s.old), show(s.new)));
    }
    if let Some(e) = &r.error {
        parts.push(format!(
            "error: {}",
            e.new.as_deref().or(e.old.as_deref()).unwrap_or("")
        ));
    }
    if !r.headers.is_empty() {
        parts.push(format!("{} header(s)", r.headers.len()));
    }
    if !r.body.is_empty() {
        let at: Vec<&str> = r.body.iter().take(3).map(|b| b.pointer.as_str()).collect();
        let more = r.body.len().saturating_sub(3);
        let tail = if more > 0 {
            format!(" +{more}")
        } else {
            String::new()
        };
        parts.push(format!("body {}{tail}", at.join(", ")));
    }
    if r.body_text {
        parts.push("body text".into());
    }
    if !r.tests.is_empty() {
        parts.push(format!("{} test(s)", r.tests.len()));
    }
    if parts.is_empty() {
        parts.push("not sent in one environment".into());
    }
    parts.join(", ")
}

fn text(cmp: &EnvComparison) -> String {
    let mut out = String::new();
    for pair in &cmp.pairs {
        let changed = pair.requests.iter().filter(|r| !r.same).count();
        out.push_str(&format!(
            "{} (reference) vs {}: {} of {} requests differ\n",
            cmp.reference,
            pair.environment,
            changed,
            pair.requests.len()
        ));
        for r in &pair.requests {
            if r.same {
                out.push_str(&format!("  same     {}\n", r.path));
            } else {
                out.push_str(&format!("  differs  {}: {}\n", r.path, describe(r)));
            }
        }
    }
    for s in &cmp.skipped {
        out.push_str(&format!("  {}  {}\n", s.reason, s.path));
    }
    out
}

fn differs(cmp: &EnvComparison) -> bool {
    cmp.pairs.iter().any(|p| p.requests.iter().any(|r| !r.same))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, folder) = match crate::run::locate(&args.folder) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let model = doc.model();
    for env in &args.environments {
        if model.environment(env).is_none() {
            return fail(
                stderr,
                EXIT_USAGE,
                &format!("no environment {env:?} in {}", root.display()),
            );
        }
    }
    for env in &args.environments {
        if rt_app::agents::is_production(&model.config.agents, Some(env)) {
            let _ = writeln!(
                stderr,
                "rt: PRODUCTION: sending the reading requests to the environment {env}"
            );
        }
    }
    let files = match FileSet::from_dir(&root) {
        Ok(f) => f,
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{}: {e}", root.display())),
    };
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
    let run_args = crate::run::RunArgs {
        vars: args.vars.clone(),
        prompt: false,
        ..Default::default()
    };
    let send_args = run_args.send_args();
    let id = crate::oauth::collection_id(&root);
    let mut snapshots = Vec::new();
    for env in &args.environments {
        let collection =
            match runtime.block_on(secrets::resolve_all(model, Some(env), &args.vars, &id)) {
                Ok(c) => c,
                Err(e) => return fail(stderr, EXIT_PREPARE, &e),
            };
        snapshots.push(RunSnapshot {
            collection,
            environment: Some(env.clone()),
            vars: args.vars.clone(),
            files: files.clone(),
        });
    }
    let cli = match services::CliServices::new(&send_args, &root) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let runs: Vec<EnvRun> = snapshots
        .into_iter()
        .map(|snapshot| EnvRun {
            snapshot,
            services: cli.services(),
        })
        .collect();
    let request = EnvCompareRequest {
        environments: args.environments.clone(),
        folder,
        paths: None,
        ignore_env_values: args.ignore_env_values,
        production_confirmed: true,
    };
    let options = SendOptions {
        body_limit: run_args.body_limit,
        iteration: None,
    };
    let cmp = match runtime.block_on(compare_environments(
        &runs,
        &request,
        &transport,
        &options,
        &|| false,
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    let output = if args.json {
        match serde_json::to_string_pretty(&cmp) {
            Ok(mut j) => {
                j.push('\n');
                j
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        }
    } else {
        text(&cmp)
    };
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    if args.fail_on_diff && differs(&cmp) {
        EXIT_DIFFERENT
    } else {
        EXIT_OK
    }
}
