// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner (wave 7, strand D): runs the requests of a folder or the whole
//! collection in order, optionally several iterations with a data row each (CSV or JSON),
//! carrying variable changes from one request to the next. The app, the web app and
//! `rt run` (strand E) use this one implementation.
//!
//! # Variables in a run
//!
//! Every send of the run gets this runtime layer (highest precedence of the scope, later
//! entries win):
//!
//! 1. the iteration's data row ([`Iteration::data`]; strings as they are, other JSON values as
//!    their JSON text, e.g. `1`, `true`, `null`, `{"a":1}`),
//! 2. the caller's runtime variables (`input.runtime`: `rt run --var`, the app's variables),
//! 3. the run's own changes: every [`VarChange`] of earlier steps (scripts and variable
//!    actions), in order, kept for the whole run (all iterations). A change with value `None`
//!    removes the run's value of that name.
//!
//! The scope a script names (`runtime`, `environment`, `collection`, `global`) does not
//! matter here: all changes go into the runtime layer, in memory, for this run only. Nothing
//! reaches files or the app's session. So a run change wins over a data row of a later
//! iteration with the same name, and unsetting a name the environment defines makes the
//! environment's value visible again.
//!
//! The changes are carried with their real values: the report masks secret values
//! ([`SendReport::var_changes`]), the runner takes the values from the script runner itself.
//!
//! # Runner control
//!
//! [`SendReport::flow`] of a step: `Skip` (from `rt.runner.skip()` in a `before-request`
//! script) makes the step skipped, nothing is sent; `Next { request: Some(r) }` continues the
//! current iteration with the first request of the plan whose path is `r`, or else whose
//! name is `r` (jumping back is allowed; cancel stops a loop); an unknown `r` stops the run
//! with a note (source `runner`) on that step's report; `Next { request: None }` stops the
//! run. A stopped run still emits [`RunEvent::Finished`].

use std::cell::RefCell;

use serde::{Deserialize, Serialize};

use crate::model::{Collection, Item};
use crate::prepare::{FileSource, PrepareInput};
use crate::script::TestResult;
use crate::script::{AssertionResult, Flow, Iteration, ScriptCall, ScriptOutcome, VarChange};
use crate::send::{SendOptions, SendReport, send_with};
use crate::services::{LocalBoxFuture, Note, NoteLevel, ScriptRunner, Services};
use crate::transport::Transport;

/// One data row: column (CSV) or key (JSON) and value, in file order.
pub type DataRow = Vec<(String, serde_json::Value)>;

/// What to run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunPlan {
    /// Request paths in run order (see [`plan`]).
    pub requests: Vec<String>,
    /// Data rows; one iteration per row. Empty: `iterations` iterations without data.
    #[serde(default)]
    pub data: Vec<Vec<(String, serde_json::Value)>>,
    /// Iterations when there is no data (at least 1).
    #[serde(default = "one")]
    pub iterations: u32,
    /// Pause between requests.
    #[serde(default)]
    pub delay_ms: u64,
    /// Stop at the first failed request, test or assertion.
    #[serde(default)]
    pub bail: bool,
    /// Wave 18: waiting for rate limits (`rt run --respect-limits`). Off by default; it adds to
    /// nothing: with `delay_ms` the longer wait applies.
    #[serde(
        default,
        skip_serializing_if = "crate::ratelimit::Throttle::is_default"
    )]
    pub throttle: crate::ratelimit::Throttle,
}

fn one() -> u32 {
    1
}

impl RunPlan {
    /// Iterations this plan runs: one per data row, else `iterations` (at least 1).
    pub fn iteration_count(&self) -> u32 {
        if self.data.is_empty() {
            self.iterations.max(1)
        } else {
            u32::try_from(self.data.len()).unwrap_or(u32::MAX)
        }
    }
}

/// One request of the run, as it finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStep {
    /// 0-based iteration.
    pub iteration: u32,
    pub request_path: String,
    pub request_name: String,
    /// The full report (as `rt send` prints it).
    pub report: SendReport,
    /// No error, every test and assertion passed. A skipped request counts as passed.
    pub passed: bool,
    pub skipped: bool,
}

