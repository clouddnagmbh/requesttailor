// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC services and message templates, GraphQL schemas in the browser (wave 8, strand B).
//!
//! ```text
//! Workspace.grpcServices(path, input) -> Promise<GrpcService[]>
//! Workspace.grpcTemplate(path, method, input) -> Promise<string>
//! Workspace.graphqlSchema(path, input, refresh, companion, services) -> Promise<string>
//! ```
//!
//! `input` is an `rt_app::SendInput` (with the secret values), `companion` and `services` as
//! for `Workspace.send`. gRPC descriptors come from the proto files of the opened folder
//! (`protoFilePath` of the request or `config.protobuf`); server reflection needs a stream
//! transport, which in the browser is the companion's relay (strand A) and rejects with kind
//! `unsupported` until it is there. The GraphQL introspection query goes out like a send
//! (`fetch`, or the companion when it is on); the SDL is cached per environment and URL until
//! `refresh`. Errors reject with an `AppError` (`kind`, `message`).

use std::cell::RefCell;

use js_sys::Promise;
use rt_app::graphql::{self, SchemaCache};
use rt_app::{AppError, SendInput, grpc};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

thread_local! {
    static SCHEMAS: RefCell<SchemaCache> = RefCell::new(SchemaCache::default());
}

fn local_descriptors(snapshot: &grpc::GrpcSnapshot) -> Result<grpc::Descriptors, AppError> {
    snapshot.local_descriptors().unwrap_or_else(|| {
        Err(AppError::Unsupported(
            "gRPC server reflection in the browser goes through the companion's stream relay; \
             until then, add the proto files to the collection (protoFilePath or \
             config.protobuf)"
                .into(),
        ))
    })
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Services of the gRPC request at `path`; see the module documentation.
    #[wasm_bindgen(js_name = grpcServices)]
    pub fn grpc_services(&self, path: String, input: JsValue) -> Result<Promise, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let snapshot = grpc::snapshot(&self.workspace(), &path, input).map_err(app_error)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let d = local_descriptors(&snapshot).map_err(app_error)?;
            to_js(&d.services())
        }))
    }

    /// The JSON skeleton of the input message of `method`.
    #[wasm_bindgen(js_name = grpcTemplate)]
    pub fn grpc_template(
        &self,
        path: String,
        method: String,
        input: JsValue,
    ) -> Result<Promise, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let snapshot = grpc::snapshot(&self.workspace(), &path, input).map_err(app_error)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let d = local_descriptors(&snapshot).map_err(app_error)?;
            let template = d.template(&method).map_err(|e| app_error(e.into()))?;
            Ok(JsValue::from_str(&template))
        }))
    }

    /// The schema of the GraphQL endpoint of the request at `path`, as SDL.
    #[wasm_bindgen(js_name = graphqlSchema)]
    pub fn graphql_schema(
        &self,
        path: String,
        input: JsValue,
        refresh: bool,
        companion: JsValue,
        services: JsValue,
    ) -> Result<Promise, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let (key, snapshot) = {
            let ws = self.workspace();
            let key = graphql::cache_key(&ws, &path, &input).map_err(app_error)?;
            let snapshot = graphql::introspection_snapshot(&ws, &path, input).map_err(app_error)?;
            (key, snapshot)
        };
        if !refresh && let Some(sdl) = SCHEMAS.with(|c| c.borrow().get(&key).map(str::to_owned)) {
            return Ok(Promise::resolve(&JsValue::from_str(&sdl)));
        }
        let parts = crate::services_js::SendServices::from_js(&services)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let services = parts.services();
            let report = match companion {
                Some(c) => {
                    let transport = CompanionTransport::new(c.url, c.token);
                    snapshot.send_with(&transport, &options, &services).await
                }
                None => {
                    snapshot
                        .send_with(&FetchTransport::new(), &options, &services)
                        .await
                }
            };
            let sdl = graphql::schema_from_report(&report).map_err(app_error)?;
            SCHEMAS.with(|c| c.borrow_mut().put(key, sdl.clone()));
            Ok(JsValue::from_str(&sdl))
        }))
    }
}
