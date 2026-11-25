// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt access check <request> --as <env-a> --as <env-b> [--allow-writes] [--b-must-be-denied]
//! [--json]` (wave 19, strand D): the two-user access check of `rt_app::access`. A profile is
//! an environment. Exit `EXIT_TESTS` (4) when a finding is `broken-object-access` or
//! `broken-function-access`. Exit 5 when the send guard refused;
//! 2 when the check could not run (a writing request without `--allow-writes`, a production
//! environment, no object reference found); 0 otherwise (also `inconclusive`: the text says so).

use std::io::Write;
use std::path::PathBuf;

use rt_app::access::{AccessCheck, AccessEnv, AccessOptions, AccessProfile, AccessVerdict, check};
use rt_app::envcompare::EnvRun;
use rt_app::runner::RunSnapshot;
use rt_core::send::SendOptions;
use rt_format::FileSet;

use crate::{
    EXIT_BLOCKED, EXIT_OK, EXIT_PREPARE, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, secrets, services,
};

pub const USAGE: &str = "  rt access check <request-file> --as <env-a> --as <env-b> [--allow-writes]\n            [--b-must-be-denied] [--rate <N>] [--no-plugins] [--no-scripts]\n            [--var NAME=VALUE]... [--json]\n";

pub const HELP: &str = "rt access check sends one request as user A (an environment) and, if the answer holds\nan object, the same access as user B, and reports broken object or function access with\nmasked evidence. Reading requests run as they are; a writing request only with\n--allow-writes and never against a production environment. It uses only ids user A saw,\nsends two requests and at most --rate per second (default 5). Exit code 4 when access is\nbroken, 5 when the send guard refused, 2 when the check did not run (writes not allowed,\nproduction, no object reference found). Use it only with accounts you own.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    /// The two environments, A first.
    pub profiles: Vec<String>,
    pub allow_writes: bool,
    pub b_must_be_denied: bool,
    pub rate: u32,
    pub vars: Vec<(String, String)>,
    pub no_plugins: bool,
    pub no_scripts: bool,
    pub json: bool,
    /// The distribution's send guard (never from the command line; `with_guard` sets it).
    pub guard: rt_core::guard::GuardHandle,
    /// The distribution's exchange observers (likewise).
    pub observers: rt_core::observe::ObserverSet,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    match parser.next().map_err(err)? {
        Some(Short('h') | Long("help")) => return Ok(None),
        Some(Value(v)) if v == "check" => {}
        _ => {
            return Err(
                "access: expected `rt access check <request-file> --as <A> --as <B>`".into(),
            );
        }
    }
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        profiles: Vec::new(),
        allow_writes: false,
        b_must_be_denied: false,
        rate: 0,
        vars: Vec::new(),
        no_plugins: false,
        no_scripts: false,
        json: false,
        guard: Default::default(),
        observers: Default::default(),
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("as") => args
                .profiles
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("allow-writes") => args.allow_writes = true,
            Long("b-must-be-denied") => args.b_must_be_denied = true,
            Long("rate") => args.rate = parser.value().map_err(err)?.parse().map_err(err)?,
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("no-plugins") => args.no_plugins = true,
            Long("no-scripts") => args.no_scripts = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("access check: missing <request-file>")?;
    if args.profiles.len() != 2 {
        return Err("access check: give exactly two environments with --as".into());
    }
    Ok(Some(args))
}

