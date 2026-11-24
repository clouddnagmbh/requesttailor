// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! An open collection being edited: tree, requests, environments, dirty tracking, save, and
//! the snapshot used for sending.
//!
//! Owner: wave 3, strand F.
//!
//! Files are the truth (product principle 1): every edit goes straight into the format
//! document (`rt_format::CollectionDoc`), and "dirty" means "the bytes this file would have
//! differ from the bytes on disk". An edit that restores a request or environment to its
//! saved state restores the saved bytes, so it is no longer dirty.
//!
//! Secret values never enter the document. They arrive per send in [`SendInput::secrets`]
//! and go into the environment layer of a copy of the collection ([`SendSnapshot`]), which
//! lives only as long as the send.

use rt_core::model::{
    Auth, Body, Collection, Environment, Item, PartKind, RequestContext, RequestSpec, Variable,
};
use rt_core::prepare::{FileSource, PrepareError, PrepareInput, Prepared, prepare};
use rt_core::send::{MASK, SendOptions, SendReport};
use rt_core::transport::Transport;
use rt_format::{CollectionDoc, FileSet};
use serde::{Deserialize, Serialize};

use crate::{AppError, FileChange, TreeNode};

/// What the UI passes when sending.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendInput {
    /// Environment name; `None` for no environment.
    pub environment: Option<String>,
    /// Runtime variables (highest precedence), e.g. typed into the UI.
    #[serde(default)]
    pub vars: Vec<(String, String)>,
    /// Values of secret variables of the selected environment, read by the shell from its
    /// `SecretStore`. They are injected into the environment layer of the snapshot only, so
    /// provenance stays "environment" and the values are masked in the report.
    #[serde(default)]
    pub secrets: Vec<(String, String)>,
}

/// One variable as the editor shows it: where its value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewVar {
    pub name: String,
    /// `None` for secrets (never shown resolved) and for unresolved names.
    pub value: Option<String>,
    pub source: rt_core::vars::VarSource,
    pub secret: bool,
}

/// What a request would send, without sending it: for variable highlighting with provenance
/// in the editor. Computed by `rt_core::prepare`, so it matches what `send` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    /// Resolved URL with secret values masked as `***`; `None` if preparation failed.
    pub url: Option<String>,
    /// Resolved headers, secrets masked.
    pub headers: Vec<(String, String)>,
    pub variables: Vec<PreviewVar>,
    pub unresolved: Vec<String>,
    /// Preparation error message (e.g. unsupported auth), if any.
    pub error: Option<String>,
}

/// An owned copy of everything needed to send one request, so sending (async, possibly long)
/// never borrows the `Workspace` across an `.await`.
#[derive(Debug, Clone)]
pub struct SendSnapshot {
    pub collection: Collection,
    pub request_path: String,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    /// Files referenced by bodies, from the collection's file set.
    pub files: FileSet,
}

impl SendSnapshot {
    /// Sends the request through `transport`. `options.body_limit` should be large (the
    /// response store keeps the full body for virtual rendering).
    pub async fn send<T: Transport>(&self, transport: &T, options: &SendOptions) -> SendReport {
        self.send_with(transport, options, &rt_core::services::Services::none())
            .await
    }

    /// [`SendSnapshot::send`] with the shell's services (wave 5: tokens, prompts, cookies,
    /// plugins).
    pub async fn send_with<T: Transport>(
        &self,
        transport: &T,
        options: &SendOptions,
        services: &rt_core::services::Services<'_>,
    ) -> SendReport {
        let input = PrepareInput {
            environment: self.environment.as_deref(),
            globals: Vec::new(),
            runtime: self.vars.clone(),
        };
        rt_core::send::send_with(
            &self.collection,
            &self.request_path,
            &input,
            &MemFiles(&self.files),
            transport,
            options,
            services,
        )
        .await
    }

