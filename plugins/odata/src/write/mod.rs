// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand C: writes.
//! - CSRF (`prepare`): before a modifying request (POST, PUT, PATCH, MERGE, DELETE) without
//!   `X-CSRF-Token`, fetch one with `GET <service root>` and `X-CSRF-Token: Fetch` in the
//!   session of the send ([`rt_plugin_sdk::host::http_in_session`]), cache it per service
//!   root, set it; on a `403` with `X-CSRF-Token: Required` the cached token is dropped and
//!   the next send fetches anew ([`csrf`]).
//! - ETag (`response`, `prepare`): remember `ETag` headers and `@odata.etag` /
//!   `__metadata.etag` per entity URL; set `If-Match` on PUT/PATCH/MERGE/DELETE and on bound
//!   actions when the request has none ([`etag`]).
//! - `$batch` (request tab `batch`): compose a multipart `$batch` body (V2 and V4) with read
//!   parts and changesets; the `response` hook keeps the parts' results for the tab
//!   ([`batch`], [`batch_tab`]).
//! - Drafts (request tab `draft`): RAP and CAP draft actions (edit, prepare, activate,
//!   discard) for draft-enabled entity sets ([`draft`]).
//!
//! Interface: [`batch`] (body building and response parsing, used by the tab).

pub mod batch;
pub mod batch_tab;
pub mod csrf;
pub mod draft;
pub mod etag;
pub mod util;

use rt_plugin_sdk::api::{
    NotesReply, PluginContext, PluginNote, PluginRequest, PluginResponse, PrepareReply, ViewReply,
};
use rt_plugin_sdk::{Action, Result, Tab, header, info, text_body, warning};

use util::MASK;

pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    let _ = context;
    let raw = request.method.to_ascii_uppercase();
    if !util::MODIFYING.contains(&raw.as_str()) {
        return Ok(PrepareReply::default());
    }
    let Some(target) = util::split(&request.url) else {
        return Ok(PrepareReply::default());
    };
    let mut reply = PrepareReply::default();

    // A `$batch` body typed by hand: multipart needs CRLF.
    if target.resource == "$batch"
        && let Some(body) = &request.body
        && let Some(text) = body.text.as_deref().filter(|t| !t.contains(MASK))
        && let Some(fixed) = batch::to_crlf(text)
    {
        reply.patch.body = Some(text_body(body.content_type.as_deref(), fixed));
        reply
            .notes
            .push(info("$batch body sent with CRLF line endings"));
    }

    if header(&request.headers, csrf::HEADER).is_none()
        && !target.service_root.contains(MASK)
        && let Some(token) = csrf::token(request, &target.service_root, &mut reply.notes)?
    {
        reply.patch.set_headers.push((csrf::HEADER.into(), token));
    }

    let method = util::effective_method(request);
    let entity = match method.as_str() {
        "PUT" | "PATCH" | "MERGE" | "DELETE" => Some(request.url.clone()),
        "POST" => etag::action_entity(&target),
        _ => None,
    };
    if let Some(entity) = entity
        && header(&request.headers, "If-Match").is_none()
        && let Some(etag) = etag::lookup(&entity)?.filter(|e| !e.contains(MASK))
    {
        reply.notes.push(info(format!(
            "If-Match: {etag} (ETag from an earlier response)"
        )));
        reply.patch.set_headers.push(("If-Match".into(), etag));
    }
    Ok(reply)
}

pub fn response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
) -> Result<NotesReply> {
    let Some(target) = util::split(&request.url) else {
        return Ok(NotesReply::default());
    };
    let mut notes = Vec::new();
    csrf::on_response(request, response, &target.service_root, &mut notes)?;
    let method = util::effective_method(request);
    if target.resource == "$batch" {
        batch_response(request, response, context, &target, &mut notes)?;
    } else if (200..300).contains(&response.status) {
        if method == "DELETE" {
            etag::forget(&target.service_root, &request.url)?;
        } else {
            let entries = etag::from_response(request, response, &target);
            etag::remember(&target.service_root, &entries)?;
        }
    }
    match response.status {
        412 => notes.push(warning(
            "412 Precondition Failed: the entity changed since its ETag was read; read it again and repeat the change",
        )),
        428 => notes.push(warning(
            "428 Precondition Required: the service wants If-Match; read the entity first so its ETag is known",
        )),
        _ => {}
    }
    Ok(NotesReply { notes })
}

/// The error message of an OData error body (V2 `error.message.value`, V4 `error.message`).
pub(crate) fn error_message(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = &json.get("error")?["message"];
    message
        .get("value")
        .or(Some(message))
        .and_then(|m| m.as_str())
        .map(str::to_owned)
}

