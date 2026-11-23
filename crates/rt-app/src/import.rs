// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import in the app: convert a file (`rt_format::import`), show the loss report, then write
//! the new collection into a folder the user picks.
//!
//! Two steps, so the user sees what would be lost before anything is written:
//! 1. [`Imports::analyze`] converts and keeps the result under an id; the UI shows the
//!    [`ImportPlan`] (counts and loss report).
//! 2. [`Imports::take`] hands the converted files to the shell, which writes them into a new,
//!    empty folder and opens it as the collection (`ImportApi.write`).
//!
//! Since wave 13 an import can also go into the open collection as a new folder
//! ([`add_to_workspace`], [`add_to_files`]), and a collection converted elsewhere (an
//! extension's importer) is checked and kept like any other ([`Imports::adopt`]). An OpenAPI
//! import that kept its spec (`crate::openapi`) brings the spec along and names it as the open
//! collection's contract spec if it has none.
//!
//! Owner: wave 4, strand B. Changes to the public types are interface changes.

use std::collections::BTreeMap;

use rt_core::model::{Auth, Body, Item, PartKind, RequestDefaults, RequestSpec};
use rt_format::FileSet;
pub use rt_format::import::SourceFile;
use rt_format::import::{ImportFormat, Loss, LossEffect, LossKind, LossReport};
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::workspace::Workspace;

/// Result of the first step, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlan {
    /// Pass to `write` or `discard`.
    pub id: u64,
    pub format: ImportFormat,
    pub collection_name: String,
    /// Suggested name of the new folder (file-system safe).
    pub folder_name: String,
    /// Number of files the collection will have.
    pub files: u32,
    pub report: LossReport,
}

/// Converted collections waiting to be written, by id.
#[derive(Debug, Default)]
pub struct Imports {
    next: u64,
    /// Plan id → (suggested folder name, files).
    pending: BTreeMap<u64, (String, FileSet)>,
}

impl Imports {
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts `files` as one collection (`rt_format::import::import_files`: a collection
    /// with its environment files, several `.http` or cURL files) as `format`, or as the
    /// detected format.
    pub fn analyze(
        &mut self,
        files: &[SourceFile<'_>],
        format: Option<ImportFormat>,
    ) -> Result<ImportPlan, AppError> {
        let import = rt_format::import::import_files(format, files)
            .map_err(|e| AppError::Import(e.to_string()))?;
        Ok(self.add(import))
    }

    /// Adds a collection converted elsewhere, by a plugin's importer (wave 5): the shell runs
    /// `PluginHost::import`, checks the files with `rt_format::read_collection`, and hands the
    /// result here so it is written like any other import.
    pub fn add(&mut self, import: rt_format::import::Import) -> ImportPlan {
        self.next += 1;
        let id = self.next;
        let plan = ImportPlan {
            id,
            format: import.report.format,
            folder_name: folder_name(&import.collection_name),
            collection_name: import.collection_name,
            files: import.files.files.len() as u32,
            report: import.report,
        };
        self.pending
            .insert(id, (plan.folder_name.clone(), import.files));
        plan
    }

    /// The suggested folder name and the files of plan `id`; the plan stays, so a cancelled
    /// or failed write can be tried again. Call [`Imports::discard`] once written.
    pub fn get(&self, id: u64) -> Result<(&str, &FileSet), AppError> {
        self.pending
            .get(&id)
            .map(|(name, files)| (name.as_str(), files))
            .ok_or_else(|| not_found(id))
    }

    /// The files of plan `id`, for writing; the plan is gone afterwards.
    pub fn take(&mut self, id: u64) -> Result<FileSet, AppError> {
        self.pending
            .remove(&id)
            .map(|(_, files)| files)
            .ok_or_else(|| not_found(id))
    }

    pub fn discard(&mut self, id: u64) {
        self.pending.remove(&id);
    }
}

fn not_found(id: u64) -> AppError {
    AppError::Import(format!("import {id} not found"))
}

/// A folder name for `collection_name` that is safe on every file system.
pub fn folder_name(collection_name: &str) -> String {
    let cleaned: String = collection_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "Imported collection".to_owned()
    } else {
        trimmed.chars().take(80).collect()
    }
}

// ---- into an open collection (since wave 13) ----

/// A collection converted elsewhere, as the UI hands it over (an extension's importer):
/// its files as text and its loss report. [`Imports::adopt`] checks it like a plugin import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalImport {
    pub collection_name: String,
    pub files: Vec<ImportedFile>,
    pub report: LossReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedFile {
    /// Relative to the collection root, `/`-separated.
    pub path: String,
    pub text: String,
}

/// What adding an import to the open collection did ([`add_to_workspace`]). Nothing is on
/// disk until the user saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderImport {
    /// Path of the new folder (for `createRequest`, the tree).
    pub folder: String,
    /// Its display name: the import's name, with ` 2`, ` 3`, ... if a top-level folder of the
    /// collection already has it.
    pub name: String,
    pub requests: u32,
    /// Folders below the new one.
    pub folders: u32,
    /// Environments the collection did not have.
    pub environments_added: Vec<String>,
    /// Environments of the same name that got the variables they lacked (existing values stay).
    pub environments_extended: Vec<String>,
    /// Other files of the import (bodies, recordings, snapshots), staged below the folder.
    pub files: Vec<String>,
    /// What the folder could not take over as it was, in addition to the import's report:
    /// collection-level headers, auth and variables are carried by the requests instead.
    pub losses: Vec<Loss>,
    /// The spec kept with an OpenAPI import, now named in the open collection's
    /// `extensions.requesttailor.contract.spec` (e.g. `Petstore/openapi.yaml`); `None` if the
    /// import kept none, or the collection already names a spec (it stays).
    #[serde(default)]
    pub contract_spec: Option<String>,
}

