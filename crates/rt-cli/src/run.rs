// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt run` (wave 7, strand E): runs a collection or a folder of it with
//! `rt_core::runner`, for pipelines. The same setup as `rt send` (collection, files, native
//! transport, services with plugins, OAuth, cookies and scripts), the runner's order and
//! variable carry-over, then reporters ([`crate::report`]).
//!
//! ```text
//! rt run [<folder>] [-e NAME] [--var NAME=VALUE]... [--data FILE.csv|FILE.json]
//!        [--iterations N] [--delay MS] [--bail] [--reporter NAME[=FILE]]... [--out FILE]
//!        [--no-scripts] [--plugin FILE.wasm]... [--no-plugins] [--no-prompt]
//!        [--cookie-jar FILE] [--body-limit BYTES]
//! ```
//!
//! Reporters: `text` (live on stderr, always on), `json`, `junit`. `--reporter junit=FILE`
//! writes to FILE; `--out FILE` is the same for the reporter just before it. Without a file a
//! reporter writes to stdout; only one may.
//!
//! Exit codes: `EXIT_OK` all passed, `EXIT_TESTS` a request, test or assertion failed,
//! `EXIT_USAGE` usage error (also: nothing to run, a report that cannot be written),
//! `EXIT_PREPARE` external secrets or plugins could not be set up, `EXIT_TRANSPORT` no
//! network stack, `EXIT_BLOCKED` the distribution's send guard refused a request (wave 10;
//! before `EXIT_TESTS`).

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_core::model::{Collection, Item};
use rt_core::prepare::PrepareInput;
use rt_core::runner::{RunEvent, RunPlan, RunStep, RunSummary};
use rt_core::send::{SendError, SendOptions};
use rt_format::COLLECTION_FILE;

use crate::report::{self, RunInfo};
use crate::{
    DirFiles, EXIT_OK, EXIT_PREPARE, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, SendArgs, secrets,
    services,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReporterKind {
    Text,
    Json,
    Junit,
}

impl ReporterKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "junit" => Some(Self::Junit),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Junit => "junit",
        }
    }
}

/// `--reporter NAME[=FILE]`. `out: None`: stdout (`text`: stderr only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reporter {
    pub kind: ReporterKind,
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// A folder inside a collection or the collection root; default: the current folder.
    pub path: Option<PathBuf>,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    /// `--data FILE`: CSV or JSON rows, one iteration per row.
    pub data: Option<PathBuf>,
    /// `--iterations N`: iterations without data; with data, N rows (cycling).
    pub iterations: Option<u32>,
    pub delay_ms: u64,
    pub bail: bool,
    /// In the order given; the live text on stderr is always on.
    pub reporters: Vec<Reporter>,
    pub body_limit: usize,
    pub plugins: Vec<PathBuf>,
    pub no_plugins: bool,
    pub cookie_jar: Option<PathBuf>,
    pub prompt: bool,
    pub no_scripts: bool,
    /// Wave 10: the distribution's send guard (never from the command line).
    pub guard: rt_core::guard::GuardHandle,
    /// Wave 17: the distribution's exchange observers (never from the command line).
    pub observers: rt_core::observe::ObserverSet,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            path: None,
            environment: None,
            vars: Vec::new(),
            data: None,
            iterations: None,
            delay_ms: 0,
            bail: false,
            reporters: Vec::new(),
            body_limit: SendOptions::default().body_limit,
            plugins: Vec::new(),
            no_plugins: false,
            cookie_jar: None,
            prompt: true,
            no_scripts: false,
            guard: Default::default(),
            observers: Default::default(),
        }
    }
}

impl RunArgs {
    /// The options every request is sent with, as `rt send` takes them.
    pub fn send_args(&self) -> SendArgs {
        SendArgs {
            file: PathBuf::new(),
            environment: self.environment.clone(),
            vars: self.vars.clone(),
            body_limit: self.body_limit,
            pretty: false,
            plugins: self.plugins.clone(),
            no_plugins: self.no_plugins,
            cookie_jar: self.cookie_jar.clone(),
            prompt: self.prompt,
            no_scripts: self.no_scripts,
            session: Default::default(),
            guard: self.guard.clone(),
            observers: self.observers.clone(),
            caller: rt_core::guard::Caller::Cli,
        }
    }
}

