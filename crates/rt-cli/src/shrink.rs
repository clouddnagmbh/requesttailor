// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt shrink <request> [--env E] [--predicate P] [--keep PART]... [--max-probes N]
//! [--max-seconds N] [--simplify-values] [--allow-writes] [--json]` (wave 19, strand E): cut
//! a failing request down to the smallest one that still fails (`rt_app::shrink`). Exit 0
//! when a minimal request was found, 1 when the failure did not reproduce, `EXIT_BLOCKED`
//! (5) when the guard refused.
//!
//! Every probe is one send through the same services as `rt send` (so a distribution's
//! guard answers each of them). A production environment is said out loud on stderr, reads
//! may go there; a writing request needs `--allow-writes` and never goes to production. The
//! output holds names of removed parts and the smallest request as a `curl` command with
//! variables kept as variables; secret values appear nowhere.

use std::io::Write;
use std::path::PathBuf;

use rt_app::runner::RunSnapshot;
use rt_app::share::{CopyKind, CopyOptions, copy_spec};
use rt_app::shrink::{Predicate, ShrinkRequest, ShrinkResult, shrink};
use rt_core::send::SendOptions;
use rt_format::FileSet;

use crate::{EXIT_BLOCKED, EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, EXIT_USAGE, secrets, services};

/// Exit code when the failure did not reproduce (or came and went at the start).
pub const EXIT_NOT_REPRODUCED: i32 = 1;

pub const USAGE: &str = "  rt shrink <request-file> [-e <env>] [--predicate <P>] [--keep <part>]...\n            [--max-probes <N>] [--max-seconds <N>] [--simplify-values] [--allow-writes]\n            [--no-plugins] [--no-scripts] [--var NAME=VALUE]... [--json]\n";

pub const HELP: &str = "rt shrink sends variations of a failing request and prints the smallest one that still\nfails: fewer headers, query parameters, cookies and body fields. --predicate is\nstatus=500, 5xx, assertion[=NAME], body~PATTERN, header=NAME, no-header=NAME or last (the\ndefault: the failure of the last response). --keep header:Authorization keeps a part.\nAt most 100 probes (hard 500) and 120 s (hard 600). A writing request only runs with\n--allow-writes and never against a production environment. Exit 0 found, 1 not\nreproducible, 5 blocked by the guard.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub predicate: Predicate,
    pub keep: Vec<String>,
    pub max_probes: u32,
    pub max_seconds: u32,
    pub simplify_values: bool,
    pub allow_writes: bool,
    pub no_plugins: bool,
    pub no_scripts: bool,
    pub json: bool,
    /// The distribution's send guard (never from the command line; `with_guard` sets it).
    pub guard: rt_core::guard::GuardHandle,
    /// The distribution's exchange observers (likewise).
    pub observers: rt_core::observe::ObserverSet,
}

/// `status=500`, `5xx`, `assertion`, `assertion=NAME`, `body~PATTERN`, `header=NAME`,
/// `no-header=NAME`, `last`.
pub fn parse_predicate(text: &str) -> Result<Predicate, String> {
    let bad = || {
        format!(
            "unknown predicate {text:?} (status=N, 5xx, assertion[=NAME], body~PATTERN, header=NAME, no-header=NAME, last)"
        )
    };
    if text == "last" {
        return Ok(Predicate::LastFailure);
    }
    if text == "5xx" {
        return Ok(Predicate::ServerError);
    }
    if text == "assertion" {
        return Ok(Predicate::AssertionFails { assertion: None });
    }
    if let Some(name) = text.strip_prefix("assertion=") {
        return Ok(Predicate::AssertionFails {
            assertion: Some(name.to_owned()),
        });
    }
    if let Some(code) = text.strip_prefix("status=") {
        return Ok(Predicate::Status {
            status: code.parse().map_err(|_| bad())?,
        });
    }
    if let Some(pattern) = text.strip_prefix("body~") {
        return Ok(Predicate::BodyMatches {
            pattern: pattern.to_owned(),
        });
    }
    if let Some(name) = text.strip_prefix("header=") {
        return Ok(Predicate::HeaderPresent {
            name: name.to_owned(),
        });
    }
    if let Some(name) = text.strip_prefix("no-header=") {
        return Ok(Predicate::HeaderMissing {
            name: name.to_owned(),
        });
    }
    Err(bad())
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        environment: None,
        vars: Vec::new(),
        predicate: Predicate::LastFailure,
        keep: Vec::new(),
        max_probes: 0,
        max_seconds: 0,
        simplify_values: false,
        allow_writes: false,
        no_plugins: false,
        no_scripts: false,
        json: false,
        guard: rt_core::guard::GuardHandle::none(),
        observers: rt_core::observe::ObserverSet::new(),
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("predicate") => {
                args.predicate =
                    parse_predicate(&parser.value().map_err(err)?.string().map_err(err)?)?
            }
            Long("keep") => args
                .keep
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("max-probes") => {
                args.max_probes = parser.value().map_err(err)?.parse().map_err(err)?
            }
            Long("max-seconds") => {
                args.max_seconds = parser.value().map_err(err)?.parse().map_err(err)?
            }
            Long("simplify-values") => args.simplify_values = true,
            Long("allow-writes") => args.allow_writes = true,
            Long("no-plugins") => args.no_plugins = true,
            Long("no-scripts") => args.no_scripts = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("shrink: missing <request-file>")?;
    Ok(Some(args))
}