    /// Opens the request as a session (wave 8, strand A): WebSocket, gRPC, or an HTTP
    /// request read as SSE. Prepared like [`SendSnapshot::send_with`]; `http` carries what
    /// the preparation sends itself (OAuth token requests), `streams` opens the session. See
    /// [`rt_core::stream::open`].
    pub async fn open_stream<'a, T: Transport, S: rt_core::stream::StreamTransport + ?Sized>(
        &self,
        http: &T,
        streams: &'a S,
        options: &SendOptions,
        services: &rt_core::services::Services<'_>,
    ) -> Result<rt_core::stream::Session<'a>, rt_core::send::SendError> {
        let Some(context) = self.collection.request(&self.request_path) else {
            return Err(rt_core::send::SendError::NotFound(
                self.request_path.clone(),
            ));
        };
        let input = PrepareInput {
            environment: self.environment.as_deref(),
            globals: Vec::new(),
            runtime: self.vars.clone(),
        };
        rt_core::stream::open(
            &context,
            &input,
            &MemFiles(&self.files),
            options,
            services,
            http,
            streams,
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    doc: CollectionDoc,
    /// The document as opened or last saved: what is on disk.
    saved: CollectionDoc,
    /// Bytes of the format's files as opened or last saved.
    saved_files: FileSet,
    /// The files the workspace was opened with, kept up to date on save. Bodies read their
    /// files from here (non-YAML files are not part of the document).
    files: FileSet,
    /// Paths whose bytes differ from `saved_files`, sorted.
    dirty: Vec<String>,
    /// Companion files staged for the next save (wave 8: saved MCP tests): files of the
    /// collection folder that are not part of the OpenCollection document.
    staged: std::collections::BTreeMap<String, Vec<u8>>,
    /// Wave 17: the contract's spec staged for the next save (`rt_app::contract::stage_spec`),
    /// keyed by its path as the contract names it: in the collection folder, or with a leading
    /// `/` at the repository root. Outside the document, the companion files and `files`.
    staged_spec: std::collections::BTreeMap<String, Vec<u8>>,
}

impl Workspace {
    pub fn open(files: FileSet) -> Result<Self, AppError> {
        let doc = rt_format::read_collection(&files)?;
        let saved_files = doc.write();
        Ok(Self {
            saved: doc.clone(),
            doc,
            saved_files,
            files,
            dirty: Vec::new(),
            staged: Default::default(),
            staged_spec: Default::default(),
        })
    }

    pub fn collection(&self) -> &Collection {
        self.doc.model()
    }

    /// The files of the opened folder (wave 8, strand B: proto files of gRPC requests).
    pub fn files(&self) -> &FileSet {
        &self.files
    }

    pub fn tree(&self) -> TreeNode {
        let model = self.doc.model();
        TreeNode::Folder {
            name: model.name.clone(),
            path: String::new(),
            children: tree_nodes(&model.items),
        }
    }

    pub fn request(&self, path: &str) -> Result<RequestSpec, AppError> {
        self.doc
            .model()
            .request(path)
            .map(|c| c.request.spec.clone())
            .ok_or_else(|| AppError::RequestNotFound(path.to_owned()))
    }

    /// Creates a request with method GET, empty URL and the given name in `folder`
    /// (`""` = root). Returns its path. Not on disk until `save`.
    ///
    /// New requests inherit auth (`auth: inherit`, as Bruno creates them) and go after the
    /// last item of the folder.
    pub fn create_request(&mut self, folder: &str, name: &str) -> Result<String, AppError> {
        if !folder.is_empty() && find_folder(&self.doc.model().items, folder).is_none() {
            return Err(AppError::FolderNotFound(folder.to_owned()));
        }
        let spec = RequestSpec {
            name: name.to_owned(),
            method: "GET".to_owned(),
            url: String::new(),
            auth: Auth::Inherit,
            ..RequestSpec::default()
        };
        let path = self.doc.add_request(folder, &spec)?;
        self.refresh_dirty();
        Ok(path)
    }

