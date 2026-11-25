// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand F: consumer contracts for the web shell over `rt_app::consumers`. Both
//! functions work on text the shell read (the browser has no paths and no clones); the shell's
//! `src/consumers.ts` finds the consumers inside the collection's repository.
//!
//! ```text
//! consumersList(opencollectionYml: string) -> ConsumerRef[]
//! consumersRun(consumer: ConsumerRef, files: { path: string, text: string }[],
//!              request: ConsumerVerifyRequest, companion: Companion | null,
//!              services: SendServices | null, cancelled: () => boolean)
//!     -> Promise<ConsumerResult>
//! ```
//!
//! `consumersRun` runs one consumer collection (its files as text; bodies from binary files
//! are not read) against `request.base_url` through the companion when the shell is paired,
//! else `fetch`, with the same services as a send (so the send guard answers per request).
//! Secret values of the consumer are not available in the browser: a secret without a value
//! stays unresolved.

use js_sys::{Function, Promise};
use rt_app::consumers::{
    Budget, ConsumerRef, ConsumerResult, ConsumerVerifyRequest, Loaded, run_consumer,
};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use rt_format::{COLLECTION_FILE, FileSet, read_collection};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[derive(Deserialize)]
struct TextFile {
    path: String,
    text: String,
}

/// The consumers `opencollection.yml` lists.
#[wasm_bindgen(js_name = consumersList)]
pub fn consumers_list(opencollection_yml: &str) -> Result<JsValue, JsValue> {
    let mut files = FileSet::default();
    files.files.insert(
        COLLECTION_FILE.to_owned(),
        opencollection_yml.as_bytes().to_vec(),
    );
    to_js(&rt_app::consumers::list(&files).map_err(app_error)?)
}

/// Runs one consumer; see the module documentation.
#[wasm_bindgen(js_name = consumersRun)]
pub fn consumers_run(
    consumer: JsValue,
    files: JsValue,
    request: JsValue,
    companion: JsValue,
    services: JsValue,
    cancelled: Function,
) -> Result<Promise, JsValue> {
    let consumer: ConsumerRef = from_js(&consumer, "consumer")?;
    let texts: Vec<TextFile> = from_js(&files, "files")?;
    let request: ConsumerVerifyRequest = from_js(&request, "request")?;
    let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
        None
    } else {
        Some(from_js(&companion, "companion")?)
    };
    let parts = SendServices::from_js(&services)?;
    let mut set = FileSet::default();
    for f in texts {
        set.files.insert(f.path, f.text.into_bytes());
    }
    let collection = read_collection(&set)
        .map_err(|e| app_error(rt_app::AppError::from(e)))?
        .model()
        .clone();
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let loaded = Loaded {
            collection,
            files: set,
            commit: None,
        };
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let stop = || {
            cancelled
                .call0(&JsValue::NULL)
                .map(|v| v.is_truthy())
                .unwrap_or(false)
        };
        let mut budget = Budget::new(request.max_minutes);
        let result: ConsumerResult = match companion {
            Some(c) => {
                let t = CompanionTransport::new(c.url, c.token);
                run_consumer(
                    &consumer,
                    loaded,
                    &request,
                    &mut budget,
                    &t,
                    &options,
                    &parts.services(),
                    &stop,
                )
                .await
            }
            None => {
                let t = FetchTransport::new();
                run_consumer(
                    &consumer,
                    loaded,
                    &request,
                    &mut budget,
                    &t,
                    &options,
                    &parts.services(),
                    &stop,
                )
                .await
            }
        };
        to_js(&result)
    }))
}