/// Progress, for the app's result view and the CLI's live output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RunEvent {
    /// First event. `total`: requests × iterations as planned (`setNext` and stopping may
    /// make the run shorter or longer).
    Started { total: u32 },
    /// A request is about to be sent.
    Request {
        iteration: u32,
        request_path: String,
    },
    /// A request finished (also when skipped or failed).
    Step { step: Box<RunStep> },
    /// Wave 18: the runner waits for a rate limit (`RunPlan.throttle`) before sending
    /// `request_path` again. `until` is Unix milliseconds; `reason` is a sentence for the
    /// user, e.g. `rate limit: 0 left, resets in 12 s`.
    Waiting {
        request_path: String,
        until: i64,
        reason: String,
    },
    /// Last event, always, also after bail, cancel or `setNext(null)`.
    Finished { summary: RunSummary },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunSummary {
    /// Steps run: `passed + failed + skipped`.
    pub requests: u32,
    /// Steps that passed and were not skipped.
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub assertions_passed: u32,
    pub assertions_failed: u32,
    /// Wall time of the run.
    pub duration_ms: u64,
    /// The run was stopped (bail, cancel, `rt.runner.setNext(null)`).
    pub stopped: bool,
}

/// The requests below `folder` (`None` or `""`: the whole collection; a trailing `/` is
/// ignored) in run order: depth first, in the order of the model, which is the app tree's
/// order (`rt-format` sorts folders and requests by `seq`, then name). An unknown folder gives
/// an empty list.
pub fn plan(collection: &Collection, folder: Option<&str>) -> Vec<String> {
    fn find<'a>(items: &'a [Item], path: &str) -> Option<&'a [Item]> {
        items.iter().find_map(|item| match item {
            Item::Folder(f) if f.path == path => Some(f.items.as_slice()),
            Item::Folder(f) => find(&f.items, path),
            Item::Request(_) => None,
        })
    }
    fn collect(items: &[Item], out: &mut Vec<String>) {
        for item in items {
            match item {
                Item::Folder(f) => collect(&f.items, out),
                Item::Request(r) => out.push(r.path.clone()),
            }
        }
    }
    let folder = folder.map(|f| f.trim_end_matches('/')).unwrap_or("");
    let items = if folder.is_empty() {
        Some(collection.items.as_slice())
    } else {
        find(&collection.items, folder)
    };
    let mut out = Vec::new();
    if let Some(items) = items {
        collect(items, &mut out);
    }
    out
}

/// Runs `plan` with [`platform_sleep`] for the delay. See [`run_with`].
#[allow(clippy::too_many_arguments)]
pub async fn run<T: Transport>(
    collection: &Collection,
    plan: &RunPlan,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    on_event: &mut dyn FnMut(RunEvent),
    cancelled: &dyn Fn() -> bool,
) -> RunSummary {
    run_with(
        collection,
        plan,
        input,
        files,
        transport,
        options,
        services,
        on_event,
        cancelled,
        &platform_sleep,
    )
    .await
}

/// Longest single sleep of a delay, so cancelling never waits for a whole long delay.
const SLICE_MS: u64 = 50;

