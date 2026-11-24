// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt snapshot update [<path>...] [--all] [-e <NAME>] [--var <NAME=VALUE>]...` (wave 17,
//! strand B): writes the snapshot files (`rt_app::snapshots`) of requests anew, from a fresh
//! response. Only on this explicit command: `rt run` and `rt send` compare, they never write.
//!
//! A path is a request file (its snapshot is written, and the request gets the assertion if it
//! has none) or a folder (the requests below it that have a snapshot assertion); `--all`
//! takes every request with one. Each request is sent once; the files hold `***` for the
//! secret values of the environment, and a response with a credential is refused.

use std::io::Write;
use std::path::PathBuf;

use crate::{DirFiles, EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, EXIT_USAGE, SendArgs};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotArgs {
    /// Request files or folders; empty: the collection of the current folder.
    pub paths: Vec<PathBuf>,
    /// `--all`: every request that has a snapshot assertion.
    pub all: bool,
    pub environment: Option<String>,
    /// `--var NAME=VALUE`, repeatable.
    pub vars: Vec<(String, String)>,
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<SnapshotArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = SnapshotArgs::default();
    let mut action = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("all") => args.all = true,
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("var") => {
                let v = parser.value().map_err(err)?.string().map_err(err)?;
                let (k, v) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--var {v}: expected NAME=VALUE"))?;
                args.vars.push((k.to_owned(), v.to_owned()));
            }
            Value(v) if action.is_none() => action = Some(v.string().map_err(err)?),
            Value(v) => args.paths.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    match action.as_deref() {
        Some("update") => Ok(Some(args)),
        Some(other) => Err(format!("unknown snapshot command `{other}` (try `update`)")),
        None => Err("rt snapshot needs a command: `update`".into()),
    }
}

/// What one target needs: its request path and whether the request file gets the assertion.
struct Target {
    request: String,
    add_assertion: bool,
}

