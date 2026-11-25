// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt notebook run <file.notebook.md> [-e <env>] [--var NAME=VALUE]... [--block <id>]
//! [--allow-writes] [--json]` (wave 19, strand W): runs the request blocks of a notebook
//! (`rt_app::notebook`) and prints what each block produced; outputs are printed, never saved.
//! Exit `EXIT_TESTS` (4) when a request, test or assertion failed, `EXIT_BLOCKED` (5) when the
//! send guard refused a request, 2 for usage errors, 0 otherwise. A writing block without
//! `--allow-writes` and every writing block against a production environment is skipped, said
//! on stderr, and does not change the exit code.

use std::io::Write;
use std::path::PathBuf;

use rt_app::envcompare::EnvRun;
use rt_app::notebook::{BlockResult, NotebookEnv, NotebookRun, Show};
use rt_app::runner::RunSnapshot;
use rt_core::send::{ReportBody, SendError, SendOptions};
use rt_format::FileSet;

use crate::{
    EXIT_BLOCKED, EXIT_OK, EXIT_PREPARE, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, secrets, services,
};

pub const USAGE: &str = "  rt notebook run <file.notebook.md> [-e <env>] [--var <NAME=VALUE>]... [--block <id>]\n            [--allow-writes] [--no-plugins] [--no-scripts] [--json]\n";

pub const HELP: &str = "rt notebook run runs the request blocks of an API notebook (a .notebook.md file next to\nyour requests), passing variables from block to block, and prints each response. Reading\nrequests run as they are; writing ones only with --allow-writes and never against\nproduction. Outputs are not saved.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub file: PathBuf,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub block: Option<String>,
    pub allow_writes: bool,
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
        Some(Value(v)) if v == "run" => {}
        _ => return Err("notebook: expected `rt notebook run <file.notebook.md>`".into()),
    }
    let mut file = None;
    let mut args = Args {
        file: PathBuf::new(),
        environment: None,
        vars: Vec::new(),
        block: None,
        allow_writes: false,
        no_plugins: false,
        no_scripts: false,
        json: false,
        guard: Default::default(),
        observers: Default::default(),
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
                    .ok_or("--var expects NAME=VALUE")?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("block") => args.block = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("allow-writes") => args.allow_writes = true,
            Long("no-plugins") => args.no_plugins = true,
            Long("no-scripts") => args.no_scripts = true,
            Long("json") => args.json = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("notebook run: missing <file.notebook.md>")?;
    Ok(Some(args))
}

const SHOWN_BODY: usize = 4096;