fn batch_response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
    target: &crate::service::Target,
    notes: &mut Vec<PluginNote>,
) -> Result<()> {
    let content_type = header(&response.headers, "Content-Type").unwrap_or_default();
    let Some(text) = response.body.text.as_deref() else {
        return Ok(());
    };
    if batch::boundary(content_type).is_none() {
        // A V4 JSON batch: only a summary of the statuses.
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(responses) = json.get("responses").and_then(|r| r.as_array())
        {
            let statuses: Vec<String> = responses
                .iter()
                .map(|r| {
                    let id = r["id"].as_str().unwrap_or("?");
                    format!("{id} {}", r["status"])
                })
                .collect();
            let failed = responses.iter().any(|r| {
                r["status"]
                    .as_u64()
                    .is_none_or(|s| !(200..300).contains(&s))
            });
            let text = format!("$batch (JSON): {}", statuses.join(", "));
            notes.push(if failed { warning(text) } else { info(text) });
        }
        return Ok(());
    }
    let items = request
        .body
        .as_ref()
        .and_then(|b| {
            let ct = header(&request.headers, "Content-Type")
                .or(b.content_type.as_deref())
                .unwrap_or_default();
            batch::parse_request(ct, b.text.as_deref()?)
        })
        .unwrap_or_default();
    let groups = batch::align(&items, batch::parse_groups(content_type, text));

    let mut rows = Vec::new();
    let mut summary = Vec::new();
    let mut failed = false;
    let mut remembered = Vec::new();
    let mut n = 0;
    for (i, group) in groups.iter().enumerate() {
        let ops: Vec<&batch::Part> = match items.get(i) {
            Some(batch::Item::Read(p)) => vec![p],
            Some(batch::Item::Changeset(ps)) => ps.iter().collect(),
            None => Vec::new(),
        };
        let responses: Vec<&batch::PartResponse> = match group {
            batch::Group::Single(r) => vec![r],
            batch::Group::Changeset(rs) => rs.iter().collect(),
        };
        let changeset = responses.first().and_then(|r| r.changeset);
        let ok = responses.iter().all(|r| (200..300).contains(&r.status));
        failed |= !ok;
        let rolled_back =
            changeset.is_some() && matches!(group, batch::Group::Single(_)) && ops.len() > 1;
        match (changeset, ok) {
            (Some(c), true) => summary.push(format!("changeset {}: {} ok", c + 1, responses.len())),
            (Some(c), false) => summary.push(format!(
                "changeset {}: failed with {}, rolled back",
                c + 1,
                responses[0].status
            )),
            (None, true) => summary.push(format!("read: {}", responses[0].status)),
            (None, false) => summary.push(format!("read: failed with {}", responses[0].status)),
        }
        for (j, r) in responses.iter().enumerate() {
            let op = if rolled_back {
                None
            } else {
                ops.get(j).copied()
            };
            n += 1;
            let label = if rolled_back {
                format!("{} operations", ops.len())
            } else {
                op.map(|o| format!("{} {}", o.method, o.url))
                    .unwrap_or_default()
            };
            rows.push(serde_json::json!({
                "n": n,
                "changeset": r.changeset.map(|c| c + 1),
                "operation": label,
                "status": r.status,
                "message": error_message(&r.body).unwrap_or_default(),
            }));
            if let Some(op) = op
                && (200..300).contains(&r.status)
            {
                let url = if op.url.contains("://") {
                    op.url.clone()
                } else {
                    format!("{}{}", target.service_root, op.url.trim_start_matches('/'))
                };
                let part_request = PluginRequest {
                    method: op.method.clone(),
                    url,
                    headers: op.headers.clone(),
                    body: None,
                };
                let part_response = PluginResponse {
                    status: r.status,
                    headers: r.headers.clone(),
                    url: part_request.url.clone(),
                    body: text_body(header(&r.headers, "Content-Type"), r.body.clone()),
                    truncated: false,
                };
                if let Some(part_target) = util::split(&part_request.url) {
                    remembered.extend(etag::from_response(
                        &part_request,
                        &part_response,
                        &part_target,
                    ));
                }
            }
        }
    }
    etag::remember(&target.service_root, &remembered)?;
    rt_plugin_sdk::host::storage_set(
        crate::service::storage_key("batch-result", &context.request_path),
        serde_json::json!({ "root": target.service_root, "rows": rows }),
    )?;
    let text = format!("$batch: {}", summary.join("; "));
    notes.push(if failed { warning(text) } else { info(text) });
    Ok(())
}

pub fn render_batch(tab: Tab) -> Result<ViewReply> {
    batch_tab::render(tab)
}

pub fn action_batch(action: Action) -> Result<ViewReply> {
    batch_tab::action(action)
}

pub fn render_draft(tab: Tab) -> Result<ViewReply> {
    draft::render(tab)
}

pub fn action_draft(action: Action) -> Result<ViewReply> {
    draft::action(action)
}
