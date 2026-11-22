// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins as the UI sees them (wave 5). The API types themselves are `rt_plugin::api`; the
//! shells keep a `rt_plugin::host::PluginHost` and persist installed modules (desktop and CLI
//! share one plugin directory, the web shell keeps them in browser storage).
//!
//! Owner in wave 5: strand A (install, permissions, enable). Tabs are rendered by the UI from
//! `rt_plugin::api::View` (strand B). Changes here are interface changes.
//!
//! Besides the types, the parts every shell needs the same way (strand A):
//! - [`ResponseHeads`] and [`Recording`]: response tabs get the response from the response
//!   store (body) and from the head recorded at send time (status, headers, URL), since the
//!   store keeps bodies only;
//! - [`plugin_files`] and [`plugin_import`]: an importer's input, and its result checked like
//!   a built-in import before it reaches `Imports::add`.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine as _;
use rt_core::services::{HookContext, LocalBoxFuture, Note, RequestHooks};
use rt_core::transport::{HttpRequest, HttpVersion, ResponseHead, Timing};
use rt_format::FileSet;
use rt_format::import::{Import, ImportFormat, SourceFile};
use serde::{Deserialize, Serialize};

pub use rt_plugin::api::{
    ActionButton, Block, Contributions, FieldKind, FormField, ImportReply, ImporterContribution,
    Manifest, Permissions, PluginFile, PluginResponse, TabContribution, TabPlace, TabTarget,
    TreeItem, View, ViewReply,
};
pub use rt_plugin::host::{
    LoadedPlugin, PLUGIN_BODY_LIMIT, Plugin, PluginError, PluginHost, response_to_plugin,
};

use crate::AppError;

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub manifest: Manifest,
    /// The user granted `manifest.permissions` at install time (all or nothing).
    pub granted: bool,
    pub enabled: bool,
}

impl From<&LoadedPlugin> for PluginInfo {
    fn from(p: &LoadedPlugin) -> Self {
        Self {
            manifest: p.manifest.clone(),
            granted: p.fully_granted(),
            enabled: p.enabled,
        }
    }
}

impl From<PluginError> for AppError {
    fn from(e: PluginError) -> Self {
        AppError::Plugin(e.to_string())
    }
}

/// The head of the last response per collection and request, recorded at send time.
#[derive(Debug, Default)]
pub struct ResponseHeads(Mutex<HashMap<(String, String), ResponseHead>>);

impl ResponseHeads {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, collection: &str, request_path: &str, head: &ResponseHead) {
        let mut map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        // Heads are small, but a long session must not grow without bound.
        if map.len() > 1000 {
            map.clear();
        }
        map.insert(
            (collection.to_owned(), request_path.to_owned()),
            head.clone(),
        );
    }

    pub fn get(&self, collection: &str, request_path: &str) -> Option<ResponseHead> {
        let map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&(collection.to_owned(), request_path.to_owned()))
            .cloned()
    }

    /// The response for a response tab: `body` from the response store, the head recorded
    /// for that request (status 0 and no headers if there is none, e.g. a plugin installed
    /// after the send).
    pub fn tab_response(
        &self,
        collection: &str,
        request_path: &str,
        body: &[u8],
    ) -> PluginResponse {
        let head = self.get(collection, request_path).unwrap_or(ResponseHead {
            status: 0,
            version: HttpVersion::Unknown,
            headers: Vec::new(),
            url: String::new(),
            redirects: Vec::new(),
            timing: Timing::default(),
        });
        response_to_plugin(&head, body, false, PLUGIN_BODY_LIMIT, &Default::default())
    }
}

/// The hooks of one send: the plugins' (if any), and the response head recorded for
/// response tabs.
pub struct Recording<'a> {
    pub hooks: Option<&'a dyn RequestHooks>,
    pub heads: &'a ResponseHeads,
    pub collection: &'a str,
}

impl RequestHooks for Recording<'_> {
    fn before_send<'a>(
        &'a self,
        request: HttpRequest,
        secrets: &'a [String],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Result<(HttpRequest, Vec<Note>), String>> {
        match self.hooks {
            Some(h) => h.before_send(request, secrets, context),
            None => Box::pin(async move { Ok((request, Vec::new())) }),
        }
    }

    fn after_response<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Vec<Note>> {
        self.heads
            .record(self.collection, &context.request_path, head);
        match self.hooks {
            Some(h) => h.after_response(head, body, context),
            None => Box::pin(async { Vec::new() }),
        }
    }
}