/// `Ok(None)` for `--help`.
pub(crate) fn parse(parser: &mut lexopt::Parser) -> Result<Option<RunArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = RunArgs::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
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
            Long("data") => args.data = Some(parser.value().map_err(err)?.into()),
            Long("iterations") => {
                let n: u32 = parser.value().map_err(err)?.parse().map_err(err)?;
                if n == 0 {
                    return Err("--iterations must be at least 1".into());
                }
                args.iterations = Some(n);
            }
            Long("delay") => args.delay_ms = parser.value().map_err(err)?.parse().map_err(err)?,
            Long("bail") => args.bail = true,
            Long("reporter") => {
                let spec = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, out) = match spec.split_once('=') {
                    Some((_, "")) => {
                        return Err(format!("--reporter {spec}: missing file name"));
                    }
                    Some((name, file)) => (name, Some(PathBuf::from(file))),
                    None => (spec.as_str(), None),
                };
                let kind = ReporterKind::parse(name).ok_or_else(|| {
                    format!("unknown reporter {name:?} (expected text, json or junit)")
                })?;
                args.reporters.push(Reporter { kind, out });
            }
            Long("out") => {
                let file = PathBuf::from(parser.value().map_err(err)?);
                let last = args
                    .reporters
                    .last_mut()
                    .ok_or("--out must follow a --reporter")?;
                if let Some(already) = &last.out {
                    return Err(format!(
                        "--out: reporter {} already writes to {}",
                        last.kind.name(),
                        already.display()
                    ));
                }
                last.out = Some(file);
            }
            Long("body-limit") => {
                args.body_limit = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("plugin") => args.plugins.push(parser.value().map_err(err)?.into()),
            Long("no-plugins") => args.no_plugins = true,
            Long("cookie-jar") => args.cookie_jar = Some(parser.value().map_err(err)?.into()),
            Long("no-prompt") => args.prompt = false,
            Long("no-scripts") => args.no_scripts = true,
            Value(v) if args.path.is_none() => args.path = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    let to_stdout = args
        .reporters
        .iter()
        .filter(|r| r.kind != ReporterKind::Text && r.out.is_none())
        .count();
    if to_stdout > 1 {
        return Err(
            "only one reporter can write to stdout; give the others a file (--reporter junit=FILE)"
                .into(),
        );
    }
    let mut files: Vec<&PathBuf> = args
        .reporters
        .iter()
        .filter_map(|r| r.out.as_ref())
        .collect();
    files.sort();
    if let Some(w) = files.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!(
            "two reporters write to the same file {}",
            w[0].display()
        ));
    }
    Ok(Some(args))
}

/// `EXIT_BLOCKED` if the send guard refused a request, `EXIT_TESTS` if a request, test or
/// assertion failed, else `EXIT_OK`.
pub fn exit_code(summary: &RunSummary, steps: &[RunStep]) -> i32 {
    if steps
        .iter()
        .any(|s| matches!(s.report.error, Some(SendError::Blocked { .. })))
    {
        return crate::EXIT_BLOCKED;
    }
    let failed = summary.failed > 0
        || summary.tests_failed > 0
        || summary.assertions_failed > 0
        || steps.iter().any(|s| !s.passed);
    if failed { EXIT_TESTS } else { EXIT_OK }
}

