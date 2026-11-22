// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand D: SAP system landscapes.
//! - Collection tab `landscape` ([`template`]): create DEV/QAS/PRD environments from a form
//!   (host, `sapClient`, `sapLanguage`, user, password as a secret without value, PRD marked
//!   with `rtProduction=true`, optional Hub sandbox with `apiKey` as a secret), added via
//!   `ViewReply::add_environments`.
//! - `prepare` ([`params`]): adds `sap-client` and `sap-language` query parameters from the
//!   variables `sapClient` / `sapLanguage` unless the URL has them; drops an `APIKey` header
//!   without value and, in the sandbox, `Authorization`.
//! - Importer `sap-hub` ([`hub`]): an API specification downloaded from the SAP Business
//!   Accelerator Hub (OpenAPI JSON with `x-sap-*` extensions, or EDMX from the sandbox):
//!   requests ([`openapi`], or [`crate::metadata::requests_for`]), environments for the
//!   sandbox and a DEV/QAS/PRD template, a loss report. Written as OpenCollection files by
//!   [`files`]. No Hub data is in the repo; tests use synthetic files (`testdata/hub/`).

pub mod files;
pub mod hub;
pub mod openapi;
pub mod params;
pub mod template;

use rt_plugin_sdk::api::{
    ImportReply, PluginContext, PluginFile, PluginRequest, PrepareReply, ViewReply,
};
use rt_plugin_sdk::{Action, Result, Tab};

pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    params::prepare(request, context)
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    template::render(tab)
}

pub fn action(action: Action) -> Result<ViewReply> {
    template::action(action)
}

pub fn detect_hub(files: &[PluginFile]) -> Result<bool> {
    hub::detect(files)
}

pub fn import_hub(files: &[PluginFile]) -> Result<ImportReply> {
    hub::import(files)
}