/// Runs `plan` (see the module documentation for variables and runner control).
///
/// For each iteration and each request: polls `cancelled`, emits [`RunEvent::Request`],
/// sends with [`send_with`] (`options` with [`SendOptions::iteration`] set, `services` as
/// given), emits [`RunEvent::Step`]. Before every request but the first it pauses
/// `plan.delay_ms` (in slices of at most 50 ms through `sleep`, polling `cancelled` between
/// them). `bail` stops after the first step that did not pass ([`passed`]). Emits
/// [`RunEvent::Started`] first and [`RunEvent::Finished`] last, and returns that summary;
/// never fails.
///
/// `sleep(ms)` waits `ms` milliseconds; [`run`] passes [`platform_sleep`], tests a virtual
/// clock.
#[allow(clippy::too_many_arguments)]
pub async fn run_with<T: Transport>(
    collection: &Collection,
    plan: &RunPlan,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    on_event: &mut dyn FnMut(RunEvent),
    cancelled: &dyn Fn() -> bool,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> RunSummary {
    let started = web_time::Instant::now();
    let count = plan.iteration_count();
    let total = count.saturating_mul(u32::try_from(plan.requests.len()).unwrap_or(u32::MAX));
    on_event(RunEvent::Started { total });

    let recorder = Recorder {
        inner: services.scripts,
        changes: RefCell::new(Vec::new()),
    };
    let services = Services {
        scripts: services.scripts.map(|_| &recorder as &dyn ScriptRunner),
        ..*services
    };
    let names: Vec<String> = plan
        .requests
        .iter()
        .map(|path| match collection.request(path) {
            Some(c) => c.request.spec.name.clone(),
            None => path.clone(),
        })
        .collect();

    let mut summary = RunSummary::default();
    // The run's own variables, in the order set (see the module documentation).
    let mut carried: Vec<(String, String)> = Vec::new();
    let mut first = true;
    'run: for index in 0..count {
        let data = plan.data.get(index as usize).cloned().unwrap_or_default();
        let mut at = 0;
        while at < plan.requests.len() {
            if !first && !pause(plan.delay_ms, sleep, cancelled).await {
                summary.stopped = true;
                break 'run;
            }
            if cancelled() {
                summary.stopped = true;
                break 'run;
            }
            first = false;
            let path = &plan.requests[at];
            on_event(RunEvent::Request {
                iteration: index,
                request_path: path.clone(),
            });

            let mut runtime: Vec<(String, String)> = data
                .iter()
                .map(|(name, value)| (name.clone(), data_text(value)))
                .collect();
            runtime.extend(input.runtime.iter().cloned());
            runtime.extend(carried.iter().cloned());
            let send_input = PrepareInput {
                environment: input.environment,
                globals: input.globals.clone(),
                runtime,
            };
            let send_options = SendOptions {
                iteration: Some(Iteration {
                    index,
                    count,
                    data: data.clone(),
                }),
                ..options.clone()
            };
            recorder.changes.borrow_mut().clear();
            let mut report = send_with(
                collection,
                path,
                &send_input,
                files,
                transport,
                &send_options,
                &services,
            )
            .await;

            // Carry the changes with their real values: the script runner's, then the
            // variable actions' (the report lists them in that order, masked).
            let recorded = std::mem::take(&mut *recorder.changes.borrow_mut());
            let actions = report.var_changes.get(recorded.len()..).unwrap_or_default();
            for change in recorded.iter().chain(actions) {
                carried.retain(|(name, _)| name != &change.name);
                if let Some(value) = &change.value {
                    carried.push((change.name.clone(), value.clone()));
                }
            }

            let skipped = report.flow == Some(Flow::Skip);
            let ok = skipped || passed(&report);
            let mut next = at + 1;
            let mut stop = false;
            if let Some(Flow::Next { request }) = &report.flow {
                match request {
                    None => stop = true,
                    Some(target) => match find_target(&plan.requests, &names, target) {
                        Some(i) => next = i,
                        None => {
                            report.notes.push(Note {
                                source: "runner".into(),
                                level: NoteLevel::Error,
                                text: format!(
                                    "rt.runner.setNext: no request \"{target}\" in this run; \
                                     the run stops"
                                ),
                            });
                            stop = true;
                        }
                    },
                }
            }

            summary.requests += 1;
            if skipped {
                summary.skipped += 1;
            } else if ok {
                summary.passed += 1;
            } else {
                summary.failed += 1;
            }
            let (tp, tf) = count_by(&report.tests, |t: &TestResult| t.passed);
            let (ap, af) = count_by(&report.assertions, |a: &AssertionResult| a.passed);
            summary.tests_passed += tp;
            summary.tests_failed += tf;
            summary.assertions_passed += ap;
            summary.assertions_failed += af;

            on_event(RunEvent::Step {
                step: Box::new(RunStep {
                    iteration: index,
                    request_path: path.clone(),
                    request_name: names[at].clone(),
                    report,
                    passed: ok,
                    skipped,
                }),
            });
            if stop || (plan.bail && !ok) {
                summary.stopped = true;
                break 'run;
            }
            at = next;
        }
    }
    summary.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    on_event(RunEvent::Finished {
        summary: summary.clone(),
    });
    summary
}

/// Waits `ms` in slices; `false` if cancelled meanwhile.
async fn pause(
    ms: u64,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
    cancelled: &dyn Fn() -> bool,
) -> bool {
    let mut left = ms;
    while left > 0 {
        if cancelled() {
            return false;
        }
        let slice = left.min(SLICE_MS);
        sleep(slice).await;
        left -= slice;
    }
    true
}

/// The index of the request `target` names: by path first, then by name.
fn find_target(paths: &[String], names: &[String], target: &str) -> Option<usize> {
    paths
        .iter()
        .position(|p| p == target)
        .or_else(|| names.iter().position(|n| n == target))
}