fn sentence(result: &ShrinkResult) -> String {
    if result.not_reproduced {
        return "The failure did not reproduce: nothing was shrunk.".into();
    }
    if result.flaky && result.removed.is_empty() {
        return "The failure comes and goes (flaky): nothing was shrunk.".into();
    }
    let parts = result.removed.iter().filter(|s| s.kind != "value").count();
    let values = result.removed.len() - parts;
    let mut out = format!(
        "Removed {parts} {} in {} probes",
        if parts == 1 { "part" } else { "parts" },
        result.probes
    );
    if values > 0 {
        out.push_str(&format!(
            ", simplified {values} {}",
            if values == 1 { "value" } else { "values" }
        ));
    }
    out.push_str(if result.one_minimal {
        " (1-minimal: every part left is needed)."
    } else {
        "."
    });
    out
}

fn text(args: &Args, result: &ShrinkResult, curl: Result<String, String>) -> String {
    let mut out = format!("{}\n", sentence(result));
    if result.flaky && !result.removed.is_empty() {
        out.push_str("flaky, result may not be minimal\n");
    }
    if let Some(reason) = &result.stopped_by {
        out.push_str(&format!(
            "Stopped early: {reason}; the result may not be minimal.\n"
        ));
    }
    if !result.removed.is_empty() {
        out.push_str("\nWhat was removed:\n");
        for step in &result.removed {
            let verb = if step.kind == "value" {
                "simplified"
            } else {
                "removed"
            };
            out.push_str(&format!("  {verb} {}\n", step.part));
        }
    }
    if !(result.not_reproduced || result.flaky && result.removed.is_empty()) {
        out.push_str(&format!(
            "\nThe smallest request for {}:\n",
            args.file.display()
        ));
        match curl {
            Ok(curl) => out.push_str(&format!("{curl}\n")),
            Err(why) => out.push_str(&format!("  (no curl command: {why})\n")),
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
    if let Some(env) = &args.environment
        && model.environment(env).is_none()
    {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("no environment {env:?} in {}", root.display()),
        );
    }
    if let Some(text) =
        crate::send_banner::banner_for(model, &request_path, args.environment.as_deref())
    {
        let _ = writeln!(stderr, "rt: {text}");
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
        environment: args.environment.clone(),
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
    let collection = match runtime.block_on(secrets::resolve_all(
        model,
        args.environment.as_deref(),
        &args.vars,
        &id,
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let snapshot = RunSnapshot {
        collection,
        environment: args.environment.clone(),
        vars: args.vars.clone(),
        files,
    };
    let cli = match services::CliServices::new(&send_args, &root) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let request = ShrinkRequest {
        path: request_path.clone(),
        environment: args.environment.clone(),
        predicate: args.predicate.clone(),
        keep: args.keep.clone(),
        max_probes: args.max_probes,
        max_seconds: args.max_seconds,
        simplify_values: args.simplify_values,
        confirmed_writes: args.allow_writes,
    };
    let options = SendOptions {
        body_limit: run_args.body_limit,
        iteration: None,
    };
    let result = match runtime.block_on(shrink(
        &snapshot,
        &request,
        &transport,
        &options,
        &cli.services(),
        &|| false,
    )) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    if let Some(rule) = result
        .stopped_by
        .as_deref()
        .and_then(|s| s.strip_prefix("blocked: "))
    {
        return fail(
            stderr,
            EXIT_BLOCKED,
            &format!("blocked by {rule}; nothing was sent"),
        );
    }
    let environment = args
        .environment
        .as_deref()
        .and_then(|name| model.environment(name));
    let output = if args.json {
        match serde_json::to_string_pretty(&result) {
            Ok(mut j) => {
                j.push('\n');
                j
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        }
    } else {
        let curl = copy_spec(
            &result.request,
            CopyKind::Curl,
            environment,
            &CopyOptions::default(),
        )
        .map(|c| c.text)
        .map_err(|e| e.to_string());
        text(&args, &result, curl)
    };
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    if result.not_reproduced || (result.flaky && result.removed.is_empty()) {
        EXIT_NOT_REPRODUCED
    } else {
        EXIT_OK
    }
}