    /// Makes sure the folder `path` exists (wave 6: plugin views add requests in folders) and
    /// returns its actual path. Each `/`-separated part names a folder of its parent, by
    /// directory or by display name; a missing one is created (`folder.yml`). `""` is the
    /// root. Not on disk until `save`.
    pub fn create_folder(&mut self, path: &str) -> Result<String, AppError> {
        let mut at = String::new();
        if path.is_empty() {
            return Ok(at);
        }
        for part in path.split('/') {
            let bad = part.trim().is_empty()
                || part == "."
                || part == ".."
                || part.contains('\\')
                || part.chars().any(char::is_control);
            if bad {
                return Err(AppError::FolderNotFound(path.to_owned()));
            }
            let model = self.doc.model();
            let siblings = if at.is_empty() {
                &model.items
            } else {
                &find_folder(&model.items, &at).expect("found before").items
            };
            let existing = siblings.iter().find_map(|i| match i {
                Item::Folder(f) if f.path.rsplit('/').next() == Some(part) || f.name == part => {
                    Some(f.path.clone())
                }
                _ => None,
            });
            at = match existing {
                Some(p) => p,
                None => self.doc.add_folder(&at, part)?,
            };
        }
        self.refresh_dirty();
        Ok(at)
    }

    /// Replaces a request in memory. Not on disk until `save`.
    pub fn update_request(&mut self, path: &str, spec: RequestSpec) -> Result<(), AppError> {
        if self.doc.model().request(path).is_none() {
            return Err(AppError::RequestNotFound(path.to_owned()));
        }
        let saved = self.saved.model().request(path).map(|c| &c.request.spec);
        if saved.is_some_and(|s| *s == without_secret_values(&spec)) {
            self.restore(path)?;
        } else {
            self.doc.set_request(path, &spec)?;
        }
        self.refresh_dirty();
        Ok(())
    }

    /// The format document, for edits composed of several steps (an import added as a folder,
    /// [`crate::import::add_to_workspace`]). Call [`Workspace::refresh`] afterwards.
    pub(crate) fn doc_mut(&mut self) -> &mut CollectionDoc {
        &mut self.doc
    }

    /// Recomputes `dirty` after edits through [`Workspace::doc_mut`].
    pub(crate) fn refresh(&mut self) {
        self.refresh_dirty();
    }

    /// Request paths (and `environments/<name>.yml` paths) with unsaved changes.
    ///
    /// Also new and deleted files, sorted by path. An environment declared inside
    /// `opencollection.yml` shows up as that file.
    pub fn dirty(&self) -> Vec<String> {
        let mut dirty = self.dirty.clone();
        dirty.extend(
            self.staged
                .iter()
                .filter(|(path, bytes)| self.files.files.get(*path) != Some(*bytes))
                .map(|(path, _)| path.clone()),
        );
        dirty.extend(
            self.staged_spec
                .iter()
                .filter(|(path, bytes)| self.files.files.get(*path) != Some(*bytes))
                .map(|(path, _)| path.clone()),
        );
        dirty.sort();
        dirty.dedup();
        dirty
    }

    /// Whether a spec is staged at `path` (wave 17).
    pub fn staged_spec_path(&self, path: &str) -> bool {
        self.staged_spec.contains_key(path)
    }

    /// The staged spec's bytes at `path` (wave 17), if one is staged.
    pub(crate) fn staged_spec_file(&self, path: &str) -> Option<&[u8]> {
        self.staged_spec.get(path).map(Vec::as_slice)
    }

    /// Stages the contract's spec for the next save (wave 17). The caller
    /// (`rt_app::contract::stage_spec`) has checked that `path` is the contract's spec and
    /// that `bytes` parse; this is the one way to stage a `.yml` file or a path with a
    /// leading `/`.
    pub(crate) fn stage_spec_file(&mut self, path: &str, bytes: Vec<u8>) {
        self.staged_spec.insert(path.to_owned(), bytes);
    }

    /// Paths of the companion files (wave 8) for which `wanted` is true: files of the
    /// collection folder outside the OpenCollection document, with staged ones, sorted.
    pub fn companion_paths(&self, wanted: impl Fn(&str) -> bool) -> Vec<String> {
        let mut paths: Vec<String> = self
            .files
            .files
            .keys()
            .chain(self.staged.keys())
            .filter(|p| wanted(p))
            .cloned()
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }

    /// A companion file's bytes, staged ones first.
    pub fn companion_file(&self, path: &str) -> Option<&[u8]> {
        self.staged
            .get(path)
            .or_else(|| self.files.files.get(path))
            .map(Vec::as_slice)
    }

