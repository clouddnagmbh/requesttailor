// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand A: `$metadata` (EDMX, V2 `edmx:Edmx Version="1.0"` with `m:DataServiceVersion`,
//! V4 `Version="4.0"`) into [`Model`]; the cache per service root in plugin storage; the
//! entity browser (collection tab `entities`); the `edmx` importer (a request per entity set:
//! list, read by key, create, update, delete as the set allows; operations).
//!
//! Interface (used by B, C, D): [`parse`], [`cached`], [`store`], [`requests_for`].

use rt_plugin_sdk::api::{
    ImportReply, NewRequest, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse,
    ViewReply,
};
use rt_plugin_sdk::{Action, Result, Tab};

use crate::edm::Model;

/// Parses an EDMX document; `service_root` is where it came from (with trailing `/`).
pub fn parse(xml: &str, service_root: &str) -> Result<Model> {
    let _ = (xml, service_root);
    Err(crate::todo("metadata::parse"))
}

/// The cached model of `service_root`, if any (plugin storage key
/// `service::storage_key("metadata", root)`).
pub fn cached(service_root: &str) -> Result<Option<Model>> {
    let _ = service_root;
    Ok(None)
}

/// Caches `model` under its service root and adds the root to the list of known roots.
pub fn store(model: &Model) -> Result<()> {
    let _ = model;
    Err(crate::todo("metadata::store"))
}

/// Service roots with cached metadata (for [`crate::service::split`]).
pub fn known_roots() -> Result<Vec<String>> {
    Ok(Vec::new())
}

/// Requests for the collection from a model: per entity set list/read/create/update/delete
/// (as allowed), per operation one request. Folder per entity set. URLs use `{{baseUrl}}`
/// for the service root.
pub fn requests_for(model: &Model) -> Vec<NewRequest> {
    let _ = model;
    Vec::new()
}

/// `response` hook: a `$metadata` response is parsed and cached.
pub fn response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
) -> Result<NotesReply> {
    let _ = (request, response, context);
    Ok(NotesReply::default())
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let _ = tab;
    Err(crate::todo("the entity browser"))
}

pub fn action(action: Action) -> Result<ViewReply> {
    let _ = action;
    Err(crate::todo("the entity browser"))
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    let _ = files;
    Ok(false)
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    let _ = files;
    Err(crate::todo("the edmx importer"))
}