/// Files for an importer (base64, names as picked).
pub fn plugin_files(files: &[SourceFile<'_>]) -> Vec<PluginFile> {
    files
        .iter()
        .map(|f| PluginFile {
            name: f
                .name
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(f.name)
                .to_owned(),
            base64: base64::engine::general_purpose::STANDARD.encode(f.data),
        })
        .collect()
}

/// A plugin importer's result as an [`Import`]: paths must stay inside the new folder and
/// the files must be a collection `rt-format` reads.
pub fn plugin_import(reply: ImportReply) -> Result<Import, AppError> {
    let invalid = |m: String| AppError::Import(format!("the plugin's import is invalid: {m}"));
    let mut files = FileSet::default();
    for (path, text) in reply.files {
        let safe = !path.is_empty()
            && !path.starts_with(['/', '\\'])
            && !path.contains(['\\', ':', '\0'])
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != "..");
        if !safe {
            return Err(invalid(format!(
                "path {path:?} leaves the collection folder"
            )));
        }
        if files
            .files
            .insert(path.clone(), text.into_bytes())
            .is_some()
        {
            return Err(invalid(format!("{path} appears twice")));
        }
    }
    rt_format::read_collection(&files).map_err(|e| invalid(e.to_string()))?;
    let mut report = reply.report;
    report.format = ImportFormat::Plugin;
    let name = reply.collection_name.trim();
    Ok(Import {
        collection_name: if name.is_empty() {
            "Imported collection".to_owned()
        } else {
            name.to_owned()
        },
        files,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_format::import::LossReport;

    fn reply(files: Vec<(&str, &str)>) -> ImportReply {
        ImportReply {
            collection_name: "Demo".into(),
            files: files
                .into_iter()
                .map(|(p, t)| (p.to_owned(), t.to_owned()))
                .collect(),
            report: LossReport {
                format: ImportFormat::Postman,
                requests: 1,
                folders: 0,
                environments: 0,
                losses: Vec::new(),
            },
        }
    }

    const ROOT: &str = "opencollection: 1.0.0\n\ninfo:\n  name: Demo\n";
    const REQ: &str = "info:\n  name: Hello\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://example.com/\n";

    #[test]
    fn plugin_imports_are_checked() {
        let ok = plugin_import(reply(vec![
            ("opencollection.yml", ROOT),
            ("hello.yml", REQ),
        ]))
        .unwrap();
        assert_eq!(
            ok.report.format,
            ImportFormat::Plugin,
            "the format is always plugin"
        );
        assert_eq!(ok.files.files.len(), 2);
        for bad in [
            "../x.yml",
            "/etc/x.yml",
            "a//b.yml",
            "c:\\x.yml",
            "a/./b.yml",
        ] {
            let e =
                plugin_import(reply(vec![("opencollection.yml", ROOT), (bad, REQ)])).unwrap_err();
            assert!(matches!(e, AppError::Import(_)), "{bad}");
        }
        assert!(
            plugin_import(reply(vec![("hello.yml", REQ)])).is_err(),
            "no collection file"
        );
        assert!(plugin_import(reply(vec![("opencollection.yml", "info: [")])).is_err());
    }

    #[test]
    fn heads_are_recorded_per_request() {
        let heads = ResponseHeads::new();
        let head = ResponseHead {
            status: 201,
            version: HttpVersion::Http11,
            headers: vec![("Content-Type".into(), "application/json".into())],
            url: "https://a.test/".into(),
            redirects: Vec::new(),
            timing: Timing::default(),
        };
        heads.record("c", "a.yml", &head);
        let r = heads.tab_response("c", "a.yml", b"{}");
        assert_eq!((r.status, r.body.text.as_deref()), (201, Some("{}")));
        assert_eq!(heads.tab_response("c", "b.yml", b"").status, 0);
        assert_eq!(heads.tab_response("other", "a.yml", b"").status, 0);
    }

    #[test]
    fn files_for_importers() {
        let files = plugin_files(&[SourceFile {
            name: "dir/x.edmx",
            data: b"hi",
        }]);
        assert_eq!(files[0].name, "x.edmx");
        assert_eq!(files[0].base64, "aGk=");
    }
}
