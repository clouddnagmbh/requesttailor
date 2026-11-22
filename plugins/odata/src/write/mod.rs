// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand C: writes.
//! - CSRF (`prepare`): before a modifying request (POST, PUT, PATCH, MERGE, DELETE) without
//!   `X-CSRF-Token`, fetch one with `GET <service root>` and `X-CSRF-Token: Fetch` in the
//!   session of the send ([`rt_plugin_sdk::host::http_in_session`]), cache it per service
//!   root, set it; on a `403` with `X-CSRF-Token: Required` the next send fetches anew.
//! - ETag (`response`, `prepare`): remember `ETag` headers and `@odata.etag` /
//!   `__metadata.etag` per entity URL; set `If-Match` on PUT/PATCH/MERGE/DELETE when the
//!   request has none.
//! - `$batch` (request tab `batch`): compose a multipart `$batch` body (V2 and V4 multipart;
//!   V4 JSON batch too) with read parts and changesets; show the parts of the response.
//! - Drafts (request tab `draft`): RAP and CAP draft actions (`draftEdit`, `draftPrepare`,
//!   `draftActivate`, discard) for draft-enabled entity sets.
//!
//! Interface: [`batch`] (body building and response parsing, used by the tab).

pub mod batch;

use rt_plugin_sdk::api::{
    NotesReply, PluginContext, PluginRequest, PluginResponse, PrepareReply, ViewReply,
};
use rt_plugin_sdk::{Action, Result, Tab};

pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    let _ = (request, context);
    Ok(PrepareReply::default())
}

pub fn response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
) -> Result<NotesReply> {
    let _ = (request, response, context);
    Ok(NotesReply::default())
}

pub fn render_batch(tab: Tab) -> Result<ViewReply> {
    let _ = tab;
    Err(crate::todo("the batch tab"))
}

pub fn action_batch(action: Action) -> Result<ViewReply> {
    let _ = action;
    Err(crate::todo("the batch tab"))
}

pub fn render_draft(tab: Tab) -> Result<ViewReply> {
    let _ = tab;
    Err(crate::todo("the draft tab"))
}

pub fn action_draft(action: Action) -> Result<ViewReply> {
    let _ = action;
    Err(crate::todo("the draft tab"))
}