impl Imports {
    /// Adds an [`ExternalImport`] after checking it: safe paths, a readable collection.
    pub fn adopt(&mut self, import: ExternalImport) -> Result<ImportPlan, AppError> {
        let mut files = FileSet::default();
        for f in import.files {
            if !safe_path(&f.path) {
                return Err(AppError::Import(format!("invalid path {:?}", f.path)));
            }
            files.files.insert(f.path, f.text.into_bytes());
        }
        rt_format::read_collection(&files).map_err(|e| AppError::Import(e.to_string()))?;
        Ok(self.add(rt_format::import::Import {
            collection_name: import.collection_name,
            files,
            report: import.report,
        }))
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.starts_with('.'))
}

/// Adds the collection `imported` to the open collection as a new top-level folder named
/// `name` (made unique), with the same rules on every surface:
///
/// - Folders and requests keep their structure and order; folder scripts stay folder scripts,
///   the imported collection's scripts become the new folder's.
/// - Collection- and folder-level headers, auth and variables of the import cannot be folder
///   defaults yet; each request gets them (own values win), reported as approximated.
/// - Environments: a new name is added; an existing one gets only the variables it lacks.
///   Secret values are never written.
/// - Every other file (bodies, companion files) is staged below the new folder; body file
///   paths of the requests follow.
/// - A spec the import kept (OpenAPI wizard, `extensions.requesttailor.contract.spec` of the
///   import) is staged like those files and named in the open collection's
///   `extensions.requesttailor.contract.spec` as `<folder>/<file>`, unless the collection
///   names a spec already ([`FolderImport::contract_spec`]).
///
/// Everything is unsaved, like any edit.
pub fn add_to_workspace(
    workspace: &mut Workspace,
    imported: &FileSet,
    name: &str,
) -> Result<FolderImport, AppError> {
    let source =
        rt_format::read_collection(imported).map_err(|e| AppError::Import(e.to_string()))?;
    let source = source.model().clone();
    let name = unique_folder_name(workspace.collection(), name);
    let mut out = FolderImport {
        folder: String::new(),
        name: name.clone(),
        requests: 0,
        folders: 0,
        environments_added: Vec::new(),
        environments_extended: Vec::new(),
        files: Vec::new(),
        losses: Vec::new(),
        contract_spec: None,
    };
    let mut work = workspace.clone();
    let doc = work.doc_mut();
    let top = doc.add_folder("", &name)?;
    if !source.defaults.scripts.is_empty() {
        doc.set_folder_scripts(&top, &source.defaults.scripts)?;
    }
    report_carried(&source.defaults, "collection", &mut out.losses);
    if source.defaults.settings != Default::default() {
        out.losses.push(loss(
            "collection",
            LossKind::Setting,
            LossEffect::Dropped,
            "collection request settings (timeouts, redirects) not taken over",
        ));
    }
    if source.config != Default::default() {
        out.losses.push(loss(
            "collection",
            LossKind::Setting,
            LossEffect::Dropped,
            "collection config (certificates, protobuf, agents) not taken over",
        ));
    }
    let chain = vec![&source.defaults];
    add_items(doc, &top, &source.items, &chain, &top, &mut out)?;
    out.folder = top.clone();

    for env in &source.environments {
        match work.collection().environment(&env.name).cloned() {
            None => {
                work.update_environment(env.clone())?;
                out.environments_added.push(env.name.clone());
            }
            Some(mut existing) => {
                let missing: Vec<_> = env
                    .variables
                    .iter()
                    .filter(|v| !existing.variables.iter().any(|e| e.name == v.name))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    existing.variables.extend(missing);
                    work.update_environment(existing)?;
                    out.environments_extended.push(env.name.clone());
                }
            }
        }
    }

    for (path, bytes) in &imported.files {
        if owned_by_format(path, imported) {
            continue;
        }
        let target = format!("{top}/{path}");
        work.stage_companion(&target, bytes.clone())?;
        out.files.push(target);
    }
    // The spec an OpenAPI import kept becomes the collection's, unless it names one already.
    if let Some(spec) = rt_format::contract::configured_spec(imported)
        && !spec.starts_with('/')
        && imported.files.contains_key(&spec)
        && !owned_by_format(&spec, imported)
        && rt_format::contract::configured_spec(&work.doc_mut().write()).is_none()
    {
        let target = format!("{top}/{spec}");
        work.doc_mut().set_contract_spec(&target)?;
        out.contract_spec = Some(target);
    }
    work.refresh();
    *workspace = work;
    Ok(out)
}

