// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI wizard in the web shell, over `rt_app::openapi` and
//! `rt_app::contract::{requirements, validate_response}`:
//!
//! ```text
//! openapiFetch(url, companion | null) -> Promise<FetchedSpec>   // fetch, or through rt agent
//! openapiInspect(files: { name, data }[]) -> SpecOverview
//! Importer.openapiPlan(files, options) -> ImportPlan            // written with take / addImport
//! Workspace.openapiSpecPath() -> string | null                  // named in opencollection.yml
//! Workspace.openapiRequirements(path, input, specs) -> SendRequirements | null
//! Workspace.openapiValidateResponse(path, responseId, specs) -> ResponseCheck | null
//! ```
//!
//! The browser reads files asynchronously, the core synchronously: the web shell reads the
//! spec's saved files first and passes them as `specs` (`[path, Uint8Array][]`; a path not in
//! the list reads as missing). Staged spec files (a spec just kept by the wizard) come from the
//! workspace itself. The parsed spec is cached between calls while its bytes stay the same.

use std::cell::RefCell;
use std::collections::HashMap;

use js_sys::{Array, Promise, Reflect, Uint8Array};
use rt_app::contract::SpecCache;
use rt_app::import::SourceFile;
use rt_app::openapi::OpenApiOptions;
use rt_app::{AppError, SendInput};
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::import_js::Importer;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

thread_local! {
    static CACHE: RefCell<SpecCache> = RefCell::new(SpecCache::default());
}

fn type_error(message: &str) -> JsValue {
    js_sys::TypeError::new(message).into()
}

/// `(name, bytes)` of each `{ name, data }` in `files`.
fn read_files(files: &Array) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let mut out = Vec::with_capacity(files.length() as usize);
    for entry in files.iter() {
        let name = Reflect::get(&entry, &"name".into())?
            .as_string()
            .ok_or_else(|| type_error("files[].name must be a string"))?;
        let data = Reflect::get(&entry, &"data".into())?
            .dyn_into::<Uint8Array>()
            .map_err(|_| type_error("files[].data must be a Uint8Array"))?;
        out.push((name, data.to_vec()));
    }
    Ok(out)
}

fn sources(owned: &[(String, Vec<u8>)]) -> Vec<SourceFile<'_>> {
    owned
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect()
}

/// `specs` as path → bytes (`[path, Uint8Array][]`).
fn spec_map(specs: &Array) -> Result<HashMap<String, Vec<u8>>, JsValue> {
    let mut out = HashMap::new();
    for entry in specs.iter() {
        let pair: Array = entry
            .dyn_into()
            .map_err(|_| type_error("specs[] must be [path, bytes]"))?;
        let path = pair
            .get(0)
            .as_string()
            .ok_or_else(|| type_error("specs[][0] must be a string"))?;
        let data = pair
            .get(1)
            .dyn_into::<Uint8Array>()
            .map_err(|_| type_error("specs[][1] must be a Uint8Array"))?;
        out.insert(path, data.to_vec());
    }
    Ok(out)
}

/// Reads from `specs`; a leading `/` means the same as none (the folder is the repository).
fn reader(specs: &HashMap<String, Vec<u8>>) -> impl Fn(&str) -> Result<Option<Vec<u8>>, AppError> {
    |p: &str| Ok(specs.get(p.trim_start_matches('/')).cloned())
}

/// `FetchedSpec` for `url` and the files its relative `$ref`s name; through the companion
/// when `companion` is given (`{ url, token }`), else with the browser's `fetch` (CORS).
#[wasm_bindgen(js_name = openapiFetch)]
pub fn openapi_fetch(url: String, companion: JsValue) -> Result<Promise, JsValue> {
    let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
        None
    } else {
        Some(from_js(&companion, "companion")?)
    };
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let fetched = match companion {
            Some(c) => {
                let transport = CompanionTransport::new(c.url, c.token);
                rt_app::openapi::fetch_spec(&transport, &url).await
            }
            None => rt_app::openapi::fetch_spec(&FetchTransport::new(), &url).await,
        };
        to_js(&fetched.map_err(app_error)?)
    }))
}

/// `SpecOverview` of the spec in `files` (`{ name, data }[]`).
#[wasm_bindgen(js_name = openapiInspect)]
pub fn openapi_inspect(files: Array) -> Result<JsValue, JsValue> {
    let owned = read_files(&files)?;
    to_js(&rt_app::openapi::inspect(&sources(&owned)).map_err(app_error)?)
}

#[wasm_bindgen]
impl Importer {
    /// `ImportPlan` for the spec in `files` with the wizard's `options`; written with `take`
    /// (a new collection) or `Workspace.addImport` (a folder of the open one) like any plan.
    #[wasm_bindgen(js_name = openapiPlan)]
    pub fn openapi_plan(&mut self, files: Array, options: JsValue) -> Result<JsValue, JsValue> {
        let options: OpenApiOptions = from_js(&options, "options")?;
        let owned = read_files(&files)?;
        let plan = self
            .imports
            .analyze_openapi(&sources(&owned), &options)
            .map_err(app_error)?;
        to_js(&plan)
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// The spec path `opencollection.yml` names (unsaved edits included), or `null`: then the
    /// shell passes `openapi.yaml`, `openapi.yml` and `openapi.json` if they exist.
    #[wasm_bindgen(js_name = openapiSpecPath)]
    pub fn openapi_spec_path(&self) -> Result<Option<String>, JsValue> {
        rt_app::contract::configured_spec(&self.workspace()).map_err(app_error)
    }

    /// What request `path` still needs for its spec operation before sending (`null`: no
    /// spec or no matching operation). `input` may carry secret values (they never leave).
    #[wasm_bindgen(js_name = openapiRequirements)]
    pub fn openapi_requirements(
        &self,
        path: &str,
        input: JsValue,
        specs: Array,
    ) -> Result<JsValue, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let specs = spec_map(&specs)?;
        let read = reader(&specs);
        let found = CACHE.with(|cache| {
            rt_app::contract::requirements(
                &self.workspace(),
                path,
                &input,
                &mut cache.borrow_mut(),
                &read,
            )
        });
        to_js(&found.map_err(app_error)?)
    }

    /// The kept response `responseId` of request `path` against its spec operation (`null`:
    /// no spec, no matching operation).
    #[wasm_bindgen(js_name = openapiValidateResponse)]
    pub fn openapi_validate_response(
        &self,
        path: &str,
        response_id: f64,
        specs: Array,
    ) -> Result<JsValue, JsValue> {
        let id = int(response_id)?;
        let specs = spec_map(&specs)?;
        let read = reader(&specs);
        let found = CACHE.with(|cache| {
            rt_app::contract::validate_response(
                &self.workspace(),
                path,
                &self.responses(),
                id,
                &mut cache.borrow_mut(),
                &read,
            )
        });
        to_js(&found.map_err(app_error)?)
    }
}