    /// Stages a companion file for the next [`save`](Self::save), like a request edit: it is
    /// `dirty` until saved (unless it has the bytes on disk). Refuses collection files
    /// (`*.yml`, which the document owns) and paths that could leave the collection folder.
    pub fn stage_companion(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), AppError> {
        let bad = path.is_empty()
            || path.ends_with(".yml")
            || path.starts_with('/')
            || path.contains(['\\', ':', '\0'])
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".." || p.starts_with('.'));
        if bad {
            return Err(AppError::Shell(format!("cannot write {path:?} here")));
        }
        self.staged.insert(path.to_owned(), bytes);
        Ok(())
    }

    /// Files that changed since open or the last save; the shell writes them. Clears `dirty`.
    ///
    /// **Deleted files** (a deleted environment) are returned with `deleted: true` and an
    /// empty `text`: the shell deletes that file.
    pub fn save(&mut self) -> Result<Vec<FileChange>, AppError> {
        let now = self.doc.write();
        let changes: Vec<FileChange> = self
            .dirty
            .iter()
            .map(|path| match now.files.get(path) {
                Some(b) => FileChange {
                    path: path.clone(),
                    text: String::from_utf8_lossy(b).into_owned(),
                    deleted: false,
                },
                None => FileChange {
                    path: path.clone(),
                    text: String::new(),
                    deleted: true,
                },
            })
            .collect();
        let mut changes = changes;
        let mut written = Vec::new();
        for (path, bytes) in std::mem::take(&mut self.staged) {
            if self.files.files.get(&path) != Some(&bytes) {
                changes.push(FileChange {
                    path: path.clone(),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    deleted: false,
                });
                written.push((path, bytes));
            }
        }
        for (path, bytes) in std::mem::take(&mut self.staged_spec) {
            if self.files.files.get(&path) != Some(&bytes) {
                changes.push(FileChange {
                    path: path.clone(),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    deleted: false,
                });
                // A spec in the collection folder is a file of the collection from now on;
                // one at the repository root is not (its path has a leading `/`).
                if !path.starts_with('/') {
                    written.push((path, bytes));
                }
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        for change in &changes {
            if let Some(i) = written.iter().position(|(p, _)| *p == change.path) {
                let (path, bytes) = written.swap_remove(i);
                self.files.files.insert(path, bytes);
                continue;
            }
            match now.files.get(&change.path) {
                Some(bytes) => {
                    self.files.files.insert(change.path.clone(), bytes.clone());
                }
                None => {
                    self.files.files.remove(&change.path);
                }
            }
        }
        self.saved_files = now;
        self.saved = self.doc.clone();
        self.dirty.clear();
        Ok(changes)
    }

    /// Environments as in the files: secret variables have `value: None`.
    pub fn environments(&self) -> Vec<Environment> {
        self.doc.model().environments.clone()
    }

    /// Resolves the request as `send` would, without sending.
    ///
    /// With a preparation error (typically a host that is still a placeholder), `url` is
    /// `None` and `error` says why; variables and unresolved names are still reported, from
    /// a preparation of the request with the failing part neutralized (URL behind a dummy
    /// host, auth off, body off). Headers are reported then only if the URL was the only
    /// problem. Unresolved names are not listed in `variables` (they have no source).
    pub fn preview(&self, request_path: &str, input: SendInput) -> Result<Preview, AppError> {
        let collection = self.collection_for_send(request_path, &input)?;
        // Preparation errors quote the substituted URL: mask secret values in them.
        let known_secrets: Vec<String> = {
            let mut v: Vec<String> = input
                .secrets
                .iter()
                .chain(
                    input
                        .vars
                        .iter()
                        .filter(|(n, _)| input.secrets.iter().any(|(s, _)| s == n)),
                )
                .map(|(_, value)| value.clone())
                .filter(|value| !value.is_empty())
                .collect();
            v.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            v.dedup();
            v
        };
        let masked = |e: &PrepareError| mask(&e.to_string(), &known_secrets);
        let files = MemFiles(&self.files);
        let mut prepare_input = PrepareInput {
            environment: input.environment.as_deref(),
            globals: Vec::new(),
            runtime: input.vars.clone(),
        };
        let context = collection.request(request_path).expect("checked");
        let error = match prepare(&context, &prepare_input, &files) {
            Ok(prepared) => return Ok(preview_of(&context, &prepare_input, &prepared, true, None)),
            Err(e) => e,
        };

        // Neutralize what failed, one problem at a time, to still show the variables.
        let mut probe = collection.clone();
        let mut only_url = true;
        let mut last = error.clone();
        for _ in 0..4 {
            let spec = &mut find_request_mut(&mut probe.items, request_path)
                .expect("checked")
                .spec;
            match &last {
                PrepareError::UnknownEnvironment(_) => {
                    prepare_input.environment = None;
                    only_url = false;
                }
                PrepareError::UnsupportedAuth(_) => {
                    spec.auth = Auth::None;
                    only_url = false;
                }
                PrepareError::BodyFile { .. } => {
                    spec.body = Body::None;
                    only_url = false;
                }
                PrepareError::InvalidUrl(_) => {
                    if spec.url.starts_with(PROBE_ORIGIN) {
                        break;
                    }
                    spec.url = format!("{PROBE_ORIGIN}{}", spec.url);
                }
            }
            let context = probe.request(request_path).expect("checked");
            match prepare(&context, &prepare_input, &files) {
                Ok(prepared) => {
                    return Ok(preview_of(
                        &context,
                        &prepare_input,
                        &prepared,
                        false,
                        Some((masked(&error), only_url)),
                    ));
                }
                Err(e) => last = e,
            }
        }
        Ok(Preview {
            url: None,
            headers: Vec::new(),
            variables: Vec::new(),
            unresolved: Vec::new(),
            error: Some(masked(&error)),
        })
    }

    /// Creates or replaces an environment (by `env.name`). Secret values are never written.
    pub fn update_environment(&mut self, env: Environment) -> Result<(), AppError> {
        let path = self.doc.set_environment(&env)?;
        let saved = self.saved.model().environment(&env.name);
        if saved.is_some_and(|s| *s == env_without_secret_values(&env))
            && self.saved_files.files.contains_key(&path)
        {
            self.restore(&path)?;
        }
        self.refresh_dirty();
        Ok(())
    }

    /// Every variable `path` (a request, a folder, `""` for the collection) sees with
    /// `input`, for variable completion ([`crate::variables::scope`]).
    pub fn variables(
        &self,
        path: &str,
        input: &SendInput,
    ) -> Result<crate::variables::VariableScope, AppError> {
        crate::variables::scope(self.doc.model(), path, input)
    }

    /// Adds or replaces a plain variable of the collection (`folder` is `""`) or a folder
    /// ([`crate::variables::define`]). Not on disk until `save`.
    pub fn define_variable(&mut self, folder: &str, variable: &Variable) -> Result<(), AppError> {
        let vars = crate::variables::define(self.doc.model(), folder, variable)?;
        self.doc.set_folder_vars(folder, &vars)?;
        self.refresh_dirty();
        Ok(())
    }

    /// The scripts of the collection (`folder` is `""`) or of a folder, in file order
    /// (wave 16, `Backend.folderScripts`).
    pub fn folder_scripts(&self, folder: &str) -> Result<Vec<rt_core::model::Script>, AppError> {
        let model = self.doc.model();
        if folder.is_empty() {
            return Ok(model.defaults.scripts.clone());
        }
        find_folder(&model.items, folder)
            .map(|f| f.defaults.scripts.clone())
            .ok_or_else(|| AppError::FolderNotFound(folder.to_owned()))
    }

    /// Replaces the scripts of the collection (`folder` is `""`) or of a folder. The file is
    /// read back and must give `scripts` (`CollectionDoc::set_folder_scripts`). Not on disk
    /// until `save`.
    pub fn set_folder_scripts(
        &mut self,
        folder: &str,
        scripts: &[rt_core::model::Script],
    ) -> Result<(), AppError> {
        self.folder_scripts(folder)?;
        self.doc.set_folder_scripts(folder, scripts)?;
        self.refresh_dirty();
        Ok(())
    }

    pub fn delete_environment(&mut self, name: &str) -> Result<(), AppError> {
        if self.doc.model().environment(name).is_none() {
            return Err(AppError::EnvironmentNotFound(name.to_owned()));
        }
        self.doc.remove_environment(name)?;
        self.refresh_dirty();
        Ok(())
    }

    /// Sets what coding agents may do (wave 8, strand C: `rt_app::agents`), in memory like
    /// every edit; `opencollection.yml` is dirty until `save`.
    pub fn set_agent_policy(
        &mut self,
        policy: &rt_core::model::AgentPolicy,
    ) -> Result<(), AppError> {
        self.doc.set_agent_policy(policy)?;
        self.refresh_dirty();
        Ok(())
    }

    pub fn snapshot(&self, request_path: &str, input: SendInput) -> Result<SendSnapshot, AppError> {
        let collection = self.collection_for_send(request_path, &input)?;
        let spec = &collection
            .request(request_path)
            .expect("checked")
            .request
            .spec;
        let mut files = FileSet::default();
        // Client certificates (wave 5) are read like body files: only collection-relative
        // paths, from the collection's file set.
        let environment = input
            .environment
            .as_deref()
            .and_then(|name| collection.environment(name));
        let certificates = collection
            .config
            .client_certificates
            .iter()
            .chain(environment.into_iter().flat_map(|e| &e.client_certificates))
            .flat_map(|c| match &c.source {
                rt_core::model::CertificateSource::Pem {
                    certificate_path,
                    private_key_path,
                } => vec![certificate_path.as_str(), private_key_path.as_str()],
                rt_core::model::CertificateSource::Pkcs12 { path } => vec![path.as_str()],
            });
        for path in body_files(&spec.body).into_iter().chain(certificates) {
            if let Some(key) = file_key(path)
                && let Some(bytes) = self.files.files.get(&key)
            {
                files.files.insert(key, bytes.clone());
            }
        }
        Ok(SendSnapshot {
            collection,
            request_path: request_path.to_owned(),
            environment: input.environment,
            vars: input.vars,
            files,
        })
    }

    /// A copy of the collection with the secret values of `input` in the selected
    /// environment. Fails if the request does not exist.
    fn collection_for_send(
        &self,
        request_path: &str,
        input: &SendInput,
    ) -> Result<Collection, AppError> {
        if self.doc.model().request(request_path).is_none() {
            return Err(AppError::RequestNotFound(request_path.to_owned()));
        }
        Ok(self.with_secret_values(input))
    }

    /// Everything a collection run needs (wave 7, strand D, [`crate::runner`]): the
    /// collection with the secret values of `input`, and the files the requests' bodies and
    /// the client certificates read.
    pub fn run_snapshot(&self, input: SendInput) -> crate::runner::RunSnapshot {
        fn bodies<'a>(items: &'a [Item], out: &mut Vec<&'a str>) {
            for item in items {
                match item {
                    Item::Folder(f) => bodies(&f.items, out),
                    Item::Request(r) => out.extend(body_files(&r.spec.body)),
                }
            }
        }
        let collection = self.with_secret_values(&input);
        let mut paths = Vec::new();
        bodies(&collection.items, &mut paths);
        let certificates = collection.config.client_certificates.iter().chain(
            collection
                .environments
                .iter()
                .flat_map(|e| &e.client_certificates),
        );
        for c in certificates {
            match &c.source {
                rt_core::model::CertificateSource::Pem {
                    certificate_path,
                    private_key_path,
                } => paths.extend([certificate_path.as_str(), private_key_path.as_str()]),
                rt_core::model::CertificateSource::Pkcs12 { path } => paths.push(path.as_str()),
            }
        }
        let mut files = FileSet::default();
        for path in paths {
            if let Some(key) = file_key(path)
                && let Some(bytes) = self.files.files.get(&key)
            {
                files.files.insert(key, bytes.clone());
            }
        }
        crate::runner::RunSnapshot {
            environment: input.environment,
            vars: input.vars,
            files,
            collection,
        }
    }

    /// A copy of the collection with the secret values of `input` in the selected
    /// environment.
    fn with_secret_values(&self, input: &SendInput) -> Collection {
        let mut collection = self.doc.model().clone();
        if let Some(name) = &input.environment
            && let Some(env) = collection.environments.iter_mut().find(|e| e.name == *name)
        {
            for (var, value) in &input.secrets {
                match env.variables.iter_mut().find(|v| v.name == *var) {
                    Some(v) => {
                        v.value = Some(value.clone());
                        v.secret = true;
                    }
                    None => env.variables.push(Variable {
                        name: var.clone(),
                        value: Some(value.clone()),
                        enabled: true,
                        secret: true,
                    }),
                }
            }
        }
        collection
    }

    /// Puts the saved bytes of `path` back into the document.
    fn restore(&mut self, path: &str) -> Result<(), AppError> {
        let mut files = self.doc.write();
        match self.saved_files.files.get(path) {
            Some(bytes) => files.files.insert(path.to_owned(), bytes.clone()),
            None => files.files.remove(path),
        };
        self.doc = rt_format::read_collection(&files)?;
        Ok(())
    }

    fn refresh_dirty(&mut self) {
        let now = self.doc.write();
        let mut dirty: Vec<String> = now
            .files
            .iter()
            .filter(|(path, bytes)| self.saved_files.files.get(*path) != Some(*bytes))
            .map(|(path, _)| path.clone())
            .collect();
        dirty.extend(
            self.saved_files
                .files
                .keys()
                .filter(|path| !now.files.contains_key(*path))
                .cloned(),
        );
        dirty.sort();
        self.dirty = dirty;
    }
}

