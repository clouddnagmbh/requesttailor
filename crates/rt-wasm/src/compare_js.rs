// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand C: runs at a revision and their comparison, bisect (`rt_app::compare`,
//! `rt_app::bisect`) over the open `Workspace`, with the same transports and services as
//! `Workspace.send`. The web shell's `src/compare.ts` declares the TypeScript types; the
//! Git work (trees, commits) stays in the shell and is passed in.
//!
//! ```text
//! Workspace.runAtRevision(request, commit, tree, input, companion, services, cancelled,
//!     onRecords) -> Promise<RevisionRun>
//! Workspace.compareRevisions(request, fromCommit, fromTree, toCommit, toTree, input,
//!     companion, services, cancelled, onRecords) -> Promise<RunComparison>
//! Workspace.bisect(request, candidates, treeAt, input, companion, services, onStep, cancelled,
//!     onRecords) -> Promise<BisectOutcome>
//! ```
//!
//! `request` is a `RevisionRunRequest`, `CompareRequest` or `BisectRequest`, `commit` a
//! `GitCommit`, `tree` a `GitTree`, `candidates` the `GitCommit`s newest first (what
//! `GitApi.commits(good, bad)` returns), `treeAt(oid)` returns a promise of the commit's
//! `GitTree` (a rejection makes that commit `skip`). `onStep(step)` gets every `BisectStep`;
//! `cancelled()` is polled before each request and test. `onRecords(oid, records)` (optional)
//! gets the `TestRecord`s of every finished run under the commit it ran at, for the time
//! travel (`rt_app::timeline`, source `compare` or `bisect`, collection `""`: the web app's
//! collection is the repository root). Rejects with the usual shell errors (`format` when the
//! collection cannot be read at the revision).

use js_sys::{Function, Promise};
use rt_app::SendInput;
use rt_app::bisect::{BisectRequest, BisectStep, search, test_revision};
use rt_app::compare::{CompareRequest, RevisionRunRequest, compare_revisions, run_revision};
use rt_app::git::{GitCommit, GitTree};
use rt_app::timeline::{RecordSource, records_of_steps};
use rt_core::runner::RunStep;
use rt_core::send::SendOptions;
use rt_core::transport::Transport;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

fn companion_of(value: &JsValue) -> Result<Option<Companion>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(from_js(value, "companion")?))
    }
}

fn app_error(e: rt_app::AppError) -> JsValue {
    crate::workspace_js::app_error(e)
}

fn options() -> SendOptions {
    SendOptions {
        body_limit: BODY_LIMIT,
        iteration: None,
    }
}

/// Hands the records of a run at `oid` to `onRecords`, if there is one.
fn record(on_records: &JsValue, source: RecordSource, oid: &str, steps: &[RunStep]) {
    let Some(f) = on_records.dyn_ref::<Function>() else {
        return;
    };
    // Wall-clock seconds, as the native recorder.
    let time = (js_sys::Date::now() / 1000.0) as i64;
    let records = records_of_steps("", time, source, steps);
    if records.is_empty() {
        return;
    }
    if let Ok(value) = to_js(&records) {
        // Recording is a convenience: a throwing listener must not fail the run.
        let _ = f.call2(&JsValue::NULL, &JsValue::from_str(oid), &value);
    }
}