/// [`add_to_workspace`] on files instead of an open workspace, for the CLI and tools that
/// write a commit: `base` with the folder added (unchanged files byte-identical).
pub fn add_to_files(
    base: &FileSet,
    imported: &FileSet,
    name: &str,
) -> Result<(FileSet, FolderImport), AppError> {
    let mut workspace = Workspace::open(base.clone())?;
    let result = add_to_workspace(&mut workspace, imported, name)?;
    let mut files = base.clone();
    for change in workspace.save()? {
        if change.deleted {
            files.files.remove(&change.path);
        } else {
            files.files.insert(change.path, change.text.into_bytes());
        }
    }
    Ok((files, result))
}

fn owned_by_format(path: &str, files: &FileSet) -> bool {
    // The same rule as `read_collection`: `.yml` files outside nested collections.
    path.ends_with(".yml") && {
        let mut dir = path.rsplit_once('/').map_or("", |(d, _)| d);
        let mut nested = false;
        while !dir.is_empty() {
            if files
                .files
                .contains_key(&format!("{dir}/{}", rt_format::COLLECTION_FILE))
            {
                nested = true;
                break;
            }
            dir = dir.rsplit_once('/').map_or("", |(d, _)| d);
        }
        !nested
    }
}

fn unique_folder_name(collection: &rt_core::model::Collection, wanted: &str) -> String {
    let wanted = folder_name(wanted);
    let taken = |n: &str| {
        collection.items.iter().any(|i| match i {
            Item::Folder(f) => {
                f.name.eq_ignore_ascii_case(n)
                    || f.path
                        .rsplit('/')
                        .next()
                        .is_some_and(|d| d.eq_ignore_ascii_case(n))
            }
            Item::Request(_) => false,
        })
    };
    if !taken(&wanted) {
        return wanted;
    }
    (2..)
        .map(|i| format!("{wanted} {i}"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

fn loss(location: &str, kind: LossKind, effect: LossEffect, detail: impl Into<String>) -> Loss {
    Loss {
        location: location.to_owned(),
        kind,
        effect,
        detail: detail.into(),
    }
}

/// Notes the defaults of `location` that the requests carry instead.
fn report_carried(defaults: &RequestDefaults, location: &str, losses: &mut Vec<Loss>) {
    if !defaults.headers.is_empty() {
        losses.push(loss(
            location,
            LossKind::Other,
            LossEffect::Approximated,
            format!(
                "{} header(s) copied into each request below",
                defaults.headers.len()
            ),
        ));
    }
    if !matches!(defaults.auth, Auth::Inherit) {
        losses.push(loss(
            location,
            LossKind::Auth,
            LossEffect::Approximated,
            "auth set on each request below that inherited it",
        ));
    }
    if !defaults.vars.is_empty() {
        losses.push(loss(
            location,
            LossKind::Variable,
            LossEffect::Approximated,
            format!(
                "{} variable(s) became request variables of each request below (an environment no longer overrides them)",
                defaults.vars.len()
            ),
        ));
    }
}

fn add_items(
    doc: &mut rt_format::CollectionDoc,
    parent: &str,
    items: &[Item],
    chain: &[&RequestDefaults],
    top: &str,
    out: &mut FolderImport,
) -> Result<(), AppError> {
    for item in items {
        match item {
            Item::Folder(f) => {
                let path = doc.add_folder(parent, &f.name)?;
                out.folders += 1;
                if !f.defaults.scripts.is_empty() {
                    doc.set_folder_scripts(&path, &f.defaults.scripts)?;
                }
                report_carried(&f.defaults, &f.name, &mut out.losses);
                let mut inner = chain.to_vec();
                inner.push(&f.defaults);
                add_items(doc, &path, &f.items, &inner, top, out)?;
            }
            Item::Request(r) => {
                let spec = carried(&r.spec, chain, top);
                doc.add_request(parent, &spec)?;
                out.requests += 1;
            }
        }
    }
    Ok(())
}

/// `spec` with the headers, auth and variables of `chain` (outermost first) it inherited,
/// and body file paths below `top`.
fn carried(spec: &RequestSpec, chain: &[&RequestDefaults], top: &str) -> RequestSpec {
    let mut spec = spec.clone();
    let mut headers: Vec<_> = Vec::new();
    let mut vars: Vec<_> = Vec::new();
    for d in chain {
        for h in &d.headers {
            headers.retain(|x: &rt_core::model::KeyValue| !x.name.eq_ignore_ascii_case(&h.name));
            headers.push(h.clone());
        }
        for v in &d.vars {
            vars.retain(|x: &rt_core::model::Variable| x.name != v.name);
            vars.push(v.clone());
        }
    }
    headers.retain(|h| {
        !spec
            .headers
            .iter()
            .any(|x| x.name.eq_ignore_ascii_case(&h.name))
    });
    headers.append(&mut spec.headers);
    spec.headers = headers;
    vars.retain(|v| !spec.vars.iter().any(|x| x.name == v.name));
    vars.append(&mut spec.vars);
    spec.vars = vars;
    if matches!(spec.auth, Auth::Inherit)
        && let Some(auth) = chain
            .iter()
            .rev()
            .map(|d| &d.auth)
            .find(|a| !matches!(a, Auth::Inherit))
    {
        spec.auth = auth.clone();
    }
    let moved = |p: &str| format!("{top}/{p}");
    match &mut spec.body {
        Body::File { path, .. } => *path = moved(path),
        Body::Multipart { parts } => {
            for part in parts.iter_mut().filter(|p| p.kind == PartKind::File) {
                for v in &mut part.values {
                    *v = moved(v);
                }
            }
        }
        _ => {}
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(folder_name("My API"), "My API");
        assert_eq!(folder_name("a/b:c"), "a-b-c");
        assert_eq!(folder_name("  ..  "), "Imported collection");
    }

    fn file<'a>(name: &'a str, data: &'a [u8]) -> SourceFile<'a> {
        SourceFile { name, data }
    }

    #[test]
    fn unknown_formats_are_an_import_error() {
        let mut imports = Imports::new();
        let e = imports
            .analyze(&[file("x.bin", b"\x00")], None)
            .unwrap_err();
        assert!(matches!(e, AppError::Import(_)), "{e:?}");
        assert!(imports.take(1).is_err());
    }

    const COLLECTION: &str = r#"{"info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": [{"name": "List", "request": {"method": "GET", "url": "{{base}}/items"}}]}"#;
    const ENVIRONMENT: &str = r#"{"name": "Prod", "_postman_variable_scope": "environment",
        "values": [{"key": "base", "value": "https://shop.example.com"}, {"key": "token", "value": "not-real", "type": "secret"}]}"#;

    #[test]
    fn analyze_then_take_hands_over_the_files_once() {
        let mut imports = Imports::new();
        let plan = imports
            .analyze(
                &[
                    file("shop.postman_collection.json", COLLECTION.as_bytes()),
                    file("prod.postman_environment.json", ENVIRONMENT.as_bytes()),
                ],
                None,
            )
            .unwrap();
        assert_eq!(plan.format, ImportFormat::Postman);
        assert_eq!(plan.collection_name, "Shop");
        assert_eq!(plan.folder_name, "Shop");
        assert_eq!((plan.report.requests, plan.report.environments), (1, 1));
        // The secret's value is reported, never written.
        assert!(
            plan.report
                .losses
                .iter()
                .any(|l| l.detail.contains("secret value of `token`"))
        );
        let (folder, kept) = imports.get(plan.id).unwrap();
        assert_eq!(folder, "Shop");
        assert_eq!(kept.files.len() as u32, plan.files);
        let files = imports.take(plan.id).unwrap();
        assert_eq!(files.files.len() as u32, plan.files);
        assert!(
            files.files.contains_key("environments/Prod.yml"),
            "{:?}",
            files.files.keys()
        );
        let all: Vec<u8> = files.files.values().flatten().copied().collect();
        assert!(!String::from_utf8_lossy(&all).contains("not-real"));
        assert!(imports.take(plan.id).is_err(), "taken twice");
    }

    #[test]
    fn discarded_plans_are_gone_and_ids_are_not_reused() {
        let mut imports = Imports::new();
        let a = imports
            .analyze(&[file("a.sh", b"curl https://a.example.com")], None)
            .unwrap();
        imports.discard(a.id);
        assert!(imports.take(a.id).is_err());
        let b = imports
            .analyze(&[file("b.sh", b"curl https://b.example.com")], None)
            .unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(b.format, ImportFormat::Curl);
    }
}
