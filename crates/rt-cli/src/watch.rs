// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand D: `rt send <file> --watch`.
//!
//! Sends the request, then again whenever something it is made of changes, and prints one line
//! per send: status, duration, size and what differs from the previous answer (`same`,
//! `changed: $.items[1].price`). With `--json` every send is one JSON line, the
//! [`SendReport`]. Ctrl-C ends the command with the exit code of the last send.
//!
//! What is watched: the request file, the body file it reads, `opencollection.yml`, the
//! `folder.yml` of every folder above it, the chosen environment's file and the companion
//! files next to the request (`<name>.*`, e.g. its snapshot). The modification times are
//! polled every [`POLL`], so there is no file-watcher dependency and it behaves the same on
//! every target. A change that is still being written is given [`SETTLE`] to finish.
//!
//! Safety: only reading requests are sent. A writing request needs `--allow-writes`, and is
//! never sent while the environment is a production system (`rt_app::agents::is_production`):
//! `rt` refuses to start then, or, if the file changes into one while watching, skips that
//! send with an `error:` line.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rt_app::agents::is_production;
use rt_core::guard::{Effect, effect};
use rt_core::model::{Body, Collection};
use rt_core::send::{ReportBody, SendReport};
use serde_json::Value;

use crate::report::error_message;
use crate::send_banner::{add_note, banner_for, duration, timing_line};
use crate::{
    DirFiles, EXIT_OK, EXIT_TRANSPORT, EXIT_USAGE, SendArgs, exit_code, find_root, load_collection,
    render, send_request,
};

/// How often the modification times are read.
pub const POLL: Duration = Duration::from_millis(250);
/// After a change, how long to wait for a writer to finish before sending.
pub const SETTLE: Duration = Duration::from_millis(40);

/// Options of `--watch`, `--allow-writes`, `--timing` and `--json` of `rt send`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchArgs {
    pub watch: bool,
    pub allow_writes: bool,
    /// Also print the timing line (`rt send` without `--watch` too).
    pub timing: bool,
    /// One JSON report per line instead of the text line (with `--watch`).
    pub json: bool,
}

type Fingerprint = BTreeMap<PathBuf, Option<(SystemTime, u64)>>;

fn fingerprint(files: &[PathBuf]) -> Fingerprint {
    files
        .iter()
        .map(|f| {
            let stamp = std::fs::metadata(f)
                .ok()
                .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
            (f.clone(), stamp)
        })
        .collect()
}

/// The files the send of `request_path` depends on.
pub fn watched_files(
    root: &Path,
    request_path: &str,
    collection: &Collection,
    environment: Option<&str>,
) -> Vec<PathBuf> {
    let mut files = vec![root.join(request_path), root.join("opencollection.yml")];
    let mut folder = Path::new(request_path).parent();
    while let Some(dir) = folder.filter(|d| !d.as_os_str().is_empty()) {
        files.push(root.join(dir).join("folder.yml"));
        folder = dir.parent();
    }
    if let Some(env) = environment {
        let file = root.join("environments").join(format!("{env}.yml"));
        if file.exists() {
            files.push(file);
        } else if let Ok(dir) = std::fs::read_dir(root.join("environments")) {
            files.extend(dir.flatten().map(|e| e.path()));
        }
    }
    if let Some(context) = collection.request(request_path)
        && let Body::File { path, .. } = &context.request.spec.body
        && !path.split(['/', '\\']).any(|part| part == "..")
    {
        files.push(root.join(path));
    }
    files.extend(companions(root, request_path));
    files.sort();
    files.dedup();
    files
}

/// Companion files: `<stem>.<anything>` next to the request. A new one counts as a change, so
/// the list is read again on every poll.
fn companions(root: &Path, request_path: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let request_file = root.join(request_path);
    if let (Some(dir), Some(stem)) = (request_file.parent(), request_file.file_stem())
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let prefix = format!("{}.", stem.to_string_lossy());
        for entry in entries.flatten() {
            let path = entry.path();
            if path != request_file
                && path.is_file()
                && entry.file_name().to_string_lossy().starts_with(&prefix)
            {
                files.push(path);
            }
        }
    }
    files
}