fn tree_nodes(items: &[Item]) -> Vec<TreeNode> {
    items
        .iter()
        .map(|item| match item {
            Item::Folder(f) => TreeNode::Folder {
                name: f.name.clone(),
                path: f.path.clone(),
                children: tree_nodes(&f.items),
            },
            Item::Request(r) => TreeNode::Request {
                name: r.spec.name.clone(),
                path: r.path.clone(),
                method: r.spec.method.clone(),
            },
        })
        .collect()
}

fn find_folder<'a>(items: &'a [Item], path: &str) -> Option<&'a rt_core::model::Folder> {
    items.iter().find_map(|i| match i {
        Item::Folder(f) if f.path == path => Some(f),
        Item::Folder(f) => find_folder(&f.items, path),
        Item::Request(_) => None,
    })
}

fn find_request_mut<'a>(
    items: &'a mut [Item],
    path: &str,
) -> Option<&'a mut rt_core::model::RequestEntry> {
    items.iter_mut().find_map(|i| match i {
        Item::Request(r) if r.path == path => Some(r),
        Item::Request(_) => None,
        Item::Folder(f) => find_request_mut(&mut f.items, path),
    })
}

/// The request as the files hold it: secret variables without a value.
fn without_secret_values(spec: &RequestSpec) -> RequestSpec {
    let mut spec = spec.clone();
    for v in spec.vars.iter_mut().filter(|v| v.secret) {
        v.value = None;
    }
    spec
}