fn stop(cancelled: &Function) -> bool {
    cancelled
        .call0(&JsValue::NULL)
        .map(|v| v.is_truthy())
        .unwrap_or(false)
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Runs the requests of `request` at one revision; see the module documentation.
    #[wasm_bindgen(js_name = runAtRevision)]
    #[allow(clippy::too_many_arguments)] // one argument per value of the JS signature
    pub fn run_at_revision(
        &self,
        request: JsValue,
        commit: JsValue,
        tree: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
        #[wasm_bindgen(js_name = onRecords)] on_records: JsValue,
    ) -> Result<Promise, JsValue> {
        let request: RevisionRunRequest = from_js(&request, "request")?;
        let commit: GitCommit = from_js(&commit, "commit")?;
        let tree: GitTree = from_js(&tree, "tree")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion = companion_of(&companion)?;
        let today = self.workspace().run_snapshot(input);
        let parts = SendServices::from_js(&services)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let services = parts.services();
            let cancelled = || stop(&cancelled);
            let run = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    run_revision(
                        &today,
                        commit,
                        &tree,
                        &request,
                        &t,
                        &options(),
                        &services,
                        &mut |_| {},
                        &cancelled,
                    )
                    .await
                }
                None => {
                    let t = FetchTransport::new();
                    run_revision(
                        &today,
                        commit,
                        &tree,
                        &request,
                        &t,
                        &options(),
                        &services,
                        &mut |_| {},
                        &cancelled,
                    )
                    .await
                }
            };
            let run = run.map_err(app_error)?;
            record(
                &on_records,
                RecordSource::Compare,
                &run.commit.oid,
                &run.steps,
            );
            to_js(&run)
        }))
    }

    /// Runs the same requests at two revisions and compares them; see the module documentation.
    #[wasm_bindgen(js_name = compareRevisions)]
    #[allow(clippy::too_many_arguments)] // one argument per value of the JS signature
    pub fn compare_revisions_js(
        &self,
        request: JsValue,
        from_commit: JsValue,
        from_tree: JsValue,
        to_commit: JsValue,
        to_tree: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
        #[wasm_bindgen(js_name = onRecords)] on_records: JsValue,
    ) -> Result<Promise, JsValue> {
        let request: CompareRequest = from_js(&request, "request")?;
        let from: (GitCommit, GitTree) = (
            from_js(&from_commit, "fromCommit")?,
            from_js(&from_tree, "fromTree")?,
        );
        let to: (GitCommit, GitTree) = (
            from_js(&to_commit, "toCommit")?,
            from_js(&to_tree, "toTree")?,
        );
        let input: SendInput = from_js(&input, "input")?;
        let companion = companion_of(&companion)?;
        let today = self.workspace().run_snapshot(input);
        let parts = SendServices::from_js(&services)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let services = parts.services();
            let cancelled = || stop(&cancelled);
            let mut on_run = |commit: &GitCommit, run: &rt_app::compare::RevisionOutcome| {
                record(&on_records, RecordSource::Compare, &commit.oid, &run.steps);
            };
            let cmp = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    compare_revisions(
                        &today,
                        (from.0, &from.1),
                        (to.0, &to.1),
                        &request,
                        &t,
                        &options(),
                        &services,
                        &mut on_run,
                        &cancelled,
                    )
                    .await
                }
                None => {
                    let t = FetchTransport::new();
                    compare_revisions(
                        &today,
                        (from.0, &from.1),
                        (to.0, &to.1),
                        &request,
                        &t,
                        &options(),
                        &services,
                        &mut on_run,
                        &cancelled,
                    )
                    .await
                }
            };
            to_js(&cmp.map_err(app_error)?)
        }))
    }

    /// Bisects the candidates; see the module documentation.
    #[wasm_bindgen(js_name = bisect)]
    #[allow(clippy::too_many_arguments)] // one argument per value of the JS signature
    pub fn bisect_js(
        &self,
        request: JsValue,
        candidates: JsValue,
        #[wasm_bindgen(js_name = treeAt)] tree_at: Function,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        #[wasm_bindgen(js_name = onStep)] on_step: Function,
        cancelled: Function,
        #[wasm_bindgen(js_name = onRecords)] on_records: JsValue,
    ) -> Result<Promise, JsValue> {
        let request: BisectRequest = from_js(&request, "request")?;
        let candidates: Vec<GitCommit> = from_js(&candidates, "candidates")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion = companion_of(&companion)?;
        let today = self.workspace().run_snapshot(input);
        let parts = SendServices::from_js(&services)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let services = parts.services();
            let outcome = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    bisect_with(
                        &today,
                        &request,
                        candidates,
                        &tree_at,
                        &t,
                        &services,
                        &on_step,
                        &cancelled,
                        &on_records,
                    )
                    .await
                }
                None => {
                    let t = FetchTransport::new();
                    bisect_with(
                        &today,
                        &request,
                        candidates,
                        &tree_at,
                        &t,
                        &services,
                        &on_step,
                        &cancelled,
                        &on_records,
                    )
                    .await
                }
            };
            to_js(&outcome)
        }))
    }
}

#[allow(clippy::too_many_arguments)]
async fn bisect_with<T: Transport>(
    today: &rt_app::runner::RunSnapshot,
    request: &BisectRequest,
    candidates: Vec<GitCommit>,
    tree_at: &Function,
    transport: &T,
    services: &rt_core::services::Services<'_>,
    on_step: &Function,
    cancelled: &Function,
    on_records: &JsValue,
) -> rt_app::bisect::BisectOutcome {
    let options = options();
    let folder = request.folder.as_deref().filter(|f| !f.is_empty());
    let cancel = || stop(cancelled);
    search(
        candidates,
        async |commit: &GitCommit| {
            let tree = fetch_tree(tree_at, &commit.oid).await;
            test_revision(
                today,
                tree,
                folder,
                transport,
                &options,
                services,
                &mut |run| record(on_records, RecordSource::Bisect, &commit.oid, &run.steps),
                &cancel,
            )
            .await
        },
        &mut |step: &BisectStep| {
            if let Ok(value) = to_js(step) {
                // A throwing listener must not stop the search.
                let _ = on_step.call1(&JsValue::NULL, &value);
            }
        },
        &cancel,
    )
    .await
}

async fn fetch_tree(tree_at: &Function, oid: &str) -> Result<GitTree, String> {
    let promise = tree_at
        .call1(&JsValue::NULL, &JsValue::from_str(oid))
        .map_err(|e| format!("{e:?}"))?;
    let value = JsFuture::from(Promise::resolve(&promise))
        .await
        .map_err(|e| format!("{e:?}"))?;
    from_js(&value, "tree").map_err(|e| format!("{e:?}"))
}
