// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand A: `$metadata` (EDMX, V2 `edmx:Edmx Version="1.0"` with `m:DataServiceVersion`,
//! V4 `Version="4.0"`) into [`Model`]; the cache per service root in plugin storage; the
//! entity browser (collection tab `entities`); the `edmx` importer (a request per entity set:
//! list, read by key, create, update, delete as the set allows; operations).
//!
//! Interface (used by B, C, D): [`parse`], [`cached`], [`store`], [`known_roots`],
//! [`requests_for`]; for importers [`collection`].
//!
//! Modules: [`xml`] (a small XML reader, no dependency), [`edmx`] (EDMX rules),
//! `cache` (storage format), [`requests`] (requests from a model), `browser` (the tab),
//! `import` (the importer).

mod browser;
mod cache;
pub mod edmx;
mod import;
pub mod requests;
pub mod xml;

use rt_plugin_sdk::api::{
    ImportReply, NewRequest, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse,
    ViewReply,
};
use rt_plugin_sdk::{Action, Error, Result, Tab};

use crate::edm::Model;

pub use browser::{DEFAULT_ENVIRONMENT, metadata_request, normalize_root};
pub use cache::{ROOTS_KEY, forget};
pub use import::{FALLBACK_BASE_URL, collection};
pub use requests::{key_predicate, sample_literal};

/// Parses an EDMX document; `service_root` is where it came from (with trailing `/`).
pub fn parse(xml: &str, service_root: &str) -> Result<Model> {
    edmx::parse(xml, service_root)
        .map(|p| p.model)
        .map_err(Error::new)
}

/// The cached model of `service_root`, if any (plugin storage key
/// `service::storage_key("metadata", root)`).
pub fn cached(service_root: &str) -> Result<Option<Model>> {
    cache::cached(service_root)
}

/// Caches `model` under its service root and adds the root to the list of known roots.
pub fn store(model: &Model) -> Result<()> {
    cache::store(model)
}

/// Service roots with cached metadata (for [`crate::service::split`]).
pub fn known_roots() -> Result<Vec<String>> {
    cache::known_roots()
}

/// Requests for the collection from a model: per entity set list/read/create/update/delete
/// (as allowed), per operation one request. Folder per entity set. URLs use `{{baseUrl}}`
/// for the service root. Details in [`requests`].
pub fn requests_for(model: &Model) -> Vec<NewRequest> {
    requests::requests_for(model)
}

/// `response` hook: a `200` XML response to `<root>$metadata` is parsed and cached, with an
/// info note. Problems become a warning note; the send never fails because of them.
pub fn response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
) -> Result<NotesReply> {
    let _ = context;
    let Some(root) = metadata_root(&request.url) else {
        return Ok(NotesReply::default());
    };
    if response.status != 200 {
        return Ok(NotesReply::default());
    }
    let content_type = rt_plugin_sdk::header(&response.headers, "content-type")
        .or(response.body.content_type.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type.contains("xml") {
        // V4 CSDL JSON (`$format=json`) and anything else: not ours.
        return Ok(NotesReply::default());
    }
    let warn = |text: String| NotesReply {
        notes: vec![rt_plugin_sdk::warning(text)],
        segments: Vec::new(),
    };
    if response.truncated {
        return Ok(warn(
            "OData metadata not cached: the response was cut at the plugin size limit".into(),
        ));
    }
    let text = match rt_plugin_sdk::body_bytes(&response.body)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(t) => t,
        None => {
            return Ok(warn(
                "OData metadata not cached: the body is not UTF-8".into(),
            ));
        }
    };
    let model = match parse(&text, &root) {
        Ok(m) => m,
        Err(e) => return Ok(warn(format!("OData metadata could not be read: {e}"))),
    };
    if let Err(e) = store(&model) {
        return Ok(warn(format!("OData metadata not cached: {e}")));
    }
    Ok(NotesReply {
        notes: vec![rt_plugin_sdk::info(browser::cached_text(&model))],
        segments: Vec::new(),
    })
}

/// The service root of a `…/<root>/$metadata` URL (query and fragment ignored).
fn metadata_root(url: &str) -> Option<String> {
    let base = url.split(['?', '#']).next()?;
    let root = base.strip_suffix("$metadata")?;
    (root.ends_with('/') && root.contains("://")).then(|| root.to_owned())
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    browser::render(tab)
}

pub fn action(action: Action) -> Result<ViewReply> {
    browser::action(action)
}

pub fn detect(files: &[PluginFile]) -> Result<bool> {
    import::detect(files)
}

pub fn import(files: &[PluginFile]) -> Result<ImportReply> {
    import::import(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_urls() {
        assert_eq!(
            metadata_root("https://h/sap/opu/odata/sap/S/$metadata?sap-client=100").as_deref(),
            Some("https://h/sap/opu/odata/sap/S/")
        );
        assert_eq!(
            metadata_root("https://h/$metadata").as_deref(),
            Some("https://h/")
        );
        assert_eq!(metadata_root("https://h/S/Orders"), None);
        assert_eq!(metadata_root("https://h/S/x$metadata"), None);
        assert_eq!(metadata_root("$metadata"), None);
    }
}
