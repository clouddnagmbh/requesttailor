// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand D: SAP system landscapes.
//! - Collection tab `landscape`: create DEV/QAS/PRD environments from a template (host,
//!   `sapClient`, `sapLanguage`, user, password as secret without value), shown and added via
//!   `ViewReply::add_environments`.
//! - `prepare`: adds `sap-client` and `sap-language` query parameters from the variables
//!   `sapClient` / `sapLanguage` of the environment unless the URL has them.
//! - Importer `sap-hub`: an API specification downloaded from the SAP Business Accelerator
//!   Hub (EDMX or OpenAPI JSON with `x-sap-*` extensions): requests via
//!   [`crate::metadata::requests_for`], environments for the sandbox and a DEV/QAS/PRD
//!   template. No Hub data is in the repo; tests use synthetic files.

use rt_plugin_sdk::api::{
    ImportReply, PluginContext, PluginFile, PluginRequest, PrepareReply, ViewReply,
};
use rt_plugin_sdk::{Action, Result, Tab};

pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    let _ = (request, context);
    Ok(PrepareReply::default())
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let _ = tab;
    Err(crate::todo("the landscape tab"))
}

pub fn action(action: Action) -> Result<ViewReply> {
    let _ = action;
    Err(crate::todo("the landscape tab"))
}

pub fn detect_hub(files: &[PluginFile]) -> Result<bool> {
    let _ = files;
    Ok(false)
}

pub fn import_hub(files: &[PluginFile]) -> Result<ImportReply> {
    let _ = files;
    Err(crate::todo("the sap-hub importer"))
}
