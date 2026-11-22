// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner in the app (wave 7, strand D): an owned copy of the open collection
//! for one run ([`RunSnapshot`], from [`crate::Workspace::run_snapshot`]), run by
//! `rt_core::runner::run`. Desktop (Tauri) and web (`rt-wasm`) both use it.
//!
//! Variable changes of the run stay in the run: they never reach the app's session
//! variables or files.

use rt_core::model::Collection;
use rt_core::prepare::PrepareInput;
use rt_core::runner::{RunEvent, RunPlan, RunSummary};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::Transport;
use rt_format::FileSet;

use crate::workspace::MemFiles;

/// Everything a run needs, owned, so running (async, long) never borrows the `Workspace`.
#[derive(Debug, Clone)]
pub struct RunSnapshot {
    /// The collection with the secret values of the selected environment.
    pub collection: Collection,
    pub environment: Option<String>,
    /// The caller's runtime variables (`SendInput::vars`).
    pub vars: Vec<(String, String)>,
    /// Files the requests' bodies and the client certificates read.
    pub files: FileSet,
}

impl RunSnapshot {
    /// The request paths below `folder` (`None`: all) in run order.
    pub fn plan(&self, folder: Option<&str>) -> Vec<String> {
        rt_core::runner::plan(&self.collection, folder)
    }

    /// Runs `plan` (`rt_core::runner::run`).
    pub async fn run<T: Transport>(
        &self,
        plan: &RunPlan,
        transport: &T,
        options: &SendOptions,
        services: &Services<'_>,
        on_event: &mut dyn FnMut(RunEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> RunSummary {
        let input = PrepareInput {
            environment: self.environment.as_deref(),
            globals: Vec::new(),
            runtime: self.vars.clone(),
        };
        rt_core::runner::run(
            &self.collection,
            plan,
            &input,
            &MemFiles(&self.files),
            transport,
            options,
            services,
            on_event,
            cancelled,
        )
        .await
    }
}