/// The collection root above (or at) `dir` and the folder `dir` is, relative to the root
/// (`None`: the root itself).
pub(crate) fn locate(dir: &Path) -> Result<(PathBuf, Option<String>), String> {
    let dir =
        std::fs::canonicalize(dir).map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    if dir.is_file() {
        return Err(format!(
            "{} is a file: rt run takes a folder (use rt send <request-file> for one request)",
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

/// Data rows for the plan: the file's rows; with `--iterations N`, N rows, cycling.
fn data_rows(args: &RunArgs) -> Result<Vec<rt_core::runner::DataRow>, String> {
    let Some(file) = &args.data else {
        return Ok(Vec::new());
    };
    let bytes = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rows = rt_core::runner::parse_data(&name, &bytes)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    if rows.is_empty() {
        return Err(format!("{}: no data rows", file.display()));
    }
    Ok(match args.iterations {
        Some(n) => rows.iter().cycle().take(n as usize).cloned().collect(),
        None => rows,
    })
}

fn method_of(collection: &Collection, step: &RunStep) -> String {
    match &step.report.request {
        Some(r) => r.method.clone(),
        None => collection
            .request(&step.request_path)
            .map(|c| c.request.spec.method.clone())
            .unwrap_or_default(),
    }
}

pub fn run(args: RunArgs, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
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
    let data = match data_rows(&args) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let requests = rt_core::runner::plan(model, folder.as_deref());
    if requests.is_empty() {
        let place = folder.as_deref().map_or_else(
            || root.display().to_string(),
            |f| format!("{f} ({})", root.display()),
        );
        return fail(stderr, EXIT_USAGE, &format!("no requests in {place}"));
    }
    let plan = RunPlan {
        requests,
        iterations: if data.is_empty() {
            args.iterations.unwrap_or(1)
        } else {
            1
        },
        data,
        delay_ms: args.delay_ms,
        bail: args.bail,
    };
    let info = RunInfo {
        collection: if model.name.is_empty() {
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            model.name.clone()
        },
        iterations: if plan.data.is_empty() {
            plan.iterations
        } else {
            plan.data.len() as u32
        },
    };

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
    let send_args = args.send_args();
    let collection = match runtime.block_on(secrets::resolve_all(
        model,
        args.environment.as_deref(),
        &args.vars,
        &crate::oauth::collection_id(files.root()),
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let cli = match services::CliServices::new(&send_args, files.root()) {
        Ok(s) => s,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let input = PrepareInput {
        environment: args.environment.as_deref(),
        globals: Vec::new(),
        runtime: args.vars.clone(),
    };
    let options = SendOptions {
        body_limit: args.body_limit,
        iteration: None,
    };

    let mut text = report::text::started(&info, plan.requests.len());
    let _ = stderr.write_all(text.as_bytes());
    let mut steps: Vec<RunStep> = Vec::new();
    let summary = {
        let mut last_iteration = None;
        let mut on_event = |event: RunEvent| {
            let RunEvent::Step { step } = event else {
                return;
            };
            let mut chunk = String::new();
            if info.iterations > 1 && last_iteration != Some(step.iteration) {
                chunk.push_str(&report::text::iteration(step.iteration, info.iterations));
                last_iteration = Some(step.iteration);
            }
            chunk.push_str(&report::text::step(&step, &method_of(&collection, &step)));
            let _ = stderr.write_all(chunk.as_bytes());
            let _ = stderr.flush();
            text.push_str(&chunk);
            steps.push(*step);
        };
        runtime.block_on(rt_core::runner::run(
            &collection,
            &plan,
            &input,
            &files,
            &transport,
            &options,
            &cli.services(),
            &mut on_event,
            &|| false,
        ))
    };
    let end = report::text::summary(&summary);
    let _ = stderr.write_all(end.as_bytes());
    text.push_str(&end);

    let mut code = exit_code(&summary, &steps);
    for r in &args.reporters {
        let content = match r.kind {
            ReporterKind::Text if r.out.is_none() => continue,
            ReporterKind::Text => text.clone(),
            ReporterKind::Json => report::json(&summary, &steps),
            ReporterKind::Junit => report::junit(&info, &summary, &steps),
        };
        match &r.out {
            None => {
                let _ = stdout.write_all(content.as_bytes());
                let _ = stdout.flush();
            }
            Some(file) => {
                if let Err(e) = write_file(file, &content) {
                    code = fail(
                        stderr,
                        EXIT_USAGE,
                        &format!(
                            "cannot write the {} report {}: {e}",
                            r.kind.name(),
                            file.display()
                        ),
                    );
                }
            }
        }
    }
    code
}

/// Writes a report file, creating missing parent folders (`--reporter junit=out/junit.xml`).
fn write_file(file: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_root_and_folder() {
        let dir = std::env::temp_dir().join(format!("rt-run-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("col/a/b")).unwrap();
        std::fs::write(dir.join("col").join(COLLECTION_FILE), "x").unwrap();
        std::fs::write(dir.join("col/a/r.yml"), "x").unwrap();
        let root = std::fs::canonicalize(dir.join("col")).unwrap();
        assert_eq!(locate(&dir.join("col")).unwrap(), (root.clone(), None));
        assert_eq!(
            locate(&dir.join("col/a/b")).unwrap(),
            (root, Some("a/b".into()))
        );
        assert!(locate(&dir).is_err());
        assert!(
            locate(&dir.join("col/a/r.yml"))
                .unwrap_err()
                .contains("rt send")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
