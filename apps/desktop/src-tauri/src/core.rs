// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The desktop app's state and one method per `Backend` command, independent of Tauri so it
//! can be tested without a window. The Tauri commands (`commands.rs`) only forward to here.
//!
//! - The open collection (`rt_app::Workspace`) and the response bodies
//!   (`rt_app::ResponseStore`) live behind mutexes; no lock is held across an `.await`.
//! - Requests go out through `rt_core`'s native transport (one instance, connections reused).
//! - Secret values come from a [`SecretStore`] (the OS keychain in the app) and are injected
//!   into `SendInput::secrets` for the selected environment only, at send and preview time.
//! - Wave 4 parts live in their own modules and hang off `Core` as fields: `history` (strand
//!   E, recorded on every send), `recent` (strand C, told on every open), `imports` (strand
//!   B). Git (strand A1) works on [`Core::root`] and calls [`Core::reload`] afterwards.
//!   `data_dir` is the app's data folder (`None` in tests: nothing is persisted).
//! - External secrets (wave 5, strand E): before a send, the selected environment's
//!   `external_secrets` are read with the managers' CLIs (`external_secrets`, cached in memory
//!   for the session) and join the keychain values; a keychain value of the same name wins.
//!   Preview uses only what is cached, so it never runs a CLI.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rt_app::{
    AppError, FileChange, Imports, LineSlice, Preview, ResponseStore, SecretKey, SecretStore,
    SendInput, StoredResponse, TreeNode, Workspace,
};
use rt_core::model::{Environment, RequestSpec};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use serde::Serialize;

use crate::files;
use crate::history::DesktopHistory;
use crate::recent::Recent;

/// Body bytes kept per response. The response store keeps the whole body so the UI can
/// render it virtually (budget: a 100 MB response scrolls without stutter).
pub const BODY_LIMIT: usize = 256 * 1024 * 1024;

/// Mirrors `CollectionInfo` in `apps/ui/src/commands/backend.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionInfo {
    /// Canonical root path; also `SecretKey::collection`.
    pub id: String,
    pub name: String,
    pub location: String,
}

struct Session {
    id: String,
    root: PathBuf,
    workspace: Workspace,
}

pub struct Core<S> {
    session: Mutex<Option<Session>>,
    responses: Mutex<ResponseStore>,
    transport: NativeTransport,
    secrets: S,
    /// The app's data folder; `None`: persist nothing (tests).
    pub data_dir: Option<PathBuf>,
    pub history: DesktopHistory,
    pub recent: Recent,
    pub imports: Mutex<Imports>,
    /// Plugins, tokens and cookie jars for sends (wave 5, `services.rs`).
    pub services: std::sync::Arc<crate::services::ServiceParts>,
    /// Reads external secrets with the managers' CLIs; values cached per session (wave 5).
    pub external_secrets: std::sync::Arc<rt_app::secret_sources::SecretResolver>,
}