fn env_without_secret_values(env: &Environment) -> Environment {
    let mut env = env.clone();
    for v in env.variables.iter_mut().filter(|v| v.secret) {
        v.value = None;
    }
    env
}

/// Paths of the files a body reads.
fn body_files(body: &Body) -> Vec<&str> {
    match body {
        Body::File { path, .. } => vec![path.as_str()],
        Body::Multipart { parts } => parts
            .iter()
            .filter(|p| p.kind == PartKind::File)
            .flat_map(|p| p.values.iter().map(String::as_str))
            .collect(),
        _ => Vec::new(),
    }
}

/// The file-set key of a body file path: relative, `/`-separated, no `..`. `None` for
/// paths outside the collection.
fn file_key(path: &str) -> Option<String> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// Body files from an in-memory file set (same rules as the CLI and `rt-wasm`).
pub(crate) struct MemFiles<'a>(pub(crate) &'a FileSet);

impl FileSource for MemFiles<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let key = file_key(path).ok_or("path leaves the collection folder")?;
        self.0
            .files
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("no file {key}"))
    }
}

/// Origin put before a URL that failed to prepare, to still resolve its variables.
const PROBE_ORIGIN: &str = "http://preview.invalid/";

/// Builds the preview from a successful preparation. `failed` carries the original error and
/// whether the headers are still meaningful.
fn preview_of(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    prepared: &Prepared,
    url_ok: bool,
    failed: Option<(String, bool)>,
) -> Preview {
    let secrets = secret_values(context, input, prepared);
    let request = &prepared.request;
    let headers_ok = failed.as_ref().is_none_or(|(_, only_url)| *only_url);
    let mut headers: Vec<(String, String)> = if headers_ok {
        request
            .headers
            .iter()
            .map(|(n, v)| (n.clone(), mask_header(v, &secrets)))
            .collect()
    } else {
        Vec::new()
    };
    // As in the send report: the body's content type is listed when no header sets one.
    if headers_ok
        && let Some(body) = &request.body
        && let Some(content_type) = &body.content_type
        && !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".to_owned(), content_type.clone()));
    }
    let variables = prepared
        .used
        .iter()
        .map(|u| PreviewVar {
            name: u.name.clone(),
            value: (!u.secret).then(|| {
                let value = prepared
                    .scope
                    .interpolate(&format!("{{{{{}}}}}", u.name))
                    .value;
                mask(&value, &secrets)
            }),
            source: u.source.clone(),
            secret: u.secret,
        })
        .collect();
    Preview {
        url: url_ok.then(|| mask(&request.url, &secrets)),
        headers,
        variables,
        unresolved: prepared.unresolved.clone(),
        error: failed.map(|(e, _)| e),
    }
}

