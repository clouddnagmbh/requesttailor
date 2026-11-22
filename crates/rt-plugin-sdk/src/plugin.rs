// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The [`Plugin`] trait: one method per hook of `API.md`. Implement the hooks the manifest
//! contributes; the others keep their default, which answers with an error.

use rt_core::model::RequestSpec;
use rt_core::services::NoteLevel;
use rt_plugin::api::{
    ImportReply, NotesReply, PluginContext, PluginFile, PluginRequest, PluginResponse,
    PrepareReply, TabTarget, ViewReply,
};
use rt_plugin::api::{PluginNote, RequestPatch};

use crate::Result;

/// A plugin. The [`plugin!`](crate::plugin) macro connects one implementation to the binary
/// interface. A plugin is constructed per call: keep it stateless and put state into the tab
/// state ([`ViewReply::state`]) or the storage host calls.
///
/// Every hook gets owned data, so a hook can move parts of its input into its reply.
pub trait Plugin {
    /// `contributes.prepare`: before every send, after auth, before signing. The request is
    /// masked (secret values are `***`); the patch must not contain `***`.
    fn prepare(&self, request: PluginRequest, context: PluginContext) -> Result<PrepareReply> {
        let _ = (request, context);
        Err(not_contributed("prepare"))
    }

    /// `contributes.response`: after every response.
    fn response(
        &self,
        request: PluginRequest,
        response: PluginResponse,
        context: PluginContext,
    ) -> Result<NotesReply> {
        let _ = (request, response, context);
        Err(not_contributed("response"))
    }

    /// `contributes.tabs`: the user opened one of the plugin's tabs.
    fn render_tab(&self, tab: Tab) -> Result<ViewReply> {
        let _ = tab;
        Err(not_contributed("render-tab"))
    }

    /// The user clicked an action, a tree item or submitted a form in one of the tabs.
    fn action(&self, action: Action) -> Result<ViewReply> {
        let _ = action;
        Err(not_contributed("action"))
    }

    /// `contributes.importers`: whether `files` look like this importer's format.
    fn detect(&self, importer: &str, files: Vec<PluginFile>) -> Result<bool> {
        let _ = (importer, files);
        Err(not_contributed("detect"))
    }

    /// `contributes.importers`: converts `files` into OpenCollection files and a loss report.
    fn import(&self, importer: &str, files: Vec<PluginFile>) -> Result<ImportReply> {
        let _ = (importer, files);
        Err(not_contributed("import"))
    }
}

fn not_contributed(hook: &str) -> crate::Error {
    crate::Error::new(format!("this plugin does not implement the {hook} hook"))
}

/// The `render-tab` call.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    /// The tab id from the manifest.
    pub tab: String,
    pub target: TabTarget,
    /// The request as edited: unsaved changes included, templates unresolved, no secrets.
    pub spec: RequestSpec,
    /// For response tabs: the response.
    pub response: Option<PluginResponse>,
    /// The last [`ViewReply::state`] of this tab, `None` the first time.
    pub state: Option<serde_json::Value>,
}

/// The `action` call: like [`Tab`], plus the action id and the form values.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub tab: String,
    /// The `id` of the button, tree item action or form.
    pub action: String,
    pub target: TabTarget,
    pub spec: RequestSpec,
    pub response: Option<PluginResponse>,
    pub state: Option<serde_json::Value>,
    /// Form values by field name (forms only).
    pub input: Vec<(String, String)>,
}

impl Action {
    /// The value of a form field.
    pub fn input(&self, name: &str) -> Option<&str> {
        self.input
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

// -------------------------------------------------------------------------------------------
// Small builders for replies.

/// An info note.
pub fn info(text: impl Into<String>) -> PluginNote {
    note(NoteLevel::Info, text)
}

/// A warning note.
pub fn warning(text: impl Into<String>) -> PluginNote {
    note(NoteLevel::Warning, text)
}

pub fn note(level: NoteLevel, text: impl Into<String>) -> PluginNote {
    PluginNote {
        level,
        text: text.into(),
    }
}

/// A patch that sets (replaces or adds) one header.
pub fn set_header(name: impl Into<String>, value: impl Into<String>) -> RequestPatch {
    RequestPatch {
        set_headers: vec![(name.into(), value.into())],
        ..RequestPatch::default()
    }
}