fn count_by<T>(items: &[T], ok: impl Fn(&T) -> bool) -> (u32, u32) {
    items.iter().fold(
        (0, 0),
        |(p, f), item| {
            if ok(item) { (p + 1, f) } else { (p, f + 1) }
        },
    )
}

/// A data value as a variable: strings as they are, everything else as JSON text.
fn data_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Passes calls to the shell's script runner and keeps the variable changes with their real
/// values (the report has them masked).
struct Recorder<'a> {
    inner: Option<&'a dyn ScriptRunner>,
    changes: RefCell<Vec<VarChange>>,
}

impl ScriptRunner for Recorder<'_> {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            let Some(inner) = self.inner else {
                return ScriptOutcome::default();
            };
            let outcome = inner.run(call).await;
            self.changes
                .borrow_mut()
                .extend(outcome.var_changes.iter().cloned());
            outcome
        })
    }
}

/// Helper for reporters: did this report pass?
pub fn passed(report: &SendReport) -> bool {
    report.error.is_none()
        && report.tests.iter().all(|t: &TestResult| t.passed)
        && report.assertions.iter().all(|a: &AssertionResult| a.passed)
}

/// Variable changes of a report, for carrying them to the next request.
pub fn changes(report: &SendReport) -> &[VarChange] {
    &report.var_changes
}

// ---------------------------------------------------------------------------------------------
// Sleeping on every target

/// Waits `ms` milliseconds without blocking the caller's executor: natively on a helper
/// thread (no async runtime needed), in the browser with `setTimeout`.
#[cfg(not(target_arch = "wasm32"))]
pub fn platform_sleep(ms: u64) -> LocalBoxFuture<'static, ()> {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::task::{Context, Poll, Waker};

    #[derive(Default)]
    struct Shared {
        done: bool,
        waker: Option<Waker>,
    }

    struct ThreadSleep {
        ms: u64,
        shared: Arc<Mutex<Shared>>,
        started: bool,
    }

    impl Future for ThreadSleep {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            {
                let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
                if shared.done {
                    return Poll::Ready(());
                }
                shared.waker = Some(cx.waker().clone());
            }
            if !self.started {
                self.started = true;
                let shared = Arc::clone(&self.shared);
                let ms = self.ms;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    shared.done = true;
                    if let Some(waker) = shared.waker.take() {
                        waker.wake();
                    }
                });
            }
            Poll::Pending
        }
    }

    if ms == 0 {
        return Box::pin(std::future::ready(()));
    }
    Box::pin(ThreadSleep {
        ms,
        shared: Arc::default(),
        started: false,
    })
}

/// Waits `ms` milliseconds without blocking the caller's executor: natively on a helper
/// thread (no async runtime needed), in the browser with `setTimeout`.
#[cfg(target_arch = "wasm32")]
pub fn platform_sleep(ms: u64) -> LocalBoxFuture<'static, ()> {
    use wasm_bindgen::{JsCast, JsValue};
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        match set_timeout {
            Some(f) => {
                let _ = f.call2(&global, &resolve, &JsValue::from_f64(ms as f64));
            }
            None => {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
    });
    Box::pin(async move {
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    })
}

// ---------------------------------------------------------------------------------------------
// Data files

/// The rows of a data file, by extension (case-insensitive):
///
/// - `.csv` (RFC 4180): the first non-blank line is the header (non-empty, unique column
///   names); fields may be quoted (`"a, b"`, `""` for a quote, line breaks inside quotes);
///   CRLF or LF line ends; a UTF-8 byte order mark is ignored; blank lines are skipped; every
///   row has as many fields as the header. Values are strings.
/// - `.json`: an array of objects; values are kept as they are (numbers, booleans, `null`,
///   arrays, objects), keys in file order.
///
/// Errors name the file and the line (CSV) or the item and position (JSON).
pub fn parse_data(name: &str, bytes: &[u8]) -> Result<Vec<DataRow>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| {
        let line = 1 + bytes[..e.valid_up_to()]
            .iter()
            .filter(|b| **b == b'\n')
            .count();
        format!("{name}: line {line}: not valid UTF-8 text")
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lower = name.to_ascii_lowercase();
    let result = if lower.ends_with(".csv") {
        parse_csv(text)
    } else if lower.ends_with(".json") {
        parse_json(text)
    } else {
        Err("data files are .csv or .json".to_owned())
    };
    result.map_err(|e| format!("{name}: {e}"))
}