fn shell(e: impl std::fmt::Display) -> AppError {
    AppError::Shell(e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic in one command must not make every later command fail.
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl<S: SecretStore> Core<S> {
    /// A core that persists nothing (tests).
    pub fn new(secrets: S) -> Result<Self, AppError> {
        Self::with_data_dir(secrets, None)
    }

    /// A core that keeps history and recent collections in `data_dir`.
    pub fn with_data_dir(secrets: S, data_dir: Option<PathBuf>) -> Result<Self, AppError> {
        Ok(Self {
            session: Mutex::new(None),
            responses: Mutex::new(ResponseStore::new()),
            transport: NativeTransport::new().map_err(shell)?,
            history: DesktopHistory::new(data_dir.as_deref()),
            recent: Recent::new(data_dir.as_deref()),
            imports: Mutex::new(Imports::new()),
            services: std::sync::Arc::new(crate::services::ServiceParts::new(data_dir.as_deref())),
            external_secrets: std::sync::Arc::new(rt_app::secret_sources::SecretResolver::new(
                std::sync::Arc::new(rt_app::secret_sources::ProcessRunner::default()),
            )),
            secrets,
            data_dir,
        })
    }

    /// The secret store (Git credentials are kept there too).
    pub fn secrets(&self) -> &S {
        &self.secrets
    }

    /// Root folder of the open collection.
    pub fn root(&self) -> Option<PathBuf> {
        lock(&self.session).as_ref().map(|s| s.root.clone())
    }

    /// Id (`CollectionInfo::id`) of the open collection.
    pub fn collection_id(&self) -> Option<String> {
        lock(&self.session).as_ref().map(|s| s.id.clone())
    }

    /// Whether the open collection has unsaved changes (Git refuses to change files then).
    pub fn has_unsaved_changes(&self) -> bool {
        lock(&self.session)
            .as_ref()
            .is_some_and(|s| !s.workspace.dirty().is_empty())
    }

    /// Reads the open collection's folder again (after Git changed its files).
    pub fn reload(&self) -> Result<CollectionInfo, AppError> {
        let root = self
            .root()
            .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
        self.open(&root)
    }

    fn with_session<T>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut guard = lock(&self.session);
        let session = guard
            .as_mut()
            .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
        f(session)
    }

    /// Opens the collection in `root` (the folder the user picked).
    pub fn open(&self, root: &Path) -> Result<CollectionInfo, AppError> {
        let (root, id) = files::collection_id(root)
            .map_err(|e| AppError::Shell(format!("cannot open {}: {e}", root.display())))?;
        let fileset = files::read_folder(&root)
            .map_err(|e| AppError::Shell(format!("cannot read {}: {e}", root.display())))?;
        let workspace = Workspace::open(fileset)?;
        let folder_name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.clone());
        let name = match workspace.collection().name.trim() {
            "" => folder_name,
            name => name.to_owned(),
        };
        let info = CollectionInfo {
            id: id.clone(),
            name,
            location: id.clone(),
        };
        *lock(&self.session) = Some(Session {
            id,
            root,
            workspace,
        });
        // Bodies of the previous collection are no longer reachable from the UI.
        *lock(&self.responses) = ResponseStore::new();
        self.recent.opened(&info);
        Ok(info)
    }

    pub fn tree(&self) -> Result<TreeNode, AppError> {
        self.with_session(|s| Ok(s.workspace.tree()))
    }

    pub fn get_request(&self, path: &str) -> Result<RequestSpec, AppError> {
        self.with_session(|s| s.workspace.request(path))
    }

    pub fn create_request(&self, folder: &str, name: &str) -> Result<String, AppError> {
        self.with_session(|s| s.workspace.create_request(folder, name))
    }

    /// Makes sure a folder exists (wave 6); returns its path. Unsaved.
    pub fn create_folder(&self, path: &str) -> Result<String, AppError> {
        self.with_session(|s| s.workspace.create_folder(path))
    }

    /// What a plugin tab runs with (wave 6): its context (environment, non-secret variables)
    /// and the URLs `{request}` covers (`rt_app::plugins::tab_scope`).
    pub fn plugin_scope(
        &self,
        request_path: &str,
        environment: Option<&str>,
    ) -> Result<rt_plugin::host::TabScope, AppError> {
        self.with_session(|s| {
            Ok(rt_app::plugins::tab_scope(
                s.workspace.collection(),
                request_path,
                environment,
            ))
        })
    }

    pub fn update_request(&self, path: &str, spec: RequestSpec) -> Result<(), AppError> {
        self.with_session(|s| s.workspace.update_request(path, spec))
    }

    pub fn dirty(&self) -> Result<Vec<String>, AppError> {
        self.with_session(|s| Ok(s.workspace.dirty()))
    }

    /// Writes unsaved changes. The workspace only forgets its dirty state when every file
    /// was written; on an I/O error the changes stay unsaved and can be saved again.
    pub fn save(&self) -> Result<Vec<FileChange>, AppError> {
        self.with_session(|s| {
            let mut next = s.workspace.clone();
            let changes = next.save()?;
            files::write_changes(&s.root, &changes)
                .map_err(|e| AppError::Shell(format!("cannot save: {e}")))?;
            s.workspace = next;
            Ok(changes)
        })
    }

    pub fn environments(&self) -> Result<Vec<Environment>, AppError> {
        self.with_session(|s| Ok(s.workspace.environments()))
    }

    pub fn update_environment(&self, env: Environment) -> Result<(), AppError> {
        self.with_session(|s| s.workspace.update_environment(env))
    }

    pub fn delete_environment(&self, name: &str) -> Result<(), AppError> {
        self.with_session(|s| s.workspace.delete_environment(name))
    }

    pub fn preview(&self, path: &str, input: SendInput) -> Result<Preview, AppError> {
        self.with_session(|s| {
            let input = self.with_secrets(s, input)?;
            s.workspace.preview(path, input)
        })
    }

    /// Sends the current (possibly unsaved) state of the request.
    pub async fn send(&self, path: &str, input: SendInput) -> Result<StoredResponse, AppError> {
        // External secrets first, on a blocking thread (a CLI may take seconds, e.g. for a
        // biometric prompt); the values land in the resolver's cache, which `with_secrets`
        // reads. A secret that cannot be read stops the send with the reason.
        let pending = self.with_session(|s| self.external_pending(s, &input))?;
        if let Some((external, skip)) = pending {
            let resolver = std::sync::Arc::clone(&self.external_secrets);
            tokio::task::spawn_blocking(move || resolver.resolve(&external, &skip))
                .await
                .map_err(|e| AppError::Shell(format!("reading secrets failed: {e}")))?
                .map_err(AppError::Shell)?;
        }
        let (snapshot, collection, spec) = self.with_session(|s| {
            let input = self.with_secrets(s, input)?;
            let spec = s.workspace.request(path)?;
            Ok((s.workspace.snapshot(path, input)?, s.id.clone(), spec))
        })?;
        let transport = self.transport.clone();
        let parts = std::sync::Arc::clone(&self.services);
        let send_collection = collection.clone();
        // The core's send future is not `Send` (response bodies are `!Send` streams, as in
        // the browser), so it runs on a blocking thread driven by the app's tokio runtime.
        let runtime = tokio::runtime::Handle::current();
        let report = tokio::task::spawn_blocking(move || {
            let services = parts.for_send(&send_collection);
            runtime.block_on(snapshot.send_with(
                &transport,
                &SendOptions {
                    iteration: None,
                    body_limit: BODY_LIMIT,
                },
                &services.services(),
            ))
        })
        .await
        .map_err(|e| AppError::Shell(format!("send failed: {e}")))?;
        self.history
            .record(&collection, &spec.name, &spec.method, &report);
        Ok(lock(&self.responses).insert(report))
    }

    /// Request paths below `folder` in run order (wave 7, strand D, `runner.rs`).
    pub fn run_plan(&self, folder: Option<&str>) -> Result<Vec<String>, AppError> {
        self.with_session(|s| Ok(rt_core::runner::plan(s.workspace.collection(), folder)))
    }

    /// The open collection for a run (wave 7, strand D, `runner.rs`) and its id: unsaved
    /// edits included, the secret values of `input`'s environment injected as for a send
    /// (keychain, then external secrets, read first on a blocking thread).
    pub async fn run_snapshot(
        &self,
        input: SendInput,
    ) -> Result<(rt_app::runner::RunSnapshot, String), AppError> {
        let pending = self.with_session(|s| self.external_pending(s, &input))?;
        if let Some((external, skip)) = pending {
            let resolver = std::sync::Arc::clone(&self.external_secrets);
            tokio::task::spawn_blocking(move || resolver.resolve(&external, &skip))
                .await
                .map_err(|e| AppError::Shell(format!("reading secrets failed: {e}")))?
                .map_err(AppError::Shell)?;
        }
        self.with_session(|s| {
            let input = self.with_secrets(s, input)?;
            Ok((s.workspace.run_snapshot(input), s.id.clone()))
        })
    }

    pub fn response_lines(&self, id: u64, start: u64, count: u32) -> Result<LineSlice, AppError> {
        lock(&self.responses).lines(id, start, count)
    }

    pub fn response_bytes(&self, id: u64, offset: u64, length: u32) -> Result<Vec<u8>, AppError> {
        lock(&self.responses).bytes(id, offset, length)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<rt_app::SearchHit>, AppError> {
        self.with_session(|s| Ok(rt_app::search::search(&s.workspace, query, limit)))
    }

    pub fn filter_json(&self, id: u64, expression: &str) -> Result<String, AppError> {
        lock(&self.responses).filter_json(id, expression)
    }

    pub fn release_response(&self, id: u64) {
        lock(&self.responses).release(id);
    }

    fn secret_key(&self, environment: &str, variable: &str) -> Result<SecretKey, AppError> {
        self.with_session(|s| {
            Ok(SecretKey {
                collection: s.id.clone(),
                environment: environment.to_owned(),
                variable: variable.to_owned(),
            })
        })
    }

    pub fn secret_has(&self, environment: &str, variable: &str) -> Result<bool, AppError> {
        Ok(self.secret_get(environment, variable)?.is_some())
    }

    pub fn secret_get(
        &self,
        environment: &str,
        variable: &str,
    ) -> Result<Option<String>, AppError> {
        self.secrets.get(&self.secret_key(environment, variable)?)
    }

    pub fn secret_set(
        &self,
        environment: &str,
        variable: &str,
        value: &str,
    ) -> Result<(), AppError> {
        self.secrets
            .set(&self.secret_key(environment, variable)?, value)
    }

    pub fn secret_delete(&self, environment: &str, variable: &str) -> Result<(), AppError> {
        self.secrets
            .delete(&self.secret_key(environment, variable)?)
    }

    /// Replaces `input.secrets` (never trusted from the webview) with the stored values of
    /// the selected environment's secret variables, plus its external secrets already read
    /// (keychain values win).
    fn with_secrets(&self, s: &Session, mut input: SendInput) -> Result<SendInput, AppError> {
        let environments = s.workspace.environments();
        let mut values = secret_values(
            &self.secrets,
            &s.id,
            &environments,
            input.environment.as_deref(),
        )?;
        if let Some(external) = selected(&environments, input.environment.as_deref())
            .and_then(|e| e.external_secrets.as_ref())
        {
            for (name, value) in self.external_secrets.cached(external) {
                if !values.iter().any(|(n, _)| *n == name) {
                    values.push((name, value));
                }
            }
        }
        input.secrets = values;
        Ok(input)
    }

    /// The selected environment's external secrets and the names to skip (those with a
    /// keychain value); `None` when there is nothing to read.
    fn external_pending(
        &self,
        s: &Session,
        input: &SendInput,
    ) -> Result<Option<(rt_core::model::ExternalSecrets, Vec<String>)>, AppError> {
        let environments = s.workspace.environments();
        let Some(external) = selected(&environments, input.environment.as_deref())
            .and_then(|e| e.external_secrets.clone())
        else {
            return Ok(None);
        };
        let skip = secret_values(
            &self.secrets,
            &s.id,
            &environments,
            input.environment.as_deref(),
        )?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
        Ok(Some((external, skip)))
    }
}

fn selected<'a>(environments: &'a [Environment], name: Option<&str>) -> Option<&'a Environment> {
    name.and_then(|name| environments.iter().find(|e| e.name == name))
}

