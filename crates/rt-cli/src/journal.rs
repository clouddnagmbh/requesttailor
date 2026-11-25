// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt journal [list|revert <id> --with <request>|clear] [-C <collection>] [--json]` (wave
//! 19, strand C): the test-data journal of `rt_app::journal` (device state in the data
//! folder). `revert` sends the request you name with the entry's values filled in, under
//! the rules of any writing request (guard, production needs `--yes-production`).
//!
//! The journal is device state shared with the app (`journal/<collection>.jsonl` in the data
//! folder); the CLI does not record. `list` shows what the app recorded, `revert` sends the
//! request you name with the entry's values filled in (secrets from the keychain, external
//! sources or `--var`), writes an audit entry and marks the entry reverted. Exit codes: 0
//! reverted, 5 the send guard refused, 1 the request failed or was not reversible, 2 usage.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rt_app::journal::{
    FileStorage, Journal, JournalEntry, JournalQuery, RevertEnv, RevertOptions, RevertPlan,
    StepResult, execute, plan_manual,
};
use rt_app::runner::RunSnapshot;
use rt_core::send::SendOptions;
use rt_format::FileSet;

use crate::{EXIT_BLOCKED, EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, EXIT_USAGE, secrets, services};

pub const USAGE: &str = "  rt journal [list | revert <id> --with <request-file> | clear] [-C <collection>] [--json]\n            [--yes-production] [--var NAME=VALUE]...\n";