/// One CSV record: the line it starts on, its fields, and whether the line was blank.
struct Record {
    line: usize,
    fields: Vec<String>,
    blank: bool,
}

fn csv_records(text: &str) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut line = 1;
    let mut record_line = 1;
    let mut blank = true;
    let mut in_quotes = false;
    let mut quoted = false;
    let mut closed = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    in_quotes = false;
                    closed = true;
                }
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            ',' => {
                fields.push(std::mem::take(&mut field));
                quoted = false;
                closed = false;
                blank = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                fields.push(std::mem::take(&mut field));
                records.push(Record {
                    line: record_line,
                    fields: std::mem::take(&mut fields),
                    blank,
                });
                line += 1;
                record_line = line;
                blank = true;
                quoted = false;
                closed = false;
            }
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
                blank = false;
            }
            '"' => {
                return Err(format!(
                    "line {line}: a quote inside an unquoted field (quote the whole field and \
                     double the quote: \"\")"
                ));
            }
            _ if closed => {
                return Err(format!(
                    "line {line}: text after the closing quote of a field"
                ));
            }
            _ => {
                field.push(c);
                blank = false;
            }
        }
    }
    if in_quotes {
        return Err(format!(
            "line {record_line}: a quoted field is not closed (missing closing quote)"
        ));
    }
    if !blank || !fields.is_empty() {
        fields.push(field);
        records.push(Record {
            line: record_line,
            fields,
            blank: false,
        });
    }
    Ok(records)
}

fn parse_csv(text: &str) -> Result<Vec<DataRow>, String> {
    let mut records = csv_records(text)?.into_iter().filter(|r| !r.blank);
    let header = records
        .next()
        .ok_or("no header row (the first line names the columns)")?;
    for (i, column) in header.fields.iter().enumerate() {
        if column.is_empty() {
            return Err(format!(
                "line {}: column {} of the header has an empty name",
                header.line,
                i + 1
            ));
        }
        if header.fields[..i].contains(column) {
            return Err(format!(
                "line {}: column \"{column}\" appears twice in the header",
                header.line
            ));
        }
    }
    records
        .map(|r| {
            if r.fields.len() != header.fields.len() {
                return Err(format!(
                    "line {}: {} fields, but the header has {} columns",
                    r.line,
                    r.fields.len(),
                    header.fields.len()
                ));
            }
            Ok(header
                .fields
                .iter()
                .cloned()
                .zip(r.fields.into_iter().map(serde_json::Value::String))
                .collect())
        })
        .collect()
}

fn parse_json(text: &str) -> Result<Vec<DataRow>, String> {
    use serde::de::DeserializeSeed;
    use std::cell::Cell;

    let item = Cell::new(0usize);
    let mut de = serde_json::Deserializer::from_str(text);
    let rows = RowsSeed { item: &item }
        .deserialize(&mut de)
        .and_then(|rows| de.end().map(|()| rows));
    rows.map_err(|e| {
        if e.classify() == serde_json::error::Category::Data && item.get() > 0 {
            format!("item {}: {e}", item.get())
        } else {
            e.to_string()
        }
    })
}

/// The top-level array; counts the items so errors can name one.
struct RowsSeed<'c> {
    item: &'c std::cell::Cell<usize>,
}

impl<'de> serde::de::DeserializeSeed<'de> for RowsSeed<'_> {
    type Value = Vec<DataRow>;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for RowsSeed<'_> {
    type Value = Vec<DataRow>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an array of objects (one object per iteration)")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut rows = Vec::new();
        loop {
            self.item.set(rows.len() + 1);
            match seq.next_element::<Row>()? {
                Some(Row(row)) => rows.push(row),
                None => break,
            }
        }
        self.item.set(0);
        Ok(rows)
    }
}

/// One object, keys in file order (a repeated key keeps its last value).
struct Row(DataRow);

impl<'de> Deserialize<'de> for Row {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct RowVisitor;
        impl<'de> serde::de::Visitor<'de> for RowVisitor {
            type Value = Row;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut row: DataRow = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    row.retain(|(k, _)| *k != key);
                    row.push((key, value));
                }
                Ok(Row(row))
            }
        }
        d.deserialize_any(RowVisitor)
    }
}