/// Values of the enabled secret variables of `environment`, from `store`. Variables without a
/// stored value are left out (they stay unresolved).
pub fn secret_values(
    store: &impl SecretStore,
    collection: &str,
    environments: &[Environment],
    environment: Option<&str>,
) -> Result<Vec<(String, String)>, AppError> {
    let Some(env) = environment.and_then(|name| environments.iter().find(|e| e.name == name))
    else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    for var in env.variables.iter().filter(|v| v.secret && v.enabled) {
        let key = SecretKey {
            collection: collection.to_owned(),
            environment: env.name.clone(),
            variable: var.name.clone(),
        };
        if let Some(value) = store.get(&key)? {
            values.push((var.name.clone(), value));
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rt_core::model::Variable;

    use super::*;

    /// In-memory stand-in for the OS keychain.
    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            Ok(lock(&self.0).get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            lock(&self.0).insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            lock(&self.0).remove(key);
            Ok(())
        }
    }

    fn secret(name: &str, enabled: bool) -> Variable {
        Variable {
            name: name.into(),
            value: None,
            enabled,
            secret: true,
        }
    }

    #[test]
    fn injects_only_stored_enabled_secrets_of_the_selected_environment() {
        let store = MemorySecrets::default();
        let key = |env: &str, var: &str| SecretKey {
            collection: "/c".into(),
            environment: env.into(),
            variable: var.into(),
        };
        store.set(&key("Local", "password"), "hunter2").unwrap();
        store.set(&key("Local", "off"), "x").unwrap();
        store.set(&key("Prod", "password"), "prod-secret").unwrap();
        store.set(&key("Local", "plain"), "not-a-secret").unwrap();
        let envs = vec![
            Environment {
                client_certificates: Vec::new(),
                external_secrets: None,
                name: "Local".into(),
                variables: vec![
                    secret("password", true),
                    secret("off", false),
                    secret("missing", true),
                    Variable::new("plain", "p"),
                ],
            },
            Environment {
                client_certificates: Vec::new(),
                external_secrets: None,
                name: "Prod".into(),
                variables: vec![secret("password", true)],
            },
        ];
        assert_eq!(
            secret_values(&store, "/c", &envs, Some("Local")).unwrap(),
            [("password".to_owned(), "hunter2".to_owned())]
        );
        assert_eq!(
            secret_values(&store, "/c", &envs, Some("Prod")).unwrap(),
            [("password".to_owned(), "prod-secret".to_owned())]
        );
        assert!(secret_values(&store, "/c", &envs, None).unwrap().is_empty());
        assert!(
            secret_values(&store, "/c", &envs, Some("Nope"))
                .unwrap()
                .is_empty()
        );
        // Another collection's values never leak in.
        assert!(
            secret_values(&store, "/other", &envs, Some("Local"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn commands_without_an_open_collection_fail_with_a_shell_error() {
        let core = Core::new(MemorySecrets::default()).unwrap();
        assert!(matches!(core.tree(), Err(AppError::Shell(_))));
        assert!(matches!(core.save(), Err(AppError::Shell(_))));
        assert!(matches!(
            core.secret_get("Local", "password"),
            Err(AppError::Shell(_))
        ));
    }

    #[test]
    fn opening_a_missing_folder_is_a_shell_error() {
        let core = Core::new(MemorySecrets::default()).unwrap();
        let err = core
            .open(Path::new("/definitely/not/here/rt-desktop"))
            .unwrap_err();
        assert!(matches!(err, AppError::Shell(m) if m.contains("cannot open")));
    }

    #[test]
    fn errors_serialize_like_the_ts_app_error() {
        let json = serde_json::to_value(AppError::Shell("x".into())).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "shell", "message": "x" }));
        let json = serde_json::to_value(AppError::ResponseNotFound(7)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "response-not-found", "message": 7 })
        );
    }
}