/// Why `collection`'s request must not be sent while watching, or `None`.
fn refusal(
    collection: &Collection,
    request_path: &str,
    environment: Option<&str>,
    allow_writes: bool,
) -> Option<String> {
    let context = collection.request(request_path)?;
    let spec = &context.request.spec;
    if !spec.protocol.is_http() {
        return Some("--watch sends HTTP requests, not gRPC or WebSocket sessions".into());
    }
    if effect(spec) == Effect::Write {
        if is_production(&collection.config.agents, environment) {
            return Some(format!(
                "--watch never sends a writing request ({}) to a production environment",
                spec.method
            ));
        }
        if !allow_writes {
            return Some(format!(
                "--watch sends only reading requests; this one is {}. Add --allow-writes to send it on every change",
                spec.method
            ));
        }
    }
    None
}

/// The JSON paths whose values differ, `$.items[1].price`, at most `limit` and the number
/// left over.
pub fn changed_paths(old: &Value, new: &Value, limit: usize) -> (Vec<String>, usize) {
    fn walk(path: &str, old: &Value, new: &Value, out: &mut Vec<String>) {
        match (old, new) {
            (Value::Object(a), Value::Object(b)) => {
                let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
                keys.sort();
                keys.dedup();
                for key in keys {
                    let child = format!("{path}.{key}");
                    match (a.get(key), b.get(key)) {
                        (Some(x), Some(y)) => walk(&child, x, y, out),
                        _ => out.push(child),
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                for i in 0..a.len().max(b.len()) {
                    let child = format!("{path}[{i}]");
                    match (a.get(i), b.get(i)) {
                        (Some(x), Some(y)) => walk(&child, x, y, out),
                        _ => out.push(child),
                    }
                }
            }
            _ if old != new => out.push(path.to_owned()),
            _ => {}
        }
    }
    let mut all = Vec::new();
    walk("$", old, new, &mut all);
    let more = all.len().saturating_sub(limit);
    all.truncate(limit);
    (all, more)
}

/// What the previous answer was, to say what changed.
#[derive(Default)]
struct Previous {
    status: Option<u16>,
    body: Option<String>,
}

fn body_text(report: &SendReport) -> Option<String> {
    match &report.response.as_ref()?.body {
        ReportBody::Utf8(text) => Some(text.clone()),
        ReportBody::Base64(data) => Some(data.clone()),
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        _ if bytes < 1024 * 1024 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0),
    }
}

/// The one line of a send: `200 12 ms  34 B  changed: $.name`.
fn line(report: &SendReport, previous: &Previous) -> String {
    if let Some(error) = &report.error {
        return format!("error: {}", error_message(error));
    }
    let Some(response) = &report.response else {
        return "error: no response".into();
    };
    let ms = response.total.as_secs_f64() * 1000.0;
    let body = body_text(report);
    let mut difference = Vec::new();
    let what = match (&previous.status, &previous.body) {
        (None, _) => "first response".to_owned(),
        (Some(status), old) => {
            if *status != response.status {
                difference.push(format!("status {status} -> {}", response.status));
            }
            if old != &body {
                let json = |t: &Option<String>| {
                    t.as_deref()
                        .and_then(|t| serde_json::from_str::<Value>(t).ok())
                };
                match (json(old), json(&body)) {
                    (Some(a), Some(b)) => {
                        let (paths, more) = changed_paths(&a, &b, 5);
                        let mut text = paths.join(", ");
                        if more > 0 {
                            text.push_str(&format!(" and {more} more"));
                        }
                        difference.push(text);
                    }
                    _ => difference.push("body".into()),
                }
            }
            if difference.is_empty() {
                "same".to_owned()
            } else {
                format!("changed: {}", difference.join("; "))
            }
        }
    };
    format!(
        "{} {}  {}  {what}",
        response.status,
        duration(ms),
        size(response.size)
    )
}

/// Runs `rt send --watch`. Returns the exit code.
pub fn run(args: SendArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, message: &str| {
        let _ = writeln!(stderr, "rt: {message}");
        code
    };
    let (root, request_path) = match find_root(&args.file) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let environment = args.environment.clone();
    if doc.model().request(&request_path).is_none() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("request not found: {request_path}"),
        );
    }
    if let Some(why) = refusal(
        doc.model(),
        &request_path,
        environment.as_deref(),
        args.watch.allow_writes,
    ) {
        return fail(stderr, EXIT_USAGE, &why);
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
    if let Some(text) = banner_for(doc.model(), &request_path, environment.as_deref()) {
        let _ = writeln!(stderr, "rt: {text}");
    }
    let _ = writeln!(stderr, "rt: watching {request_path}; Ctrl-C to stop");

    runtime.block_on(async {
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        // Poll once so the handler is installed before the first send.
        tokio::select! {
            biased;
            _ = &mut ctrl_c => return EXIT_OK,
            () = std::future::ready(()) => {}
        }
        let mut previous = Previous::default();
        let mut code: i32;
        let mut current = Some(doc);
        loop {
            // Read the files first: a change while sending sends again.
            let doc = match current.take() {
                Some(d) => Ok(d),
                None => load_collection(&root),
            };
            let (watched, outcome) = match doc {
                Err(message) => (
                    watched_files(
                        &root,
                        &request_path,
                        &Collection::default(),
                        environment.as_deref(),
                    ),
                    Err(message),
                ),
                Ok(doc) => {
                    let watched =
                        watched_files(&root, &request_path, doc.model(), environment.as_deref());
                    (watched, Ok(doc))
                }
            };
            let stamp = fingerprint(&watched);
            match outcome {
                Err(message) => {
                    let _ = writeln!(stdout, "error: {message}");
                    code = EXIT_USAGE;
                }
                Ok(doc) => {
                    let allowed = refusal(
                        doc.model(),
                        &request_path,
                        environment.as_deref(),
                        args.watch.allow_writes,
                    );
                    if let Some(why) = allowed {
                        let _ = writeln!(stdout, "error: {why}");
                        code = EXIT_USAGE;
                    } else {
                        let mut report =
                            send_request(&doc, &request_path, &args, &files, &transport).await;
                        if let Some(text) =
                            banner_for(doc.model(), &request_path, environment.as_deref())
                        {
                            add_note(&mut report, &text);
                        }
                        if args.watch.json {
                            let _ = stdout.write_all(render(&report, false).as_bytes());
                        } else {
                            let _ = writeln!(stdout, "{}", line(&report, &previous));
                        }
                        let _ = stdout.flush();
                        if args.watch.timing
                            && let Some(t) = timing_line(&report)
                        {
                            let _ = writeln!(stderr, "{t}");
                        }
                        previous = Previous {
                            status: report.response.as_ref().map(|r| r.status),
                            body: body_text(&report),
                        };
                        code = exit_code(&report);
                    }
                }
            }
            // Wait for a change or Ctrl-C.
            let now = || {
                let mut all = watched.clone();
                all.extend(companions(&root, &request_path));
                all.sort();
                all.dedup();
                fingerprint(&all)
            };
            let interrupted = wait_for_change(&now, &stamp, &mut ctrl_c).await;
            if interrupted {
                return code;
            }
        }
    })
}

