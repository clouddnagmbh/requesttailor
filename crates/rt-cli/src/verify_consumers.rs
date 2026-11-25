// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt verify-consumers <provider-collection> --base-url <url> [--consumer N]... [--out
//! text|json|github] [--fetch] [--fail-fast] [--trust-scripts] [--allow-writes]` (wave 19,
//! strand F): runs the consumer collections listed in the provider's `opencollection.yml`
//! (`extensions.requesttailor.consumers`) against the built provider at `--base-url` and
//! names the consumer and request that breaks. Exit `EXIT_CONSUMERS` (8) when one breaks.
//!
//! Each consumer is a collection found on disk (a path relative to the provider, or a shallow
//! clone, see `rt_app::consumers::fetch`), run by the same runner as `rt run` with `baseUrl`
//! set to `--base-url`. Scripts run only with `--trust-scripts`, writing requests only with
//! `--allow-writes` and never against production. Exit codes: `EXIT_CONSUMERS` (8) when a
//! consumer breaks or cannot be verified, `EXIT_BLOCKED` (5) when the send guard refused,
//! `EXIT_USAGE` (2) when the run could not start.

use std::io::Write;
use std::path::PathBuf;

use rt_app::consumers::{
    Budget, ConsumerReport, ConsumerVerifyRequest, Loaded, fetch, run_consumer,
};
use rt_core::send::SendOptions;
use rt_format::FileSet;

use crate::{EXIT_OK, EXIT_TRANSPORT, EXIT_USAGE, secrets, services};

/// Exit code of `rt verify-consumers` when at least one consumer breaks (wave 19).
pub const EXIT_CONSUMERS: i32 = 8;

pub const USAGE: &str = "  rt verify-consumers <provider-collection> --base-url <url> [--consumer <name>]...\n            [--out text|json|github] [--fetch] [--fail-fast] [--trust-scripts] [--allow-writes] [--max-time <minutes>]\n";

pub const HELP: &str = "rt verify-consumers runs the consumer collections your provider lists\n(extensions.requesttailor.consumers in opencollection.yml: a local path or a Git URL, an\nenvironment, variables) against the provider you built, at --base-url, and reports which\nconsumer and request breaks and how. Remote consumers are cloned shallowly (natively\nonly; a token goes in RT_GIT_TOKEN). Scripts run only with --trust-scripts; writing\nrequests only with --allow-writes and never against production. Exit code 8 when a\nconsumer breaks, 5 when the send guard refused. No broker, no upload.\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
    /// GitHub Actions workflow commands.
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub provider: PathBuf,
    pub base_url: String,
    pub only: Vec<String>,
    pub format: Format,
    pub fetch: bool,
    pub fail_fast: bool,
    pub trust_scripts: bool,
    pub allow_writes: bool,
    /// `--max-time MINUTES` (default 10, at most 60).
    pub max_minutes: Option<u32>,
    /// The distribution's send guard (never from the command line; `with_guard` sets it).
    pub guard: rt_core::guard::GuardHandle,
    /// The distribution's exchange observers (never from the command line).
    pub observers: rt_core::observe::ObserverSet,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut provider = None;
    let mut args = Args {
        provider: PathBuf::from("."),
        base_url: String::new(),
        only: Vec::new(),
        format: Format::Text,
        fetch: false,
        fail_fast: false,
        trust_scripts: false,
        allow_writes: false,
        max_minutes: None,
        guard: Default::default(),
        observers: Default::default(),
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("base-url") => {
                args.base_url = parser.value().map_err(err)?.string().map_err(err)?
            }
            Long("consumer") => args
                .only
                .push(parser.value().map_err(err)?.string().map_err(err)?),
            Long("out") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "github" => Format::Github,
                    other => {
                        return Err(format!("unknown output {other:?} (text, json or github)"));
                    }
                };
            }
            Long("fetch") => args.fetch = true,
            Long("keep-going") => args.fail_fast = false,
            Long("fail-fast") => args.fail_fast = true,
            Long("trust-scripts") => args.trust_scripts = true,
            Long("allow-writes") => args.allow_writes = true,
            Long("max-time") => {
                args.max_minutes = Some(parser.value().map_err(err)?.parse().map_err(err)?)
            }
            Value(v) if provider.is_none() => provider = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if let Some(p) = provider {
        args.provider = p;
    }
    if args.base_url.is_empty() {
        return Err("verify-consumers: --base-url is required".into());
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, folder) = match crate::run::locate(&args.provider) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if folder.is_some() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!(
                "{} is a folder inside a collection: give the collection root {}",
                args.provider.display(),
                root.display()
            ),
        );
    }
    let provider_files = match FileSet::from_dir(&root) {
        Ok(f) => f,
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{}: {e}", root.display())),
    };
    let listed = match rt_app::consumers::list(&provider_files) {
        Ok(l) => l,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
    };
    if listed.is_empty() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!(
                "{} lists no consumers (extensions.requesttailor.consumers in opencollection.yml)",
                root.display()
            ),
        );
    }
    let chosen = match rt_app::consumers::select(&listed, &args.only) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
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
    let request = ConsumerVerifyRequest {
        base_url: args.base_url.trim_end_matches('/').to_owned(),
        only: args.only.clone(),
        fetch: args.fetch,
        keep_going: !args.fail_fast,
        trust_scripts: args.trust_scripts,
        allow_writes: args.allow_writes,
        max_minutes: args.max_minutes,
    };
    let data_dir = rt_plugin::host::dir::data_dir();
    let token = std::env::var("RT_GIT_TOKEN").ok().filter(|t| !t.is_empty());
    let options = SendOptions {
        body_limit: SendOptions::default().body_limit,
        iteration: None,
    };
    let mut budget = Budget::new(args.max_minutes);
    let mut report = ConsumerReport::default();
    for consumer in chosen {
        let _ = writeln!(stderr, "rt: verifying consumer {} ...", consumer.name);
        let prepared = prepare(
            &runtime,
            consumer,
            &root,
            data_dir.as_deref(),
            &request,
            token.as_deref(),
        );
        let result = match prepared {
            Ok((consumer_root, loaded)) => {
                let run_args = crate::run::RunArgs {
                    prompt: false,
                    no_scripts: !args.trust_scripts,
                    guard: args.guard.clone(),
                    observers: args.observers.clone(),
                    ..Default::default()
                };
                let send_args = run_args.send_args();
                match services::CliServices::new(&send_args, &consumer_root) {
                    Ok(cli) => runtime.block_on(run_consumer(
                        consumer,
                        loaded,
                        &request,
                        &mut budget,
                        &transport,
                        &options,
                        &cli.services(),
                        &|| false,
                    )),
                    Err(e) => error_result(consumer, &e),
                }
            }
            Err(e) => error_result(consumer, &e),
        };
        let broke = !result.failures.is_empty() || result.error.is_some();
        report.consumers.push(result);
        if broke && args.fail_fast {
            break;
        }
    }
    let output = match args.format {
        Format::Text => rt_app::consumers::text(&report),
        Format::Github => rt_app::consumers::github(&report),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(mut j) => {
                j.push('\n');
                j
            }
            Err(e) => return fail(stderr, EXIT_USAGE, &e.to_string()),
        },
    };
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    if args.format == Format::Github {
        // The annotations carry the findings; a short line tells the log reader the verdict.
        let _ = write!(stderr, "{}", rt_app::consumers::text(&report));
    }
    exit_code(&report)
}