pub fn run(args: SnapshotArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    if args.paths.is_empty() && !args.all {
        return fail(
            stderr,
            EXIT_USAGE,
            "name the requests to update, or use --all for every request with a snapshot assertion",
        );
    }
    // The root and the targets.
    let mut root: Option<PathBuf> = None;
    let mut explicit: Vec<String> = Vec::new();
    let mut folders: Vec<Option<String>> = Vec::new();
    let sources: Vec<PathBuf> = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };
    for path in &sources {
        let (this_root, request, folder) = if path.is_file() {
            match crate::find_root(path) {
                Ok((r, request)) => (r, Some(request), None),
                Err(e) => return fail(stderr, EXIT_USAGE, &e),
            }
        } else {
            match crate::run::locate(path) {
                Ok((r, folder)) => (r, None, Some(folder)),
                Err(e) => return fail(stderr, EXIT_USAGE, &e),
            }
        };
        match &root {
            Some(r) if *r != this_root => {
                return fail(
                    stderr,
                    EXIT_USAGE,
                    "the paths belong to different collections; run the command once for each",
                );
            }
            _ => root = Some(this_root),
        }
        if let Some(request) = request {
            explicit.push(request);
        }
        if let Some(folder) = folder {
            folders.push(folder);
        }
    }
    let root = root.expect("at least one path");
    let mut doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if let Some(env) = &args.environment
        && doc.model().environment(env).is_none()
    {
        return fail(stderr, EXIT_USAGE, &format!("no environment {env:?}"));
    }

    let has_assertion = |doc: &rt_format::CollectionDoc, request: &str| {
        doc.model().request(request).is_some_and(|c| {
            c.request
                .spec
                .assertions
                .iter()
                .any(|a| a.operator.trim() == rt_core::assert::MATCHES_SNAPSHOT)
        })
    };
    let mut targets: Vec<Target> = Vec::new();
    for request in &explicit {
        if doc.model().request(request).is_none() {
            return fail(
                stderr,
                EXIT_USAGE,
                &format!("{request} is not a request of the collection"),
            );
        }
        targets.push(Target {
            request: request.clone(),
            add_assertion: !has_assertion(&doc, request),
        });
    }
    for folder in &folders {
        // A folder (or `--all`) updates the snapshots that exist.
        for request in rt_core::runner::plan(doc.model(), folder.as_deref()) {
            if has_assertion(&doc, &request) && !targets.iter().any(|t| t.request == request) {
                targets.push(Target {
                    request,
                    add_assertion: false,
                });
            }
        }
    }
    if targets.is_empty() {
        return fail(
            stderr,
            EXIT_USAGE,
            "no request with a snapshot assertion there",
        );
    }

    let files = match DirFiles::new(&root) {
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
    let send_args = SendArgs {
        environment: args.environment.clone(),
        vars: args.vars.clone(),
        prompt: false,
        ..SendArgs::default()
    };
    // The secret values of the environment, to keep them out of the files.
    let secrets: Vec<String> = match runtime.block_on(crate::secrets::resolve_all(
        doc.model(),
        args.environment.as_deref(),
        &args.vars,
        &crate::oauth::collection_id(files.root()),
    )) {
        Ok(collection) => {
            let environment = args
                .environment
                .as_deref()
                .and_then(|name| collection.environment(name));
            let mut values: Vec<String> = Vec::new();
            for v in environment
                .map(|e| e.variables.as_slice())
                .unwrap_or_default()
            {
                if !v.secret {
                    continue;
                }
                // The value from the store, or given with `--var` (which wins at send time).
                let given = args.vars.iter().find(|(n, _)| *n == v.name).map(|(_, x)| x);
                values.extend(given.cloned().or_else(|| v.value.clone()));
            }
            values.retain(|v| !v.is_empty());
            values
        }
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };

    let mut code = EXIT_OK;
    let mut new_assertions: Vec<(String, rt_core::model::RequestSpec)> = Vec::new();
    for target in &targets {
        let report = runtime.block_on(crate::send_request(
            &doc,
            &target.request,
            &send_args,
            &files,
            &transport,
        ));
        if let Some(e) = &report.error {
            code = code.max(fail(
                stderr,
                crate::exit_code(&report),
                &format!("{}: {}", target.request, error_text(e)),
            ));
            continue;
        }
        let (status, headers, body) = match rt_app::snapshots::view_of_report(&report) {
            Ok(v) => v,
            Err(e) => {
                code = code.max(fail(
                    stderr,
                    EXIT_PREPARE,
                    &format!("{}: {e}", target.request),
                ));
                continue;
            }
        };
        let spec = doc
            .model()
            .request(&target.request)
            .expect("checked")
            .request
            .spec
            .clone();
        let value = spec
            .assertions
            .iter()
            .find(|a| a.operator.trim() == rt_core::assert::MATCHES_SNAPSHOT)
            .map(|a| a.value.trim().to_owned())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| rt_app::snapshots::default_value(&target.request));
        let file = rt_app::snapshots::companion_path(&target.request, &value);
        let full = root.join(&file);
        // An existing file keeps its ignored paths and its etag choice.
        let existing = std::fs::read_to_string(&full)
            .ok()
            .and_then(|t| rt_core::snapshot::parse(&t).ok());
        let snapshot = rt_core::snapshot::freeze(
            &rt_core::snapshot::ResponseView {
                status,
                headers: &headers,
                body: &body,
            },
            &secrets,
            &rt_core::snapshot::Freeze {
                keep_etag: existing
                    .as_ref()
                    .is_some_and(|s| s.headers.contains_key("etag")),
                ignore: existing.map(|s| s.ignore).unwrap_or_default(),
            },
        );
        let text = match rt_app::snapshots::file_text(&file, &snapshot) {
            Ok(t) => t,
            Err(e) => {
                code = code.max(fail(
                    stderr,
                    EXIT_PREPARE,
                    &format!("{}: {e}", target.request),
                ));
                continue;
            }
        };
        if let Err(e) = write_file(&full, &text) {
            code = code.max(fail(
                stderr,
                EXIT_USAGE,
                &format!("cannot write {file}: {e}"),
            ));
            continue;
        }
        if target.add_assertion {
            let mut spec = spec;
            spec.assertions.push(rt_core::model::Assertion {
                expression: "res".into(),
                operator: rt_core::assert::MATCHES_SNAPSHOT.into(),
                value,
                description: None,
                enabled: true,
            });
            new_assertions.push((target.request.clone(), spec));
        }
        let _ = writeln!(stdout, "wrote {file}");
    }
    // The assertions go in at the end, one write of the request files that changed.
    if !new_assertions.is_empty() {
        let before = doc.write();
        for (request, spec) in &new_assertions {
            if let Err(e) = doc.set_request(request, spec) {
                code = code.max(fail(stderr, EXIT_PREPARE, &format!("{request}: {e}")));
            }
        }
        let after = doc.write();
        for (path, bytes) in &after.files {
            if before.files.get(path) != Some(bytes)
                && let Err(e) = write_file(&root.join(path), &String::from_utf8_lossy(bytes))
            {
                code = code.max(fail(
                    stderr,
                    EXIT_USAGE,
                    &format!("cannot write {path}: {e}"),
                ));
            }
        }
    }
    code
}

fn error_text(e: &rt_core::send::SendError) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str().map(str::to_owned)))
        .unwrap_or_else(|| format!("{e:?}"))
}

fn write_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<SnapshotArgs>, String> {
        parse(&mut lexopt::Parser::from_args(args.iter().copied()))
    }

    #[test]
    fn update_takes_paths_and_options() {
        let a = parse_args(&["update", "users/get.yml", "--all", "-e", "Local"])
            .unwrap()
            .unwrap();
        assert_eq!(a.paths, vec![PathBuf::from("users/get.yml")]);
        assert!(a.all);
        assert_eq!(a.environment.as_deref(), Some("Local"));
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["freeze"]).is_err());
    }
}