fn text(args: &Args, result: &AccessCheck) -> String {
    let mut out = String::new();
    let head = match &result.verdict {
        AccessVerdict::Ok => "ok".to_owned(),
        AccessVerdict::BrokenObjectAccess => "BROKEN OBJECT ACCESS".to_owned(),
        AccessVerdict::BrokenFunctionAccess => "BROKEN FUNCTION ACCESS".to_owned(),
        AccessVerdict::Inconclusive => "inconclusive".to_owned(),
        AccessVerdict::Skipped { reason } => format!("not checked: {reason}"),
    };
    out.push_str(&format!(
        "{head}  {} ({} as A, {} as B)\n",
        result.path, args.profiles[0], args.profiles[1]
    ));
    for finding in &result.findings {
        out.push_str(&format!("  {}\n", finding.message));
        for e in &finding.evidence {
            let status = e.status.map_or("no answer".to_owned(), |s| s.to_string());
            out.push_str(&format!("    {}: status {status}", e.profile));
            if !e.values.is_empty() {
                let shown: Vec<String> = e
                    .values
                    .iter()
                    .take(6)
                    .map(|(p, v)| format!("{p}={v}"))
                    .collect();
                out.push_str(&format!(", {}", shown.join(" ")));
            }
            out.push('\n');
        }
    }
    out
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, request_path) = match crate::find_root(&args.file) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let model = doc.model();
    if model.request(&request_path).is_none() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("{request_path} is not a request of the collection"),
        );
    }
    for env in &args.profiles {
        if model.environment(env).is_none() {
            return fail(
                stderr,
                EXIT_USAGE,
                &format!("no environment {env:?} in {}", root.display()),
            );
        }
        if let Some(text) = crate::send_banner::banner_for(model, &request_path, Some(env)) {
            let _ = writeln!(stderr, "rt: {text}");
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
        no_plugins: args.no_plugins,
        no_scripts: args.no_scripts,
        guard: args.guard.clone(),
        observers: args.observers.clone(),
        ..Default::default()
    };
    let send_args = run_args.send_args();
    let id = crate::oauth::collection_id(&root);
    let mut snapshots = Vec::new();
    for env in &args.profiles {
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
    let mut snapshots = snapshots.into_iter();
    let (Some(a), Some(b)) = (snapshots.next(), snapshots.next()) else {
        return fail(
            stderr,
            EXIT_USAGE,
            "give exactly two environments with --as",
        );
    };
    let a = EnvRun {
        snapshot: a,
        services: cli.services(),
    };
    let b = EnvRun {
        snapshot: b,
        services: cli.services(),
    };
    let data = rt_plugin::host::dir::data_dir();
    let audit = |entry: &rt_app::agents::AuditEntry| {
        if let Some(dir) = &data {
            let _ = rt_app::agents::append_audit(dir, entry);
        }
    };
    let options = SendOptions {
        body_limit: run_args.body_limit,
        iteration: None,
    };
    let env = AccessEnv {
        a: &a,
        b: &b,
        transport: &transport,
        options: &options,
        caller: "cli",
        audit: &audit,
        cancelled: &|| false,
    };
    let profile = AccessProfile {
        a: args.profiles[0].clone(),
        b: args.profiles[1].clone(),
    };
    let check_options = AccessOptions {
        allow_writes: args.allow_writes,
        b_must_be_denied: args.b_must_be_denied,
        rate: args.rate,
    };
    let result = match runtime.block_on(check(&env, &request_path, &profile, &check_options)) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    let output = if args.json {
        match serde_json::to_string_pretty(&result) {
            Ok(mut j) => {
                j.push('\n');
                j
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        }
    } else {
        text(&args, &result)
    };
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    if let AccessVerdict::Skipped { reason } = &result.verdict {
        let hint = if reason == rt_app::access::SKIPPED_WRITES {
            " (pass --allow-writes to send a writing request)"
        } else {
            ""
        };
        let _ = writeln!(stderr, "rt: the check did not run: {reason}{hint}");
    }
    exit_code(&result.verdict)
}

/// The exit code of a check: 4 for broken access, 5 when the send guard refused, 2 when the
/// check did not run, 0 otherwise.
pub fn exit_code(verdict: &AccessVerdict) -> i32 {
    match verdict {
        v if v.is_broken() => EXIT_TESTS,
        AccessVerdict::Skipped { reason } if reason.contains("send guard") => EXIT_BLOCKED,
        AccessVerdict::Skipped { .. } => EXIT_USAGE,
        _ => EXIT_OK,
    }
}
