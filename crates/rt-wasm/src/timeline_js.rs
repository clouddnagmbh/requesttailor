// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand D: request history, blame per field, test records (`rt_app::timeline`) for
//! the web shell. The shell reads the versions from Git (`fileLog`, `show`) and keeps the
//! recorded results in IndexedDB; the core does the rest, the same code as the desktop app.
//! The web shell's `apps/web/src/timeline.ts` declares the TypeScript types of these
//! functions itself (as `apps/web/src/runner.ts` does), so `apps/web/src/wasm.ts` stays
//! untouched.
//!
//! ```text
//! timelineBuild(path, collection, versions, records) -> RequestTimeline
//! timelineBlame(versions) -> FieldBlame[]
//! timelineSpec(text) -> RequestSpec
//! timelineMergeRecords(existing: string, records: TestRecord[]) -> string
//! timelineParseRecords(text) -> TestRecord[]
//! timelineRecordOfStep(collection, time, source, step) -> TestRecord
//! ```
//!
//! `versions` is `{ commit: GitFileCommit, text: string | null }[]`, newest first; `records`
//! is `{ [commit oid]: TestRecord[] }`.

use std::collections::BTreeMap;

use rt_app::git::GitFileCommit;
use rt_app::timeline::{self, RecordSource, TestRecord, Version};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{app_error, from_js, to_js};

#[derive(Deserialize)]
struct JsVersion {
    commit: GitFileCommit,
    text: Option<String>,
}

fn versions(value: &JsValue) -> Result<Vec<Version>, JsValue> {
    let list: Vec<JsVersion> = from_js(value, "versions")?;
    Ok(list
        .into_iter()
        .map(|v| Version {
            commit: v.commit,
            text: v.text,
        })
        .collect())
}

#[wasm_bindgen(js_name = timelineBuild)]
pub fn timeline_build(
    path: &str,
    collection: &str,
    list: JsValue,
    records: JsValue,
) -> Result<JsValue, JsValue> {
    let records: BTreeMap<String, Vec<TestRecord>> = from_js(&records, "records")?;
    to_js(&timeline::build(
        path,
        collection,
        &versions(&list)?,
        &records,
    ))
}

#[wasm_bindgen(js_name = timelineBlame)]
pub fn timeline_blame(list: JsValue) -> Result<JsValue, JsValue> {
    to_js(&timeline::blame(&versions(&list)?))
}

#[wasm_bindgen(js_name = timelineSpec)]
pub fn timeline_spec(text: &str) -> Result<JsValue, JsValue> {
    to_js(&timeline::spec_of(text).map_err(app_error)?)
}

#[wasm_bindgen(js_name = timelineMergeRecords)]
pub fn timeline_merge_records(existing: &str, records: JsValue) -> Result<String, JsValue> {
    let records: Vec<TestRecord> = from_js(&records, "records")?;
    Ok(timeline::merge_records(existing, &records))
}

#[wasm_bindgen(js_name = timelineParseRecords)]
pub fn timeline_parse_records(text: &str) -> Result<JsValue, JsValue> {
    to_js(&timeline::parse_records(text))
}

/// One `step` of a `RunEvent` (`{ type: "step", step }`'s `step`).
#[wasm_bindgen(js_name = timelineRecordOfStep)]
pub fn timeline_record_of_step(
    collection: &str,
    time: f64,
    source: JsValue,
    step: JsValue,
) -> Result<JsValue, JsValue> {
    let source: RecordSource = from_js(&source, "source")?;
    let step: rt_core::runner::RunStep = from_js(&step, "step")?;
    to_js(&timeline::record_of_step(
        collection,
        time as i64,
        source,
        &step,
    ))
}