/// `true` on Ctrl-C, `false` when a watched file changed (and settled).
async fn wait_for_change(
    now: &dyn Fn() -> Fingerprint,
    stamp: &Fingerprint,
    mut ctrl_c: impl std::future::Future + Unpin,
) -> bool {
    loop {
        tokio::select! {
            _ = &mut ctrl_c => return true,
            () = tokio::time::sleep(POLL) => {}
        }
        if &now() != stamp {
            // Let a writer finish: wait until two reads agree.
            let mut last = now();
            loop {
                tokio::select! {
                    _ = &mut ctrl_c => return true,
                    () = tokio::time::sleep(SETTLE) => {}
                }
                let current = now();
                if current == last {
                    return false;
                }
                last = current;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changed_paths_name_the_differences() {
        let a = json!({"id": 1, "items": [{"p": 1}, {"p": 2}], "gone": true});
        let b = json!({"id": 1, "items": [{"p": 1}, {"p": 3}, 4], "new": 1});
        let (paths, more) = changed_paths(&a, &b, 10);
        assert_eq!(paths, ["$.gone", "$.items[1].p", "$.items[2]", "$.new"]);
        assert_eq!(more, 0);
        let (paths, more) = changed_paths(&a, &b, 2);
        assert_eq!((paths.len(), more), (2, 2));
        assert_eq!(changed_paths(&a, &a, 5), (vec![], 0));
    }
}