fn cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// One block as text: a header line, the variables it set, then what `show:` asked for.
fn text(result: &BlockResult, show: &[Show]) -> String {
    let mut out = format!("## {}", result.block);
    if let Some(why) = &result.skipped {
        out.push_str(&format!("  {why}\n"));
        return out;
    }
    if let Some(report) = &result.report {
        if let Some(r) = &report.request {
            out.push_str(&format!("  {} {}", r.method, r.url));
        }
        match (&report.response, &report.error) {
            (Some(r), _) => out.push_str(&format!(
                "  {} {} ms  {} B",
                r.status,
                r.total.as_millis(),
                r.size
            )),
            (None, Some(SendError::Blocked { rule, reason })) => {
                out.push_str(&format!("  refused by the send guard ({rule}): {reason}"))
            }
            (None, Some(e)) => out.push_str(&format!("  failed: {e:?}")),
            (None, None) => {}
        }
    }
    out.push('\n');
    for (name, value) in &result.set {
        out.push_str(&format!("  {name} = {value}\n"));
    }
    for note in &result.notes {
        out.push_str(&format!("  note: {note}\n"));
    }
    let Some(report) = &result.report else {
        return out;
    };
    if show.contains(&Show::Response)
        && let Some(r) = &report.response
        && let ReportBody::Utf8(body) = &r.body
    {
        let pretty = serde_json::from_str::<serde_json::Value>(body)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| body.clone());
        let mut end = pretty.len().min(SHOWN_BODY);
        while !pretty.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&pretty[..end]);
        if end < pretty.len() {
            out.push_str("\n  ... (cut)");
        }
        out.push('\n');
    }
    for view in &result.views {
        let label = match &view.show {
            Show::Table { path } => format!("table {path}"),
            Show::Chart { path } => format!("chart {path}"),
            Show::Response => continue,
        };
        out.push_str(&format!("  {label}\n"));
        if let Some(n) = &view.result.notice {
            out.push_str(&format!("    ({n})\n"));
        }
        if !view.result.columns.is_empty() {
            out.push_str(&format!("    {}\n", view.result.columns.join("\t")));
            for row in &view.result.rows {
                let cells: Vec<String> = row.iter().map(cell).collect();
                out.push_str(&format!("    {}\n", cells.join("\t")));
            }
        } else if !view.result.values.is_empty() {
            let values: Vec<String> = view.result.values.iter().map(cell).collect();
            out.push_str(&format!("    {}\n", values.join(", ")));
        }
    }
    out
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, path) = match crate::find_root(&args.file) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if !rt_app::notebook::is_notebook(&path) {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!(
                "{path} is not a notebook (its name ends in {})",
                rt_app::notebook::SUFFIX
            ),
        );
    }
    let notebook = match std::fs::read_to_string(&args.file)
        .map_err(|e| e.to_string())
        .and_then(|t| rt_app::notebook::parse_named(&path, &t).map_err(|e| e.to_string()))
    {
        Ok(n) => n,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if let Some(id) = &args.block
        && !notebook
            .blocks
            .iter()
            .any(|b| matches!(b, rt_app::notebook::Block::Request { id: i, .. } if i == id))
    {
        return fail(stderr, EXIT_USAGE, &format!("{path} has no block {id:?}"));
    }
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let model = doc.model();
    if let Some(name) = &args.environment
        && model.environment(name).is_none()
    {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("no environment {name:?} in {}", root.display()),
        );
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
    if let Some(text) =
        crate::send_banner::run_banner(&collection.config.agents, args.environment.as_deref(), 1)
    {
        let _ = writeln!(stderr, "rt: {text}");
    }
    let cli = match services::CliServices::new(&send_args, &root) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let env_run = EnvRun {
        snapshot: RunSnapshot {
            collection,
            environment: args.environment.clone(),
            vars: args.vars.clone(),
            files,
        },
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
    let env = NotebookEnv {
        run: &env_run,
        transport: &transport,
        options: &options,
        caller: "cli",
        audit: &audit,
        cancelled: &|| false,
    };
    let request = NotebookRun {
        path: path.clone(),
        block: args.block.clone(),
        environment: args.environment.clone(),
        allow_writes: args.allow_writes,
        vars: Vec::new(),
    };
    let outcome = match runtime.block_on(rt_app::notebook::run(&env, &notebook, &request)) {
        Ok(o) => o,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    let output = if args.json {
        match serde_json::to_string_pretty(&outcome) {
            Ok(mut j) => {
                j.push('\n');
                j
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        }
    } else {
        let mut out = String::new();
        for result in &outcome.results {
            let show: Vec<Show> = notebook
                .blocks
                .iter()
                .find_map(|b| match b {
                    rt_app::notebook::Block::Request { id, show, .. } if *id == result.block => {
                        Some(show.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            out.push_str(&text(result, &show));
        }
        out
    };
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    let mut code = EXIT_OK;
    let mut skipped_writes = false;
    for result in &outcome.results {
        if let Some(report) = &result.report {
            if matches!(report.error, Some(SendError::Blocked { .. })) {
                return EXIT_BLOCKED;
            }
            if !rt_core::runner::passed(report) {
                code = EXIT_TESTS;
            }
        }
        if result.skipped.as_deref() == Some(rt_app::notebook::SKIPPED_WRITES) {
            skipped_writes = true;
        }
        if result.skipped.is_some() && result.report.is_none() && result.set.is_empty() {
            // Skipped for a reason other than writes (`no request`) is a failed notebook.
            if result
                .skipped
                .as_deref()
                .is_some_and(|s| s.starts_with("no request"))
            {
                code = EXIT_TESTS;
            }
        }
    }
    if skipped_writes {
        let _ = writeln!(
            stderr,
            "rt: a writing block was skipped (pass --allow-writes to send it; never against production)"
        );
    }
    code
}
