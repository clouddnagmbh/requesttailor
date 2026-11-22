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
    Manifest, NewRequest, Permissions, PluginEnvironment, PluginFile, PluginResponse,
    PluginVariable, TabContribution, TabPlace, TabTarget, TreeItem, View, ViewReply, WelcomeChoice,
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

/// The first-start question (wave 6, strand F): bundled plugins that offer a choice.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WelcomeState {
    /// Answered or skipped already: the UI does not ask again.
    pub done: bool,
    pub choices: Vec<WelcomeOffer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeOffer {
    /// Plugin id.
    pub plugin: String,
    pub choice: WelcomeChoice,
}

/// What a shell remembers about the first-start question and the bundled plugins (desktop:
/// `welcome.json` in the data folder; the web shell keeps the same shape in browser storage).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WelcomeRecord {
    /// The question was answered (or skipped).
    #[serde(default)]
    pub done: bool,
    /// Bundled plugins installed so far, with the version installed. A bundled plugin the
    /// user removed stays removed: it is installed again only if it is not listed here.
    #[serde(default)]
    pub bundled: std::collections::BTreeMap<String, String>,
}

/// What to do with a bundled plugin at start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundledStep {
    /// Install it, disabled (it is enabled only when chosen at the first start).
    Install,
    /// Replace the installed module; the installation keeps its enabled state.
    Update,
    Keep,
}

/// `installed`: version of the plugin with that id in the plugin folder, if any.
pub fn bundled_step(
    record: &WelcomeRecord,
    installed: Option<&str>,
    shipped: &Manifest,
) -> BundledStep {
    match installed {
        None if record.bundled.contains_key(&shipped.id) => BundledStep::Keep,
        None => BundledStep::Install,
        Some(v) if newer(&shipped.version, v) => BundledStep::Update,
        Some(_) => BundledStep::Keep,
    }
}

/// Whether version `a` is newer than `b` (dotted numbers; a pre-release suffix such as
/// `-rc.1` counts as older than the release).
pub fn newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let (core, pre) = match v.split_once(['-', '+']) {
            Some((core, rest)) => (core, v.as_bytes()[core.len()] == b'-' && !rest.is_empty()),
            None => (v, false),
        };
        (
            core.split('.')
                .map(|p| p.trim().parse().unwrap_or(0))
                .collect(),
            pre,
        )
    }
    let ((mut x, xp), (mut y, yp)) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y || (x == y && yp && !xp)
}

/// The `context` of a plugin tab (wave 6): the environment and the non-secret variables in
/// scope for `request_path` (`""` for collection tabs: collection and environment only), with
/// the values as the files have them, as for the `prepare` hook. Secret variables are left
/// out; the host drops the rest for plugins without the `variables` permission. An unknown
/// environment adds no layer.
pub fn tab_context(
    collection: &rt_core::model::Collection,
    request_path: &str,
    environment: Option<&str>,
) -> rt_plugin::api::PluginContext {
    let empty = rt_core::model::RequestEntry {
        path: String::new(),
        spec: rt_core::model::RequestSpec::default(),
    };
    let context = collection
        .request(request_path)
        .unwrap_or(rt_core::model::RequestContext {
            collection,
            folders: Vec::new(),
            request: &empty,
        });
    let input = |environment| rt_core::prepare::PrepareInput {
        environment,
        ..Default::default()
    };
    let scope = rt_core::prepare::scope(&context, &input(environment))
        .or_else(|_| rt_core::prepare::scope(&context, &input(None)))
        .unwrap_or_default();
    let names: std::collections::BTreeSet<&str> = scope
        .layers
        .iter()
        .flat_map(|l| l.vars.iter().map(|(n, _, _)| n.as_str()))
        .collect();
    rt_plugin::api::PluginContext {
        request_path: request_path.to_owned(),
        environment: environment.map(str::to_owned),
        variables: names
            .into_iter()
            .filter_map(|name| {
                let (value, _, secret) = scope.get(name)?;
                (!secret).then(|| (name.to_owned(), value.to_owned()))
            })
            .collect(),
    }
}

/// Everything a tab call runs with besides the request (wave 6): [`tab_context`], and the
/// URLs the `{request}` network entry covers. For a request tab that is its URL resolved
/// with the selected environment as for sending (without secret values: a URL that needs one,
/// or has unresolved placeholders, covers no host). For a collection tab (`""`) it is every
/// non-secret variable of the selected environment whose resolved value is an absolute
/// http(s) URL (e.g. `baseUrl`).
pub fn tab_scope(
    collection: &rt_core::model::Collection,
    request_path: &str,
    environment: Option<&str>,
) -> rt_plugin::host::TabScope {
    rt_plugin::host::TabScope {
        context: Some(tab_context(collection, request_path, environment)),
        request_urls: tab_request_urls(collection, request_path, environment),
    }
}

