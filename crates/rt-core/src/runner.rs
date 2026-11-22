// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner (wave 7, strand D): runs the requests of a folder or the whole
//! collection in order, optionally several iterations with a data row each (CSV or JSON),
//! carrying variable changes from one request to the next. The app, the web app and
//! `rt run` (strand E) use this one implementation.

use serde::{Deserialize, Serialize};

use crate::model::Collection;
use crate::prepare::{FileSource, PrepareInput};
use crate::script::{AssertionResult, TestResult, VarChange};
use crate::send::{SendOptions, SendReport};
use crate::services::Services;
use crate::transport::Transport;

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
}

fn one() -> u32 {
    1
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
    Started {
        total: u32,
    },
    Request {
        iteration: u32,
        request_path: String,
    },
    Step {
        step: Box<RunStep>,
    },
    Finished {
        summary: RunSummary,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunSummary {
    pub requests: u32,
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

/// The requests below `folder` (`None`: the whole collection) in run order: folders and
/// requests by `seq`, then name, depth first.
pub fn plan(collection: &Collection, folder: Option<&str>) -> Vec<String> {
    let _ = (collection, folder);
    Vec::new()
}

/// Runs `plan`. `cancelled` is polled between requests. Variable changes of each step go into
/// the runtime layer of the next ones (and are kept for the whole run).
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
    let _ = (
        collection, plan, input, files, transport, options, services, cancelled,
    );
    let summary = RunSummary::default();
    on_event(RunEvent::Finished {
        summary: summary.clone(),
    });
    summary
}

/// One data row: column or key, value (CSV cells as strings, JSON values as they are).
pub type DataRow = Vec<(String, serde_json::Value)>;

/// Reads a data file (`.csv` or `.json`, by `name`) into rows. Strand D implements it; this
/// stub (added by strand E for `rt run --data`) refuses every file.
pub fn parse_data(name: &str, bytes: &[u8]) -> Result<Vec<DataRow>, String> {
    let _ = bytes;
    Err(format!("{name}: data files are not supported yet"))
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