/// Values that must not be shown: values of variables that are secret in any layer or
/// declared secret anywhere for this request. Longest first, so a secret containing another
/// is masked whole. Same rules as the send report.
fn secret_values(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    prepared: &Prepared,
) -> Vec<String> {
    let environment = input
        .environment
        .and_then(|name| context.collection.environment(name));
    let declared: Vec<&str> = context
        .collection
        .defaults
        .vars
        .iter()
        .chain(environment.into_iter().flat_map(|e| e.variables.iter()))
        .chain(context.folders.iter().flat_map(|f| f.defaults.vars.iter()))
        .chain(context.request.spec.vars.iter())
        .filter(|v| v.secret)
        .map(|v| v.name.as_str())
        .collect();
    let mut values: Vec<String> = prepared
        .scope
        .layers
        .iter()
        .flat_map(|l| l.vars.iter())
        .filter(|(name, _, secret)| *secret || declared.contains(&name.as_str()))
        .map(|(_, value, _)| value.clone())
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

/// Replaces every secret, raw, percent-encoded or Debug-escaped, with [`MASK`].
pub(crate) fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        for form in [
            secret.clone(),
            secret.escape_debug().to_string(),
            percent_encode(secret, false),
            percent_encode(secret, true),
        ] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, MASK);
            }
        }
    }
    out
}