fn tab_request_urls(
    collection: &rt_core::model::Collection,
    request_path: &str,
    environment: Option<&str>,
) -> Vec<String> {
    use rt_core::prepare::{FileSource, PrepareInput, prepare, scope};
    /// The URL does not depend on body files.
    struct NoFiles;
    impl FileSource for NoFiles {
        fn read(&self, _: &str) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
    }
    let absolute = |u: &str| {
        url::Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.has_host())
    };
    let input = PrepareInput {
        environment,
        ..Default::default()
    };
    if !request_path.is_empty() {
        let Some(context) = collection.request(request_path) else {
            return Vec::new();
        };
        let url = match prepare(&context, &input, &NoFiles) {
            Ok(p) if p.unresolved.is_empty() => p.request.url,
            // Auth or body errors do not change the URL.
            _ => {
                let Ok(scope) = scope(&context, &input) else {
                    return Vec::new();
                };
                let i = scope.interpolate(&context.request.spec.url);
                if !i.unresolved.is_empty() {
                    return Vec::new();
                }
                i.value
            }
        };
        return if absolute(&url) {
            vec![url]
        } else {
            Vec::new()
        };
    }
    let Some(env) = environment.and_then(|name| collection.environment(name)) else {
        return Vec::new();
    };
    let empty = rt_core::model::RequestEntry {
        path: String::new(),
        spec: rt_core::model::RequestSpec::default(),
    };
    let context = rt_core::model::RequestContext {
        collection,
        folders: Vec::new(),
        request: &empty,
    };
    let Ok(scope) = scope(&context, &input) else {
        return Vec::new();
    };
    let mut urls: Vec<String> = Vec::new();
    for v in env.variables.iter().filter(|v| v.enabled && !v.secret) {
        let Some(value) = &v.value else { continue };
        let i = scope.interpolate(value);
        let value = i.value.trim();
        if i.unresolved.is_empty()
            && i.used.iter().all(|u| !u.secret)
            && absolute(value)
            && !urls.iter().any(|u| u == value)
        {
            urls.push(value.to_owned());
        }
    }
    urls
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
    fn versions_and_bundled_steps() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("1.10", "1.9.3"));
        assert!(newer("1.0.0", "1.0.0-rc.1"));
        assert!(!newer("1.0.0-rc.1", "1.0.0"));
        assert!(!newer("1.0.0", "1.0"));
        let shipped: Manifest = serde_json::from_value(serde_json::json!({
            "id": "odata", "name": "OData", "version": "0.2.0", "api": 1
        }))
        .unwrap();
        let mut record = WelcomeRecord::default();
        assert_eq!(bundled_step(&record, None, &shipped), BundledStep::Install);
        record.bundled.insert("odata".into(), "0.1.0".into());
        assert_eq!(
            bundled_step(&record, None, &shipped),
            BundledStep::Keep,
            "removed by the user"
        );
        assert_eq!(
            bundled_step(&record, Some("0.1.0"), &shipped),
            BundledStep::Update
        );
        assert_eq!(
            bundled_step(&record, Some("0.2.0"), &shipped),
            BundledStep::Keep
        );
    }

    #[test]
    fn tab_context_has_the_non_secret_variables_in_scope() {
        let files = FileSet {
            files: [
                (
                    "opencollection.yml",
                    "opencollection: 1.0.0\n\ninfo:\n  name: C\nrequest:\n  variables:\n    - name: base\n      value: http://collection\n    - name: lang\n      value: EN\n",
                ),
                (
                    "environments/DEV.yml",
                    "name: DEV\nvariables:\n  - name: base\n    value: https://dev\n  - name: sapClient\n    value: \"100\"\n  - secret: true\n    name: password\n",
                ),
                (
                    "orders/get.yml",
                    "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/x\"\n\nruntime:\n  variables:\n    - name: lang\n      value: DE\n",
                ),
            ]
            .into_iter()
            .map(|(p, t)| (p.to_owned(), t.as_bytes().to_vec()))
            .collect(),
        };
        let doc = rt_format::read_collection(&files).unwrap();
        let c = doc.model();
        let vars = |path: &str, env: Option<&str>| tab_context(c, path, env).variables;
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            vars("", Some("DEV")),
            pairs(&[
                ("base", "https://dev"),
                ("lang", "EN"),
                ("sapClient", "100")
            ])
        );
        assert_eq!(
            vars("orders/get.yml", Some("DEV")),
            pairs(&[
                ("base", "https://dev"),
                ("lang", "DE"),
                ("sapClient", "100")
            ])
        );
        assert_eq!(
            vars("", None),
            pairs(&[("base", "http://collection"), ("lang", "EN")])
        );
        assert_eq!(
            vars("", Some("Nope")),
            pairs(&[("base", "http://collection"), ("lang", "EN")])
        );
        // What `{request}` covers in tabs.
        let urls = |path: &str, env: Option<&str>| tab_scope(c, path, env).request_urls;
        assert_eq!(urls("orders/get.yml", Some("DEV")), ["https://dev/x"]);
        assert_eq!(urls("orders/get.yml", None), ["http://collection/x"]);
        assert_eq!(urls("", Some("DEV")), ["https://dev"]);
        assert!(urls("", None).is_empty());
        assert!(urls("nope.yml", Some("DEV")).is_empty());
        let ctx = tab_context(c, "", Some("DEV"));
        assert_eq!(
            (ctx.request_path.as_str(), ctx.environment.as_deref()),
            ("", Some("DEV"))
        );
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