fn error_result(
    consumer: &rt_app::consumers::ConsumerRef,
    error: &str,
) -> rt_app::consumers::ConsumerResult {
    rt_app::consumers::ConsumerResult {
        name: consumer.name.clone(),
        commit: None,
        requests: 0,
        skipped: 0,
        failures: Vec::new(),
        error: Some(error.to_owned()),
    }
}

/// Finds and reads one consumer: its folder (for services) and the loaded collection.
fn prepare(
    runtime: &tokio::runtime::Runtime,
    consumer: &rt_app::consumers::ConsumerRef,
    provider_root: &std::path::Path,
    data_dir: Option<&std::path::Path>,
    request: &ConsumerVerifyRequest,
    token: Option<&str>,
) -> Result<(PathBuf, Loaded), String> {
    let found = fetch::fetch(consumer, provider_root, data_dir, request.fetch, token)?;
    let files = FileSet::from_dir(&found.root).map_err(|e| {
        format!(
            "consumer {}: cannot read {}: {e}",
            consumer.name,
            found.root.display()
        )
    })?;
    let doc = rt_format::read_collection(&files)
        .map_err(|e| format!("consumer {}: {e}", consumer.name))?;
    let model = doc.model();
    let given: Vec<(String, String)> = Vec::new();
    let collection = runtime
        .block_on(secrets::resolve_all(
            model,
            Some(&consumer.environment),
            &given,
            &crate::oauth::collection_id(&found.root),
        ))
        .map_err(|e| format!("consumer {}: {e}", consumer.name))?;
    Ok((
        found.root,
        Loaded {
            collection,
            files,
            commit: found.commit,
        },
    ))
}

/// 5 when the send guard refused a request, 8 when a consumer breaks or could not be
/// verified, else 0.
pub fn exit_code(report: &ConsumerReport) -> i32 {
    if report
        .consumers
        .iter()
        .any(|c| c.failures.iter().any(|f| f.assertion == "send guard"))
    {
        return crate::EXIT_BLOCKED;
    }
    if report.broken() {
        EXIT_CONSUMERS
    } else {
        EXIT_OK
    }
}
