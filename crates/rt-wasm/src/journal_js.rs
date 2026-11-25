// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand C: the test-data journal (`rt_app::journal`) over the open `Workspace`.
//! `apps/web/src/journal.ts` declares the TypeScript type and keeps the entries in IndexedDB.
//!
//! The journal of a collection lives in memory here (a [`QueueStorage`]); the shell loads what
//! IndexedDB holds with `journalOpen` and writes back what `journalDrain` returns (after each
//! call and on a timer), so the send path never waits for the browser's storage. "Track test
//! data" starts off in every page load.
//!
//! ```text
//! Workspace.journalOpen(entries: JournalEntry[], settings: JournalSettings)
//! Workspace.journalSettings() -> JournalSettings
//! Workspace.journalSetSettings(settings)
//! Workspace.journalEntries(query) -> JournalPage
//! Workspace.journalClear()
//! Workspace.journalDrain() -> StorageChange[]     // { op: "append", entry } | { op: "rewrite",
//!                                                 //   entries } | { op: "settings", settings }
//! Workspace.journalStates(ids) -> EntryStates[]
//! Workspace.journalPlanManual(revert, input) -> Promise<RevertPlan>
//! Workspace.journalPlanCompensations(items, input) -> Promise<RevertPlan>
//! Workspace.journalExecute(plan, options, input, companion, services) -> Promise<RevertOutcome>
//! Workspace.journalPreRead(request, input, companion, services) -> Promise<PreRead>
//! ```
//!
//! `input` is the `SendInput` of the plan's environment (secret values of that environment),
//! `companion` and `services` as for `Workspace.send`. There is no audit log in the browser:
//! the guard decides, the journal marks what was reverted.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use js_sys::Promise;
use rt_app::SendInput;
use rt_app::journal::{
    self, CompensationItem, Journal, JournalQuery, JournalSettings, ManualRevert, QueueStorage,
    RevertEnv, RevertOptions, RevertPlan, StorageOp,
};
use rt_core::model::RequestSpec;
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

