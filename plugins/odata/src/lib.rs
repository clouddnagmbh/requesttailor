// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OData plugin (wave 6): OData V2 and V4 services as found in SAP S/4HANA, CAP and RAP,
//! built only on the public plugin API (`crates/rt-plugin/API.md`). The core knows nothing
//! about OData or SAP; what this plugin misses in the API is listed in `API-WUENSCHE.md`.
//!
//! Modules, one per strand of wave 6:
//! - [`metadata`] (A): read `$metadata` (V2 and V4) into [`edm::Model`], cache it per service
//!   root, the entity browser (collection tab `entities`), and the `edmx` importer.
//! - [`query`] (B): the query builder (request tab `query`): `$filter`, `$expand`, `$select`,
//!   `$orderby`, `$top`, `$skip`, `$count`, from the cached model.
//! - [`write`] (C): CSRF token handshake and `If-Match` from remembered ETags (`prepare`,
//!   `response`), `$batch` with changesets (request tab `batch`), draft actions of RAP and
//!   CAP (request tab `draft`).
//! - [`landscape`] (D): SAP system landscapes (collection tab `landscape`: DEV/QAS/PRD
//!   environments with `sap-client` and language), the `sap-client`/`sap-language` query
//!   parameters (`prepare`), and the `sap-hub` importer.
//!
//! Shared: [`edm`] (the service model) and [`service`] (service root of a URL, storage keys).
//! Hooks that several modules contribute to are combined here, in a fixed order.

pub mod edm;
pub mod landscape;
pub mod metadata;
pub mod query;
pub mod service;
pub mod write;

use rt_plugin_sdk::api::{
    ImportReply, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse,
    PrepareReply, RequestPatch, ViewReply,
};
use rt_plugin_sdk::{Action, Error, Plugin, Result, Tab};

/// The manifest, embedded into the `rt-plugin` custom section by [`rt_plugin_sdk::plugin!`].
pub const MANIFEST: &[u8] = include_bytes!("../plugin.json");

pub struct OData;

rt_plugin_sdk::plugin!(OData, manifest = MANIFEST);

impl Plugin for OData {
    /// Order: landscape (query parameters `sap-client`, `sap-language`) first, so that the
    /// CSRF fetch of `write` goes to the same client; then `write` (CSRF token, If-Match).
    fn prepare(&self, request: PluginRequest, context: PluginContext) -> Result<PrepareReply> {
        let first = landscape::prepare(&request, &context)?;
        let request = apply(request, &first.patch);
        let second = write::prepare(&request, &context)?;
        Ok(PrepareReply {
            patch: merge(first.patch, second.patch),
            notes: first.notes.into_iter().chain(second.notes).collect(),
        })
    }

    fn response(
        &self,
        request: PluginRequest,
        response: PluginResponse,
        context: PluginContext,
    ) -> Result<NotesReply> {
        // `metadata` caches `$metadata` responses; `write` remembers ETags and CSRF tokens.
        let mut notes = metadata::response(&request, &response, &context)?.notes;
        notes.extend(write::response(&request, &response, &context)?.notes);
        Ok(NotesReply { notes })
    }

    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        match tab.tab.as_str() {
            "entities" => metadata::render(tab),
            "query" => query::render(tab),
            "batch" => write::render_batch(tab),
            "draft" => write::render_draft(tab),
            "landscape" => landscape::render(tab),
            other => Err(Error::new(format!("unknown tab {other}"))),
        }
    }

    fn action(&self, action: Action) -> Result<ViewReply> {
        match action.tab.as_str() {
            "entities" => metadata::action(action),
            "query" => query::action(action),
            "batch" => write::action_batch(action),
            "draft" => write::action_draft(action),
            "landscape" => landscape::action(action),
            other => Err(Error::new(format!("unknown tab {other}"))),
        }
    }

    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        match importer {
            "edmx" => metadata::detect(&files),
            "sap-hub" => landscape::detect_hub(&files),
            other => Err(Error::new(format!("unknown importer {other}"))),
        }
    }

    fn import(&self, importer: &str, files: Vec<PluginFile>) -> Result<ImportReply> {
        match importer {
            "edmx" => metadata::import(&files),
            "sap-hub" => landscape::import_hub(&files),
            other => Err(Error::new(format!("unknown importer {other}"))),
        }
    }
}

/// The request as the host would see it after `patch` (URL and headers; the body is kept).
fn apply(mut request: PluginRequest, patch: &RequestPatch) -> PluginRequest {
    if let Some(url) = &patch.url {
        request.url = url.clone();
    }
    request.headers.retain(|(name, _)| {
        !patch
            .remove_headers
            .iter()
            .chain(patch.set_headers.iter().map(|(n, _)| n))
            .any(|r| r.eq_ignore_ascii_case(name))
    });
    request.headers.extend(patch.set_headers.iter().cloned());
    if let Some(body) = &patch.body {
        request.body = Some(body.clone());
    }
    request
}

/// `second` applied after `first`, as one patch.
fn merge(first: RequestPatch, second: RequestPatch) -> RequestPatch {
    let mut set_headers: Vec<(String, String)> = first
        .set_headers
        .into_iter()
        .filter(|(n, _)| {
            !second
                .remove_headers
                .iter()
                .chain(second.set_headers.iter().map(|(n, _)| n))
                .any(|r| r.eq_ignore_ascii_case(n))
        })
        .collect();
    set_headers.extend(second.set_headers);
    let mut remove_headers = first.remove_headers;
    remove_headers.extend(second.remove_headers);
    RequestPatch {
        url: second.url.or(first.url),
        remove_headers,
        set_headers,
        body: second.body.or(first.body),
    }
}

/// Placeholder of the interface commit: the strand replaces it.
pub(crate) fn todo(what: &str) -> Error {
    Error::new(format!("odata: {what} is not implemented yet"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_patches_win() {
        let a = RequestPatch {
            url: Some("http://h/a?sap-client=100".into()),
            set_headers: vec![("X-A".into(), "1".into()), ("X-B".into(), "1".into())],
            ..RequestPatch::default()
        };
        let b = RequestPatch {
            set_headers: vec![("x-b".into(), "2".into())],
            ..RequestPatch::default()
        };
        let m = merge(a, b);
        assert_eq!(m.url.as_deref(), Some("http://h/a?sap-client=100"));
        assert_eq!(
            m.set_headers,
            vec![("X-A".into(), "1".into()), ("x-b".into(), "2".into())]
        );
    }
}
