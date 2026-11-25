// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The [`Recorder`]: an `ExchangeObserver` that keeps the little a sequence diagram needs of
//! each exchange (method, host, path, status, duration, failures), never the whole report.
//! Exchanges arrive masked (`rt_core::observe`), and the recorder only reads what the
//! diagram shows, so it adds no secret to anything.

use std::collections::VecDeque;
use std::sync::Mutex;

use rt_core::guard::Caller;
use rt_core::observe::{Exchange, ExchangeObserver};
use rt_core::send::{ReportBody, SendError};

use super::build;
use super::{SequenceModel, SequenceOptions, SequenceSource};

/// Exchanges the recorder keeps (the oldest are dropped).
pub const RECORD_LIMIT: usize = 4_000;
/// Bytes of a response body kept for "include values".
const BODY_HEAD: usize = 4_096;
/// Failed tests and assertions kept per exchange.
const FAILED_KEPT: usize = 5;

/// A failed assertion, as much as the diagram shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailedAssertion {
    pub expression: String,
    pub operator: String,
    pub expected: String,
    pub actual: Option<String>,
}

/// What the recorder keeps of one exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub run: u32,
    pub caller: Caller,
    pub request_path: String,
    pub method: String,
    /// `None` when the request never got a URL (preparation failed).
    pub host: Option<String>,
    pub path: String,
    pub query: Option<String>,
    pub status: Option<u16>,
    pub millis: Option<u64>,
    /// The kind of the send error, if there was one.
    pub error: Option<&'static str>,
    /// The rule of a refusal by the send guard.
    pub rule: Option<String>,
    pub tests_total: u32,
    pub tests_failed: Vec<String>,
    pub assertions_total: u32,
    pub assertions_failed: Vec<FailedAssertion>,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub body_head: Option<String>,
}

pub(crate) fn error_kind(error: &SendError) -> &'static str {
    match error {
        SendError::NotFound(_) => "not found",
        SendError::Prepare(_) => "prepare",
        SendError::Auth(_) => "auth",
        SendError::Plugin(_) => "plugin",
        SendError::Script(_) => "script",
        SendError::Blocked { .. } => "blocked",
        SendError::Transport(_) => "transport",
    }
}

/// Splits a URL into host (with port, without credentials), path and query. Placeholders
/// like `{{base}}` stay as they are.
pub(crate) fn split_url(url: &str) -> (Option<String>, String, Option<String>) {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    let (rest, _fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = (!host.is_empty()).then(|| host.to_owned());
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q.to_owned())),
        None => (tail, None),
    };
    let path = if path.is_empty() { "/" } else { path };
    (host, path.to_owned(), query.filter(|q| !q.is_empty()))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn record_of(exchange: &Exchange, run: u32) -> Record {
    let report = &exchange.report;
    let (method, url) = match &report.request {
        Some(r) => (r.method.clone(), Some(r.url.as_str())),
        None => (String::new(), None),
    };
    let (host, path, query) = match url {
        Some(u) => split_url(u),
        None => (None, String::new(), None),
    };
    let response = report.response.as_ref();
    let mut rule = None;
    if let Some(SendError::Blocked { rule: r, .. }) = &report.error {
        rule = Some(r.clone());
    }
    let body_head = response.and_then(|r| match &r.body {
        ReportBody::Utf8(text) => {
            let mut end = text.len().min(BODY_HEAD);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            Some(text[..end].to_owned())
        }
        ReportBody::Base64(_) => None,
    });
    Record {
        run,
        caller: exchange.caller,
        request_path: exchange
            .request_path
            .clone()
            .unwrap_or_else(|| report.request_path.clone()),
        method,
        host,
        path,
        query,
        status: response.map(|r| r.status),
        millis: response.map(|r| r.total.as_millis() as u64),
        error: report.error.as_ref().map(error_kind),
        rule,
        tests_total: report.tests.len() as u32,
        tests_failed: report
            .tests
            .iter()
            .filter(|t| !t.passed)
            .take(FAILED_KEPT)
            .map(|t| t.name.clone())
            .collect(),
        assertions_total: report.assertions.len() as u32,
        assertions_failed: report
            .assertions
            .iter()
            .filter(|a| !a.passed)
            .take(FAILED_KEPT)
            .map(|a| FailedAssertion {
                expression: a.expression.clone(),
                operator: a.operator.clone(),
                expected: a.expected.clone(),
                actual: a.actual.clone(),
            })
            .collect(),
        content_type: response.and_then(|r| header(&r.headers, "content-type").map(str::to_owned)),
        location: response.and_then(|r| header(&r.headers, "location").map(str::to_owned)),
        body_head,
    }
}

#[derive(Debug, Default)]
struct State {
    records: VecDeque<Record>,
    /// The run in progress (0: none).
    current: u32,
    /// The highest run id handed out.
    last: u32,
}

/// An `ExchangeObserver` that builds a [`SequenceModel`] from every exchange it sees: what the
/// runner, `rt run`, `rt mcp` and flows send through `send_with` with `Services.observers`.
#[derive(Debug, Default)]
pub struct Recorder {
    state: Mutex<State>,
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a run: the exchanges until [`Recorder::end_run`] belong to it. Returns its id.
    pub fn begin_run(&self) -> u32 {
        let mut state = self.lock();
        state.last += 1;
        state.current = state.last;
        state.current
    }

    /// Ends the run in progress.
    pub fn end_run(&self) {
        self.lock().current = 0;
    }

    /// Forgets everything recorded.
    pub fn clear(&self) {
        let mut state = self.lock();
        state.records.clear();
    }

    /// Exchanges recorded.
    pub fn len(&self) -> usize {
        self.lock().records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The model of `source`: a run (`run_id` 0: the latest), the session (everything
    /// recorded), or the pages of one request (the latest exchanges of `request_path`).
    /// `Mcp` and `History` have their own readers ([`super::model_from_audit`],
    /// [`super::model_from_history`]).
    pub fn model(
        &self,
        source: &SequenceSource,
        options: &SequenceOptions,
    ) -> Result<SequenceModel, String> {
        let state = self.lock();
        match source {
            SequenceSource::Session => {
                let records: Vec<&Record> = state.records.iter().collect();
                Ok(build::model_from_records(&records, options, false))
            }
            SequenceSource::Run { run_id } => {
                let id = if *run_id == 0 { state.last } else { *run_id };
                let records: Vec<&Record> = state.records.iter().filter(|r| r.run == id).collect();
                Ok(build::model_from_records(&records, options, false))
            }
            SequenceSource::Pages { request_path } => {
                let mut tail: Vec<&Record> = state
                    .records
                    .iter()
                    .rev()
                    .take_while(|r| &r.request_path == request_path)
                    .collect();
                tail.reverse();
                Ok(build::model_from_records(&tail, options, true))
            }
            SequenceSource::Mcp { .. } | SequenceSource::History { .. } => Err(
                "this source is read from the audit log or the history, not from the recorder"
                    .into(),
            ),
        }
    }
}

impl ExchangeObserver for Recorder {
    fn observe(&self, exchange: &Exchange) {
        let mut state = self.lock();
        let record = record_of(exchange, state.current);
        if state.records.len() >= RECORD_LIMIT {
            state.records.pop_front();
        }
        state.records.push_back(record);
    }
}