pub const HELP: &str = "rt journal lists the successful writing requests a session sent (when \"Track test data\"\nis on for the collection) and puts one back with a request you choose: revert <id> --with\n<request-file> fills {{journal.id}}, {{journal.location}} and {{journal.etag}} from the\nentry and sends it, as any writing request: guard, audit log, and for production only with\n--yes-production. The journal keeps no bodies and no header values.\n";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Action {
    #[default]
    List,
    Revert {
        id: u64,
        with: PathBuf,
    },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub action: Action,
    pub collection: PathBuf,
    pub json: bool,
    pub yes_production: bool,
    /// `--var NAME=VALUE`: runtime variables (also values for secrets).
    pub vars: Vec<(String, String)>,
    /// The distribution's send guard (set by `run_distribution`).
    pub guard: rt_core::guard::GuardHandle,
    pub observers: rt_core::observe::ObserverSet,
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
        vars: Vec::new(),
        guard: rt_core::guard::GuardHandle::none(),
        observers: rt_core::observe::ObserverSet::new(),
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
            Long("var") => {
                let text = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = text
                    .split_once('=')
                    .ok_or_else(|| format!("--var needs NAME=VALUE, got {text:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
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

fn fail(stderr: &mut dyn Write, code: i32, message: &str) -> i32 {
    let _ = writeln!(stderr, "rt: {message}");
    code
}

fn line(e: &JournalEntry) -> String {
    let mut text = format!("#{:<5} {} {} {}", e.id, e.method, e.url, e.status);
    if let Some(env) = &e.environment {
        text.push_str(&format!("  [{env}]"));
    }
    if let Some(id) = &e.resource_id {
        text.push_str(&format!("  id={id}"));
    }
    if let Some(loc) = &e.location {
        text.push_str(&format!("  location={loc}"));
    }
    if e.reverted_at.is_some() {
        text.push_str("  (reverted)");
    }
    text
}

fn plan_text(plan: &RevertPlan) -> String {
    let mut out = String::new();
    for step in &plan.steps {
        out.push_str(&format!(
            "{} {} ({:?}){}\n",
            step.method,
            step.url,
            step.effect,
            plan.environment
                .as_deref()
                .map(|e| format!(" in {e}"))
                .unwrap_or_default()
        ));
    }
    out
}

/// The request path of `file` inside the collection `root` (`/`-separated).
fn request_path(root: &Path, file: &Path) -> Result<String, String> {
    let candidates = [file.to_path_buf(), root.join(file)];
    for candidate in candidates {
        if let Ok(canonical) = std::fs::canonicalize(&candidate)
            && let Ok(rel) = canonical.strip_prefix(root)
        {
            return Ok(rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"));
        }
    }
    Err(format!(
        "{} is not a request file of {}",
        file.display(),
        root.display()
    ))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let (root, _) = match crate::run::locate(&args.collection) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let model = doc.model();
    let Some(data) = rt_plugin::host::dir::data_dir() else {
        return fail(stderr, EXIT_USAGE, "no data folder (set RT_DATA_DIR)");
    };
    let id = crate::oauth::collection_id(&root);
    let journal = Journal::open(&model.name, Arc::new(FileStorage::new(&data, &id)));
    match &args.action {
        Action::List => {
            let page = journal.entries(&JournalQuery {
                limit: 1000,
                ..Default::default()
            });
            let text = if args.json {
                match serde_json::to_string_pretty(&page) {
                    Ok(mut j) => {
                        j.push('\n');
                        j
                    }
                    Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
                }
            } else if page.entries.is_empty() {
                "The journal is empty.\n".to_owned()
            } else {
                page.entries.iter().map(|e| line(e) + "\n").collect()
            };
            let _ = stdout.write_all(text.as_bytes());
            EXIT_OK
        }
        Action::Clear => {
            journal.clear();
            journal.flush();
            let _ = writeln!(stdout, "Journal cleared.");
            EXIT_OK
        }
        Action::Revert { id: entry_id, with } => revert(
            &args, &root, model, &journal, *entry_id, with, stdout, stderr,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn revert(
    args: &Args,
    root: &Path,
    model: &rt_core::model::Collection,
    journal: &Journal,
    entry_id: u64,
    with: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let Some(entry) = journal.entry(entry_id) else {
        return fail(stderr, EXIT_USAGE, &format!("no journal entry #{entry_id}"));
    };
    let path = match request_path(root, with) {
        Ok(p) => p,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let files = match FileSet::from_dir(root) {
        Ok(f) => f,
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{}: {e}", root.display())),
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
    let id = crate::oauth::collection_id(root);
    let collection = match runtime.block_on(secrets::resolve_all(
        model,
        entry.environment.as_deref(),
        &args.vars,
        &id,
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let snapshot = RunSnapshot {
        collection,
        environment: entry.environment.clone(),
        vars: args.vars.clone(),
        files,
    };
    let plan = match plan_manual(&snapshot, &entry, &path) {
        Ok(p) => p,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    let _ = write!(stderr, "{}", plan_text(&plan));
    if plan.production {
        let _ = writeln!(
            stderr,
            "rt: PRODUCTION: this sends a writing request to the environment {}",
            plan.environment.as_deref().unwrap_or("")
        );
        if !args.yes_production {
            return fail(
                stderr,
                EXIT_USAGE,
                "reverting in production needs --yes-production",
            );
        }
    }
    let transport = match rt_core::transport::native::NativeTransport::new() {
        Ok(t) => t,
        Err(e) => return fail(stderr, EXIT_TRANSPORT, &e.to_string()),
    };
    let run_args = crate::run::RunArgs {
        vars: args.vars.clone(),
        prompt: false,
        guard: args.guard.clone(),
        observers: args.observers.clone(),
        ..Default::default()
    };
    let send_args = run_args.send_args();
    let cli = match services::CliServices::new(&send_args, root) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let services = cli.services();
    let options = SendOptions {
        body_limit: run_args.body_limit,
        iteration: None,
    };
    let env = RevertEnv {
        snapshot: &snapshot,
        transport: &transport,
        options: &options,
        services: &services,
        caller: "cli",
    };
    let data = rt_plugin::host::dir::data_dir();
    let audit = |entry: &rt_app::agents::AuditEntry| {
        if let Some(dir) = &data {
            let _ = rt_app::agents::append_audit(dir, entry);
        }
    };
    let outcome = match runtime.block_on(execute(
        &env,
        Some(journal),
        &plan,
        &RevertOptions {
            production_confirmed: args.yes_production,
        },
        &audit,
        &|| false,
    )) {
        Ok(o) => o,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    journal.flush();
    let mut code = EXIT_OK;
    if !args.json {
        for out in &outcome.steps {
            let _ = writeln!(
                stdout,
                "#{}: {:?}: {}",
                out.entry_id, out.result, out.message
            );
        }
    }
    for out in &outcome.steps {
        if out.result != StepResult::Reverted {
            let blocked = out.message.contains("refused by the send guard");
            code = if blocked {
                EXIT_BLOCKED
            } else {
                EXIT_TRANSPORT
            };
        }
    }
    if args.json {
        match serde_json::to_string_pretty(&outcome) {
            Ok(j) => {
                let _ = writeln!(stdout, "{j}");
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        }
    }
    code
}