thread_local! {
    /// The queue behind each journal, by collection name (what the shell drains).
    static QUEUES: RefCell<HashMap<String, Arc<QueueStorage>>> = RefCell::new(HashMap::new());
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum Change {
    Append { entry: journal::JournalEntry },
    Rewrite { entries: Vec<journal::JournalEntry> },
    Settings { settings: JournalSettings },
}

fn queue(name: &str) -> Option<Arc<QueueStorage>> {
    QUEUES.with(|q| q.borrow().get(name).cloned())
}

fn not_open() -> JsValue {
    JsValue::from_str("the journal is not open (call journalOpen first)")
}

fn companion_of(value: &JsValue) -> Result<Option<Companion>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(from_js(value, "companion")?))
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    fn journal_handle(&self) -> Result<(Arc<Journal>, String), JsValue> {
        let name = self.workspace().collection().name.clone();
        let queue = queue(&name).ok_or_else(not_open)?;
        let journal = journal::journal_for(&name, move || queue);
        Ok((journal, name))
    }

    /// Opens the journal of this collection with what the shell stored.
    #[wasm_bindgen(js_name = journalOpen)]
    pub fn journal_open(&self, entries: JsValue, settings: JsValue) -> Result<(), JsValue> {
        let name = self.workspace().collection().name.clone();
        if queue(&name).is_some() {
            return Ok(());
        }
        let entries: Vec<journal::JournalEntry> = from_js(&entries, "entries")?;
        let settings: JournalSettings = if settings.is_null() || settings.is_undefined() {
            JournalSettings::default()
        } else {
            from_js(&settings, "settings")?
        };
        let storage = Arc::new(QueueStorage::new(entries, settings));
        QUEUES.with(|q| q.borrow_mut().insert(name.clone(), Arc::clone(&storage)));
        journal::journal_for(&name, move || storage);
        Ok(())
    }

    #[wasm_bindgen(js_name = journalSettings)]
    pub fn journal_settings(&self) -> Result<JsValue, JsValue> {
        to_js(&self.journal_handle()?.0.settings())
    }

    #[wasm_bindgen(js_name = journalSetSettings)]
    pub fn journal_set_settings(&self, settings: JsValue) -> Result<(), JsValue> {
        let settings: JournalSettings = from_js(&settings, "settings")?;
        self.journal_handle()?
            .0
            .set_settings(settings)
            .map_err(app_error)?;
        journal::refresh();
        Ok(())
    }

    #[wasm_bindgen(js_name = journalEntries)]
    pub fn journal_entries(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let query: JournalQuery = from_js(&query, "query")?;
        to_js(&self.journal_handle()?.0.entries(&query))
    }

    #[wasm_bindgen(js_name = journalClear)]
    pub fn journal_clear(&self) -> Result<(), JsValue> {
        self.journal_handle()?.0.clear();
        Ok(())
    }

    /// The changes since the last call, for the shell to store.
    #[wasm_bindgen(js_name = journalDrain)]
    pub fn journal_drain(&self) -> Result<JsValue, JsValue> {
        let name = self.workspace().collection().name.clone();
        let queue = queue(&name).ok_or_else(not_open)?;
        let changes: Vec<Change> = queue
            .drain()
            .into_iter()
            .map(|op| match op {
                StorageOp::Append(entry) => Change::Append { entry },
                StorageOp::Rewrite(entries) => Change::Rewrite { entries },
                StorageOp::Settings(settings) => Change::Settings { settings },
            })
            .collect();
        to_js(&changes)
    }

    #[wasm_bindgen(js_name = journalStates)]
    pub fn journal_states(&self, ids: JsValue) -> Result<JsValue, JsValue> {
        let ids: Vec<u64> = from_js(&ids, "ids")?;
        to_js(&self.journal_handle()?.0.states(&ids))
    }

    #[wasm_bindgen(js_name = journalPlanManual)]
    pub fn journal_plan_manual(&self, revert: JsValue, input: JsValue) -> Result<Promise, JsValue> {
        let revert: ManualRevert = from_js(&revert, "revert")?;
        let input: SendInput = from_js(&input, "input")?;
        let (journal, _) = self.journal_handle()?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let entry = journal.entry(revert.entry_id).ok_or_else(|| {
                JsValue::from_str(&format!("journal entry #{} not found", revert.entry_id))
            })?;
            let plan =
                journal::plan_manual(&snapshot, &entry, &revert.request_path).map_err(app_error)?;
            to_js(&plan)
        }))
    }

    #[wasm_bindgen(js_name = journalPlanCompensations)]
    pub fn journal_plan_compensations(
        &self,
        items: JsValue,
        input: JsValue,
    ) -> Result<Promise, JsValue> {
        let items: Vec<CompensationItem> = from_js(&items, "items")?;
        let input: SendInput = from_js(&input, "input")?;
        let (journal, _) = self.journal_handle()?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let plan =
                journal::plan_compensations(&snapshot, &journal, &items).map_err(app_error)?;
            to_js(&plan)
        }))
    }

    #[wasm_bindgen(js_name = journalExecute)]
    pub fn journal_execute(
        &self,
        plan: JsValue,
        options: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
    ) -> Result<Promise, JsValue> {
        let plan: RevertPlan = from_js(&plan, "plan")?;
        let options: RevertOptions = from_js(&options, "options")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion = companion_of(&companion)?;
        let parts = SendServices::from_js(&services)?;
        let (journal, _) = self.journal_handle()?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let services = parts.services();
            let send_options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let outcome = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    let env = RevertEnv {
                        snapshot: &snapshot,
                        transport: &t,
                        options: &send_options,
                        services: &services,
                        caller: "app",
                    };
                    journal::execute(&env, Some(&journal), &plan, &options, &|_| {}, &|| false)
                        .await
                }
                None => {
                    let t = FetchTransport::new();
                    let env = RevertEnv {
                        snapshot: &snapshot,
                        transport: &t,
                        options: &send_options,
                        services: &services,
                        caller: "app",
                    };
                    journal::execute(&env, Some(&journal), &plan, &options, &|_| {}, &|| false)
                        .await
                }
            };
            to_js(&outcome.map_err(app_error)?)
        }))
    }

    #[wasm_bindgen(js_name = journalPreRead)]
    pub fn journal_pre_read(
        &self,
        request: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
    ) -> Result<Promise, JsValue> {
        let request: RequestSpec = from_js(&request, "request")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion = companion_of(&companion)?;
        let parts = SendServices::from_js(&services)?;
        let (journal, _) = self.journal_handle()?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let services = parts.services();
            let send_options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let state = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    let env = RevertEnv {
                        snapshot: &snapshot,
                        transport: &t,
                        options: &send_options,
                        services: &services,
                        caller: "app",
                    };
                    journal::pre_read(&env, &journal, &request).await
                }
                None => {
                    let t = FetchTransport::new();
                    let env = RevertEnv {
                        snapshot: &snapshot,
                        transport: &t,
                        options: &send_options,
                        services: &services,
                        caller: "app",
                    };
                    journal::pre_read(&env, &journal, &request).await
                }
            };
            to_js(&state.map_err(app_error)?)
        }))
    }
}