/// Like [`mask`]; also hides HTTP Basic credentials that contain a secret.
fn mask_header(value: &str, secrets: &[String]) -> String {
    if let Some((scheme, credentials)) = value.split_once(' ')
        && scheme.eq_ignore_ascii_case("basic")
        && let Some(decoded) = base64_decode(credentials.trim())
    {
        let decoded = String::from_utf8_lossy(&decoded);
        if secrets.iter().any(|s| decoded.contains(s.as_str())) {
            return format!("{scheme} {MASK}");
        }
    }
    mask(value, secrets)
}

fn percent_encode(s: &str, plus: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' if plus => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Standard base64 with optional padding; `None` if not valid.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_raw_encoded_and_basic_secrets() {
        let secrets = vec!["a b".to_string()];
        assert_eq!(mask("x=a%20b&y=a+b", &secrets), "x=***&y=***");
        // "u:a b" in base64.
        assert_eq!(mask_header("Basic dTphIGI=", &secrets), "Basic ***");
        assert_eq!(mask_header("Basic dTp4", &secrets), "Basic dTp4");
        assert_eq!(base64_decode("dTphIGI="), Some(b"u:a b".to_vec()));
    }

    #[test]
    fn body_file_keys_stay_inside_the_collection() {
        assert_eq!(file_key("./a/b.bin").as_deref(), Some("a/b.bin"));
        assert_eq!(file_key("a\\b.bin").as_deref(), Some("a/b.bin"));
        assert_eq!(file_key("../x"), None);
        assert_eq!(file_key("/etc/passwd"), None);
        assert_eq!(file_key("C:/x"), None);
    }
}
