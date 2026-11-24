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
//! - Wave 14 (strand A): several collections. The core holds a *collection set*
//!   (`rt_app::collections`): the roots the user opened or added, the collections found in
//!   them (`collections.rs` walks the folders), one session per collection that was read, and
//!   the active one. Every command above acts on the active collection when it is called; a
//!   collection that is not active is read only when something asks for it (its tree when the
//!   sidebar node opens, search, save). Ids are canonical collection paths, so a collection
//!   has the same id (and secrets) whether it is opened alone or through a parent folder.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use std::collections::HashMap;

use rt_app::collections::{
    CollectionChanges, CollectionEntry, CollectionRoot, CollectionSearchHit, CollectionSet,
    NewCollection,
};
use rt_app::{
    AppError, FileChange, Imports, LineSlice, Preview, ResponseStore, SecretKey, SecretStore,
    SendInput, StoredResponse, TreeNode, Workspace,
};
use rt_core::model::{Environment, RequestSpec};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use serde::Serialize;

use crate::collections::{self as set_files, Found};
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
    /// A new collection (wave 14, `create_collection`): its files, written on the first save
    /// (until then the folder does not exist).
    created: Vec<FileChange>,
}

impl Session {
    fn read(id: &str, root: &Path) -> Result<Self, AppError> {
        let fileset = files::read_folder(root)
            .map_err(|e| AppError::Shell(format!("cannot read {}: {e}", root.display())))?;
        Ok(Self {
            id: id.to_owned(),
            root: root.to_path_buf(),
            workspace: Workspace::open(fileset)?,
            created: Vec::new(),
        })
    }

    fn dirty(&self) -> Vec<String> {
        let mut dirty = self.workspace.dirty();
        for c in &self.created {
            if !dirty.contains(&c.path) {
                dirty.push(c.path.clone());
            }
        }
        dirty.sort();
        dirty
    }

    /// Writes unsaved changes. The workspace only forgets its dirty state when every file
    /// was written; on an I/O error the changes stay unsaved and can be saved again.
    fn save(&mut self) -> Result<Vec<FileChange>, AppError> {
        let mut next = self.workspace.clone();
        let mut changes = next.save()?;
        if !self.created.is_empty() {
            std::fs::create_dir_all(&self.root)
                .map_err(|e| AppError::Shell(format!("cannot save: {e}")))?;
            for c in &self.created {
                if !changes.iter().any(|x| x.path == c.path) {
                    changes.push(c.clone());
                }
            }
            changes.sort_by(|a, b| a.path.cmp(&b.path));
        }
        files::write_changes(&self.root, &changes)
            .map_err(|e| AppError::Shell(format!("cannot save: {e}")))?;
        self.workspace = next;
        self.created.clear();
        Ok(changes)
    }

    fn name(&self) -> Option<String> {
        Some(self.workspace.collection().name.trim().to_owned()).filter(|n| !n.is_empty())
    }
}

/// A folder the user opened or added.
#[derive(Debug, Clone)]
struct Root {
    id: String,
    path: PathBuf,
    name: String,
    collections: Vec<String>,
    truncated: bool,
    /// Wave 15: the repository of a *repository root* (no collections, in Git).
    repo: Option<String>,
}

/// One collection of the set.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    name: String,
    path: PathBuf,
    root: String,
    dir: String,
    repo: Option<String>,
}

/// The collection set (wave 14): roots in order, their collections, the sessions read so far.
#[derive(Default)]
struct Sets {
    roots: Vec<Root>,
    entries: Vec<Entry>,
    sessions: HashMap<String, Session>,
    active: Option<String>,
    /// Wave 15: the repository chosen last in the sidebar; `None`: the active collection's.
    chosen_repo: Option<String>,
}

impl Sets {
    fn entry(&self, id: &str) -> Result<&Entry, AppError> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| AppError::Shell(format!("no collection {id} in the sidebar")))
    }

    /// The session of collection `id`, read from disk on first use.
    fn session(&mut self, id: &str) -> Result<&mut Session, AppError> {
        if !self.sessions.contains_key(id) {
            let path = self.entry(id)?.path.clone();
            let session = Session::read(id, &path)?;
            self.sessions.insert(id.to_owned(), session);
        }
        Ok(self.sessions.get_mut(id).expect("just inserted"))
    }

    fn active_session(&mut self) -> Result<&mut Session, AppError> {
        let id = self
            .active
            .clone()
            .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
        self.session(&id)
    }

    fn info(&self, id: &str) -> Result<CollectionInfo, AppError> {
        let entry = self.entry(id)?;
        let name = self
            .sessions
            .get(id)
            .and_then(Session::name)
            .unwrap_or_else(|| entry.name.clone());
        Ok(CollectionInfo {
            id: entry.id.clone(),
            name,
            location: entry.id.clone(),
        })
    }

    /// Adds the collections `found` in a root (those not in the set yet).
    fn push_root(&mut self, found: Found) -> &Root {
        let Found {
            id,
            path,
            name,
            collections,
            truncated,
            repo_root,
        } = found;
        let mut ids = Vec::new();
        for c in collections {
            if self.entries.iter().any(|e| e.id == c.id) {
                continue;
            }
            ids.push(c.id.clone());
            self.entries.push(Entry {
                id: c.id,
                name: c.name,
                path: c.path,
                root: id.clone(),
                dir: c.dir,
                repo: c.repo,
            });
        }
        self.roots.push(Root {
            id,
            path,
            name,
            collections: ids,
            truncated,
            repo: repo_root,
        });
        self.roots.last().expect("just pushed")
    }

    fn set(&self) -> CollectionSet {
        CollectionSet {
            roots: self
                .roots
                .iter()
                .map(|r| CollectionRoot {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    location: r.id.clone(),
                    collections: r.collections.clone(),
                    truncated: r.truncated,
                })
                .collect(),
            collections: self
                .entries
                .iter()
                .map(|e| CollectionEntry {
                    id: e.id.clone(),
                    name: self
                        .sessions
                        .get(&e.id)
                        .and_then(Session::name)
                        .unwrap_or_else(|| e.name.clone()),
                    location: e.id.clone(),
                    root: e.root.clone(),
                    dir: e.dir.clone(),
                    repo: e.repo.clone(),
                    dirty: self.sessions.get(&e.id).map_or(0, |s| s.dirty().len()),
                })
                .collect(),
            active: self.active.clone(),
            repositories: self.repositories(),
            active_repository: self.active_repo(),
        }
    }

    /// Wave 15: the repositories of the set in sidebar order: those that hold collections, then
    /// the repository roots without one (`rt_app::git::repos::repositories`).
    fn repositories(&self) -> Vec<rt_app::git::repos::RepositoryEntry> {
        let entries: Vec<CollectionEntry> = self
            .entries
            .iter()
            .map(|e| CollectionEntry {
                id: e.id.clone(),
                name: e.name.clone(),
                location: e.id.clone(),
                root: e.root.clone(),
                dir: e.dir.clone(),
                repo: e.repo.clone(),
                dirty: 0,
            })
            .collect();
        let roots: Vec<rt_app::git::repos::RepoRoot> = self
            .roots
            .iter()
            .filter_map(|r| {
                r.repo.as_ref().map(|repo| rt_app::git::repos::RepoRoot {
                    root: r.id.clone(),
                    repo: repo.clone(),
                })
            })
            .collect();
        rt_app::git::repos::repositories(&entries, &roots, |id| {
            let path = Path::new(id);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.to_owned());
            // `.git` is a file in a linked worktree and in a submodule's checkout.
            (name, id.to_owned(), path.join(".git").is_file())
        })
    }

    /// Id of the repository Git acts on: the chosen one, else the active collection's.
    fn active_repo(&self) -> Option<String> {
        let entries: Vec<CollectionEntry> = self
            .entries
            .iter()
            .map(|e| CollectionEntry {
                id: e.id.clone(),
                name: String::new(),
                location: String::new(),
                root: e.root.clone(),
                dir: String::new(),
                repo: e.repo.clone(),
                dirty: 0,
            })
            .collect();
        rt_app::git::repos::active_repository(
            &self.repositories(),
            &entries,
            self.chosen_repo.as_deref(),
            self.active.as_deref(),
        )
    }

    /// Ids of the collections in the active repository (just the active collection outside
    /// Git).
    fn repo_mates(&self) -> Vec<String> {
        match self.active_repo() {
            Some(repo) => self
                .entries
                .iter()
                .filter(|e| e.repo.as_deref() == Some(repo.as_str()))
                .map(|e| e.id.clone())
                .collect(),
            None => self.active.iter().cloned().collect(),
        }
    }
}

pub struct Core<S> {
    session: Mutex<Sets>,
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
    /// Wave 15: what the commits of this core are signed and verified with (`rt_git::sign`);
    /// the default signs nothing.
    pub signing: rt_git::sign::Signing,
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
            session: Mutex::new(Sets::default()),
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
            signing: rt_git::sign::Signing::default(),
        })
    }

    /// Wave 15: every commit this core writes goes through `signing` (a distribution's hook,
    /// managed on the builder as `rt_git::sign::Signing`); `GitApi.verify` asks its verifier.
    /// Call it right after creating the core.
    pub fn with_signing(mut self, signing: rt_git::sign::Signing) -> Self {
        self.signing = signing;
        self
    }

    /// Wave 10: every send of this core (send, runner, streams, gRPC) asks `guard` first
    /// (`rt_core::guard`). Call it right after creating the core, before it is shared.
    pub fn with_guard(mut self, guard: rt_core::guard::GuardHandle) -> Self {
        std::sync::Arc::get_mut(&mut self.services)
            .expect("with_guard is called before the core is shared")
            .guard = guard;
        self
    }

    /// Wave 17: every send of this core hands its masked exchange to `observers`
    /// (`rt_core::observe`; a distribution's, managed on the builder as
    /// `rt_core::observe::ObserverHandle`). Call it right after creating the core, before it is
    /// shared.
    pub fn with_observers(mut self, observers: rt_core::observe::ObserverHandle) -> Self {
        std::sync::Arc::get_mut(&mut self.services)
            .expect("with_observers is called before the core is shared")
            .observers = observers;
        self
    }

    /// The secret store (Git credentials are kept there too).
    pub fn secrets(&self) -> &S {
        &self.secrets
    }

    /// Root folder of the open (active) collection.
    pub fn root(&self) -> Option<PathBuf> {
        let sets = lock(&self.session);
        let id = sets.active.as_deref()?;
        sets.entry(id).ok().map(|e| e.path.clone())
    }

    /// Wave 15: the working tree (or a folder inside it) of the repository Git acts on, the
    /// active repository of the set. Interface: the active collection's root, from which
    /// `rt_git::Repo::open` finds the repository; strand A adds repository roots without a
    /// collection and the repository chosen in the sidebar. Public for distributions (e.g. a
    /// Tauri plugin that works on the same repository).
    pub fn active_repository(&self) -> Option<PathBuf> {
        let sets = lock(&self.session);
        // A repository chosen in the sidebar: its working tree (paths are then relative to it).
        if let Some(repo) = sets.chosen_repo.as_deref()
            && sets.repositories().iter().any(|r| r.id == repo)
        {
            return Some(PathBuf::from(repo));
        }
        let id = sets.active.as_deref()?;
        sets.entry(id).ok().map(|e| e.path.clone())
    }

    /// Id (`CollectionInfo::id`) of the open (active) collection.
    pub fn collection_id(&self) -> Option<String> {
        lock(&self.session).active.clone()
    }

    /// Whether the open collection (with unsaved changes) has any script (wave 7).
    pub fn has_scripts(&self) -> bool {
        self.with_session(|s| Ok(rt_app::scripts::has_scripts(s.workspace.collection())))
            .unwrap_or(false)
    }

    /// What coding agents may do in the open collection (wave 8, strand C, `agents.rs`).
    pub fn agent_policy(&self) -> Result<rt_core::model::AgentPolicy, AppError> {
        self.with_session(|s| Ok(s.workspace.collection().config.agents.clone()))
    }

    /// What the agent rules say about sending `path` as it is in memory (`agents_check`).
    pub fn agent_check(
        &self,
        path: &str,
        environment: Option<&str>,
    ) -> Result<rt_app::agents::AgentCheck, AppError> {
        self.with_session(|s| rt_app::agents::check(s.workspace.collection(), path, environment))
    }

    /// Changes the agent policy in memory, like other edits (wave 8, strand C).
    pub fn set_agent_policy(&self, policy: &rt_core::model::AgentPolicy) -> Result<(), AppError> {
        self.with_session(|s| s.workspace.set_agent_policy(policy))
    }

    /// Whether a collection in the active collection's repository has unsaved changes (Git
    /// refuses to change files then; wave 14: every collection of the repository counts).
    pub fn has_unsaved_changes(&self) -> bool {
        let sets = lock(&self.session);
        sets.repo_mates()
            .iter()
            .any(|id| sets.sessions.get(id).is_some_and(|s| !s.dirty().is_empty()))
    }

    /// Reads the files again after Git changed them: every collection of the active
    /// collection's repository that was read (wave 14), after finding the roots' collections
    /// again (a checkout may add or remove some). Returns the active collection; `None` when
    /// the set has none (wave 15: it may hold only repository roots).
    pub fn reload(&self) -> Result<Option<CollectionInfo>, AppError> {
        let mut sets = lock(&self.session);
        let active = sets.active.clone();
        let mates = sets.repo_mates();
        let mut next = Sets::default();
        for root in &sets.roots {
            match set_files::find(&root.path) {
                Ok(found) => {
                    next.push_root(found);
                }
                Err(e) => {
                    // Keep what the sidebar showed for a root that cannot be read now.
                    eprintln!("RequestTailor: cannot read {}: {e}", root.path.display());
                    next.roots.push(root.clone());
                    next.entries
                        .extend(sets.entries.iter().filter(|e| e.root == root.id).cloned());
                }
            }
        }
        // New collections not saved yet are not on disk: they stay.
        for entry in &sets.entries {
            let unsaved_new = sets
                .sessions
                .get(&entry.id)
                .is_some_and(|s| !s.created.is_empty());
            if unsaved_new && !next.entries.iter().any(|e| e.id == entry.id) {
                if let Some(r) = next.roots.iter_mut().find(|r| r.id == entry.root) {
                    r.collections.push(entry.id.clone());
                }
                next.entries.push(entry.clone());
            }
        }
        // Collections of the repository are read again; the others keep their state.
        for (id, session) in std::mem::take(&mut sets.sessions) {
            let present = next.entries.iter().any(|e| e.id == id);
            if present && (!mates.contains(&id) || !session.created.is_empty()) {
                next.sessions.insert(id, session);
            }
        }
        next.chosen_repo = sets.chosen_repo.clone();
        next.active = active
            .filter(|a| next.entries.iter().any(|e| e.id == *a))
            .or_else(|| next.entries.first().map(|e| e.id.clone()));
        let Some(id) = next.active.clone() else {
            *sets = next;
            return Ok(None);
        };
        next.session(&id)?;
        let info = next.info(&id)?;
        *sets = next;
        Ok(Some(info))
    }

    /// The open workspace and the collection id, for the per-strand modules (wave 8: saved
    /// MCP tests are staged in it, `mcp_client.rs`).
    pub fn with_workspace<T>(
        &self,
        f: impl FnOnce(&mut Workspace, &str) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.with_session(|s| f(&mut s.workspace, &s.id))
    }

    fn with_session<T>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut guard = lock(&self.session);
        f(guard.active_session()?)
    }

    /// Opens what the folder `root` holds as the only root of the collection set (the folder
    /// the user picked): the collection itself, or every collection in its subfolders. The
    /// first one becomes active (wave 14). The set stays as it was if that one cannot be read.
    pub fn open(&self, root: &Path) -> Result<CollectionInfo, AppError> {
        let mut found = set_files::find(root)
            .map_err(|e| AppError::Shell(format!("cannot open {}: {e}", root.display())))?;
        if found.collections.is_empty() {
            // No collection anywhere below: the folder is tried as one (as before wave 14).
            found.collections.push(set_files::FoundCollection {
                id: found.id.clone(),
                name: found.name.clone(),
                path: found.path.clone(),
                dir: String::new(),
                repo: set_files::repo_of(&found.path),
            });
        }
        let first = found
            .collections
            .first()
            .map(|c| (c.id.clone(), c.path.clone()))
            .ok_or_else(|| AppError::Shell(format!("cannot open {}", root.display())))?;
        let session = Session::read(&first.0, &first.1)?;
        let mut sets = Sets::default();
        let root_info = root_info(&found, Some(&session));
        sets.push_root(found);
        sets.sessions.insert(first.0.clone(), session);
        sets.active = Some(first.0.clone());
        let info = sets.info(&first.0)?;
        let set = sets.set();
        *lock(&self.session) = sets;
        // Bodies of the previous collections are no longer reachable from the UI.
        *lock(&self.responses) = ResponseStore::new();
        self.recent.opened(&root_info);
        self.remember(&set);
        Ok(info)
    }

    // ---- wave 14: the collection set ----

    /// The collection set as the sidebar shows it.
    pub fn collection_set(&self) -> CollectionSet {
        lock(&self.session).set()
    }

    /// Number of collections read so far (the others are read when something asks).
    pub fn loaded_collections(&self) -> usize {
        lock(&self.session).sessions.len()
    }

    /// "Add collection…": adds the folder `root` as another root (its collections after the
    /// existing ones; collections already in the set are not added twice) and makes its first
    /// new collection active. An empty set is opened as by [`Core::open`].
    pub fn add_root(&self, root: &Path) -> Result<CollectionSet, AppError> {
        let found = set_files::find(root)
            .map_err(|e| AppError::Shell(format!("cannot open {}: {e}", root.display())))?;
        if found.repo_root.is_some() {
            // Wave 15: a folder without a collection in a Git repository is a repository root.
            return self.add_repository_root(found);
        }
        if lock(&self.session).roots.is_empty() {
            self.open(root)?;
            return Ok(self.collection_set());
        }
        if found.collections.is_empty() {
            return Err(AppError::Shell(format!(
                "there is no collection in {}; create one with New… → Collection",
                root.display()
            )));
        }
        let root_info = root_info(&found, None);
        let set = {
            let mut sets = lock(&self.session);
            // The same folder again, or one whose collections are all in the set already.
            let known = |c: &set_files::FoundCollection| sets.entries.iter().any(|e| e.id == c.id);
            if sets.roots.iter().any(|r| r.id == found.id) || found.collections.iter().all(known) {
                return Ok(sets.set());
            }
            let added = sets.push_root(found).collections.clone();
            if let Some(first) = added.first() {
                // Read it now: a folder that is no collection fails here, not later.
                if let Err(e) = sets.session(first) {
                    sets.roots.pop();
                    sets.entries.retain(|e| !added.contains(&e.id));
                    return Err(e);
                }
                sets.active = Some(first.clone());
                // Wave 15: Git follows the active collection again.
                sets.chosen_repo = None;
            }
            sets.set()
        };
        self.recent.opened(&root_info);
        self.remember(&set);
        Ok(set)
    }

    /// Adds the folder `root` as a root for a new collection (`create_collection`), also
    /// when it holds none yet; the collections it holds join the set. Returns the root's id.
    pub fn add_folder_root(&self, root: &Path) -> Result<String, AppError> {
        let found = set_files::find(root)
            .map_err(|e| AppError::Shell(format!("cannot open {}: {e}", root.display())))?;
        let id = found.id.clone();
        let root_info = root_info(&found, None);
        let set = {
            let mut sets = lock(&self.session);
            if sets.roots.iter().any(|r| r.id == id) {
                return Ok(id);
            }
            sets.push_root(found);
            sets.set()
        };
        self.recent.opened(&root_info);
        self.remember(&set);
        Ok(id)
    }

    /// Takes the root `root` and its collections out of the set (files untouched). Refused
    /// while one of them has unsaved changes.
    pub fn remove_root(&self, root: &str) -> Result<CollectionSet, AppError> {
        let set = {
            let mut sets = lock(&self.session);
            let Some(index) = sets.roots.iter().position(|r| r.id == root) else {
                return Ok(sets.set());
            };
            let ids = sets.roots[index].collections.clone();
            if let Some(dirty) = ids.iter().find(|id| {
                sets.sessions
                    .get(*id)
                    .is_some_and(|s| !s.dirty().is_empty())
            }) {
                let name = sets.info(dirty).map(|i| i.name).unwrap_or_default();
                return Err(AppError::Shell(format!(
                    "\"{name}\" has unsaved changes; save them before removing the folder"
                )));
            }
            sets.roots.remove(index);
            sets.entries.retain(|e| !ids.contains(&e.id));
            for id in &ids {
                sets.sessions.remove(id);
            }
            if sets.active.as_ref().is_some_and(|a| ids.contains(a)) {
                sets.active = sets.entries.first().map(|e| e.id.clone());
            }
            sets.set()
        };
        self.remember(&set);
        Ok(set)
    }

    /// Makes collection `id` the active one (read from disk if it was not yet).
    pub fn activate(&self, id: &str) -> Result<CollectionInfo, AppError> {
        let (info, set) = {
            let mut sets = lock(&self.session);
            sets.session(id)?;
            sets.active = Some(id.to_owned());
            // Wave 15: Git follows the active collection again.
            sets.chosen_repo = None;
            (sets.info(id)?, sets.set())
        };
        self.remember(&set);
        Ok(info)
    }

    // ---- wave 15: repositories in the set ----

    /// A folder without a collection in a Git repository joins the set as a *repository root*
    /// (no scaffold, no `opencollection.yml`) and its repository becomes the active one. An
    /// empty set is replaced by it; a repository that is in the set already is only made
    /// active.
    fn add_repository_root(&self, found: Found) -> Result<CollectionSet, AppError> {
        let repo = found.repo_root.clone().expect("a repository root");
        let set = {
            let mut sets = lock(&self.session);
            if sets.roots.is_empty() {
                let mut fresh = Sets::default();
                fresh.push_root(found.clone());
                fresh.chosen_repo = Some(repo);
                *sets = fresh;
                // Bodies of the previous collections are no longer reachable from the UI.
                *lock(&self.responses) = ResponseStore::new();
            } else {
                if !sets.roots.iter().any(|r| r.id == found.id)
                    && !sets.repositories().iter().any(|r| r.id == repo)
                {
                    sets.push_root(found.clone());
                }
                sets.chosen_repo = Some(repo);
            }
            sets.set()
        };
        self.remember(&set);
        Ok(set)
    }

    /// Makes repository `id` the one Git acts on (until a collection is activated).
    pub fn activate_repository(
        &self,
        id: &str,
    ) -> Result<rt_app::git::repos::RepositoryEntry, AppError> {
        let (entry, set) = {
            let mut sets = lock(&self.session);
            let entry = sets
                .repositories()
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| AppError::Shell(format!("no repository {id} in the sidebar")))?;
            sets.chosen_repo = Some(entry.id.clone());
            (entry, sets.set())
        };
        self.remember(&set);
        Ok(entry)
    }

    /// The repository `id` of the set.
    pub fn repository(&self, id: &str) -> Result<rt_app::git::repos::RepositoryEntry, AppError> {
        lock(&self.session)
            .repositories()
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| AppError::Shell(format!("no repository {id} in the sidebar")))
    }

    /// The folder of collection `id` (a new, unsaved collection's folder may not exist yet).
    pub fn path_of(&self, id: &str) -> Result<PathBuf, AppError> {
        Ok(lock(&self.session).entry(id)?.path.clone())
    }

    /// The tree of any collection of the set, unsaved changes included.
    pub fn tree_of(&self, id: &str) -> Result<TreeNode, AppError> {
        Ok(lock(&self.session).session(id)?.workspace.tree())
    }

    /// Unsaved paths of any collection of the set.
    pub fn dirty_of(&self, id: &str) -> Result<Vec<String>, AppError> {
        Ok(lock(&self.session).session(id)?.dirty())
    }

    /// Writes the unsaved changes of every collection, in sidebar order; stops at the first
    /// collection that cannot be written (the others before it are written).
    pub fn save_all(&self) -> Result<Vec<CollectionChanges>, AppError> {
        let mut sets = lock(&self.session);
        let ids: Vec<String> = sets.entries.iter().map(|e| e.id.clone()).collect();
        let mut written = Vec::new();
        for id in ids {
            let Some(session) = sets.sessions.get_mut(&id) else {
                continue;
            };
            if session.dirty().is_empty() {
                continue;
            }
            let changes = session.save()?;
            written.push(CollectionChanges {
                collection: id,
                changes,
            });
        }
        Ok(written)
    }

    /// Full-text search over every collection of the set (each read if it was not yet).
    pub fn search_all(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CollectionSearchHit>, AppError> {
        let mut sets = lock(&self.session);
        let ids: Vec<String> = sets.entries.iter().map(|e| e.id.clone()).collect();
        let mut per = Vec::new();
        for id in ids {
            // A collection that cannot be read has no hits; its node shows the error.
            let Ok(session) = sets.session(&id) else {
                continue;
            };
            per.push((id, rt_app::search::search(&session.workspace, query, limit)));
        }
        Ok(rt_app::collections::merge_hits(per, limit))
    }

    /// Creates a collection in a root of the set (`spec.root` must name one; the shell asks
    /// for a folder first otherwise): unsaved until saved, like any edit; it joins the set and
    /// becomes active. The folder must not exist yet or be empty.
    pub fn create_collection(&self, spec: &NewCollection) -> Result<CollectionInfo, AppError> {
        let dir = rt_app::collections::check_new_dir(&spec.dir)?;
        let files = rt_app::collections::scaffold(spec)?;
        let set = {
            let mut sets = lock(&self.session);
            let root_id = spec
                .root
                .clone()
                .ok_or_else(|| AppError::Shell("choose a folder for the new collection".into()))?;
            let root = sets
                .roots
                .iter()
                .find(|r| r.id == root_id)
                .cloned()
                .ok_or_else(|| AppError::Shell(format!("no folder {root_id} in the sidebar")))?;
            let path = if dir.is_empty() {
                root.path.clone()
            } else {
                root.path.join(&dir)
            };
            if !set_files::is_free(&path) {
                return Err(AppError::Shell(format!(
                    "{} already has files in it; choose another folder",
                    path.display()
                )));
            }
            let id = path.to_string_lossy().into_owned();
            if sets.entries.iter().any(|e| e.id == id) {
                return Err(AppError::Shell(format!("{id} is a collection already")));
            }
            let fileset = rt_format::FileSet {
                files: files
                    .iter()
                    .map(|c| (c.path.clone(), c.text.clone().into_bytes()))
                    .collect(),
            };
            let session = Session {
                id: id.clone(),
                root: path.clone(),
                workspace: Workspace::open(fileset)?,
                created: files,
            };
            sets.entries.push(Entry {
                id: id.clone(),
                name: spec.name.trim().to_owned(),
                path: path.clone(),
                root: root.id.clone(),
                dir,
                repo: set_files::repo_of(&path),
            });
            // In sidebar order: after the root's other collections.
            let order: Vec<String> = sets
                .roots
                .iter()
                .flat_map(|r| {
                    let mut ids = r.collections.clone();
                    if r.id == root.id {
                        ids.push(id.clone());
                    }
                    ids
                })
                .collect();
            sets.entries
                .sort_by_key(|e| order.iter().position(|o| *o == e.id).unwrap_or(usize::MAX));
            if let Some(r) = sets.roots.iter_mut().find(|r| r.id == root.id) {
                r.collections.push(id.clone());
            }
            sets.sessions.insert(id.clone(), session);
            sets.active = Some(id.clone());
            (sets.info(&id)?, sets.set())
        };
        let (info, set) = set;
        self.remember(&set);
        Ok(info)
    }

    /// Restores the set remembered on this device if `root` is one of its roots (all its
    /// roots that still exist, and its active collection); else opens `root` alone. `None`
    /// if nothing was remembered for it.
    pub fn restore(&self, root: &Path) -> Result<Option<CollectionInfo>, AppError> {
        let Some(saved) = self.data_dir.as_deref().and_then(set_files::load_roots) else {
            return Ok(None);
        };
        let (_, wanted) = files::collection_id(root).map_err(shell)?;
        if !saved.roots.contains(&wanted) {
            return Ok(None);
        }
        let mut first = true;
        for path in &saved.roots {
            let path = PathBuf::from(path);
            if !path.is_dir() {
                continue;
            }
            let opened = if first {
                // Wave 15: a remembered repository root (a folder in Git without collections).
                match set_files::find(&path) {
                    Ok(found) if found.repo_root.is_some() => {
                        self.add_repository_root(found).map(|_| ())
                    }
                    _ => self.open(&path).map(|_| ()),
                }
            } else {
                self.add_root(&path).map(|_| ())
            };
            match opened {
                Ok(()) => first = false,
                Err(e) => eprintln!("RequestTailor: cannot reopen {}: {e}", path.display()),
            }
        }
        if first {
            return Ok(None);
        }
        let active = saved
            .active
            .filter(|a| lock(&self.session).entries.iter().any(|e| e.id == *a))
            .or_else(|| lock(&self.session).entries.first().map(|e| e.id.clone()));
        match active {
            Some(id) => self.activate(&id).map(Some),
            None => Ok(None),
        }
    }

    /// Remembers the roots and the active collection in the data folder (nothing in tests).
    fn remember(&self, set: &CollectionSet) {
        if let Some(dir) = &self.data_dir {
            set_files::save_roots(dir, set);
        }
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
        self.with_session(|s| Ok(s.dirty()))
    }

    /// Writes the active collection's unsaved changes. The workspace only forgets its dirty
    /// state when every file was written; on an I/O error the changes stay unsaved.
    pub fn save(&self) -> Result<Vec<FileChange>, AppError> {
        self.with_session(Session::save)
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
            // Wave 7: session variables in the runtime layer (`session_vars.rs`).
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            s.workspace.preview(path, input)
        })
    }

    /// Every variable `path` sees, for variable completion (`variables.rs`): the session's
    /// variables in the runtime layer and the secret values only to mask, as for `preview`.
    pub fn variables(
        &self,
        path: &str,
        input: SendInput,
    ) -> Result<rt_app::variables::VariableScope, AppError> {
        self.with_session(|s| {
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            s.workspace.variables(path, &input)
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
            let input = self.services.session.input_for(&s.id, input);
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
            let mut report = runtime.block_on(snapshot.send_with(
                &transport,
                &SendOptions {
                    iteration: None,
                    body_limit: BODY_LIMIT,
                },
                &services.services(),
            ));
            // Wave 7: scripts of an untrusted collection did not run; say so.
            parts
                .scripts
                .note_untrusted(&send_collection, &snapshot.collection, &mut report);
            report
        })
        .await
        .map_err(|e| AppError::Shell(format!("send failed: {e}")))?;
        // Wave 7: what scripts and actions set lives on for the session.
        self.services.session.keep(&collection, &report);
        self.history
            .record(&collection, &spec.name, &spec.method, &report);
        Ok(lock(&self.responses).insert(report))
    }

    /// The request at `path` ready to open as a session (wave 8, strand A, `streams.rs`), and
    /// the collection id: like [`Core::send`] up to the wire (session variables, keychain and
    /// external secrets).
    pub async fn stream_snapshot(
        &self,
        path: &str,
        input: SendInput,
    ) -> Result<(rt_app::SendSnapshot, String), AppError> {
        let pending = self.with_session(|s| self.external_pending(s, &input))?;
        if let Some((external, skip)) = pending {
            let resolver = std::sync::Arc::clone(&self.external_secrets);
            tokio::task::spawn_blocking(move || resolver.resolve(&external, &skip))
                .await
                .map_err(|e| AppError::Shell(format!("reading secrets failed: {e}")))?
                .map_err(AppError::Shell)?;
        }
        self.with_session(|s| {
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            Ok((s.workspace.snapshot(path, input)?, s.id.clone()))
        })
    }

    /// Wave 8, strand B (`grpc.rs`): `f` on the open workspace with `input` as a send gets it
    /// (session variables; secret values from the keychain and external secrets, read first
    /// on a blocking thread), and the collection id.
    pub async fn with_send_input<T>(
        &self,
        input: SendInput,
        f: impl FnOnce(&Workspace, SendInput, &str) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let pending = self.with_session(|s| self.external_pending(s, &input))?;
        if let Some((external, skip)) = pending {
            let resolver = std::sync::Arc::clone(&self.external_secrets);
            tokio::task::spawn_blocking(move || resolver.resolve(&external, &skip))
                .await
                .map_err(|e| AppError::Shell(format!("reading secrets failed: {e}")))?
                .map_err(AppError::Shell)?;
        }
        self.with_session(|s| {
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            f(&s.workspace, input, &s.id)
        })
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

    /// What the AI assistant sends for `task` (wave 8, strand E, `ai.rs`): the masked context
    /// (unsaved edits and kept responses included), every secret value in scope for the
    /// masking (keychain, cached external secrets, session variables, as for a send) and the
    /// collection id. No value leaves here except for masking.
    pub fn ai_context(
        &self,
        task: &rt_app::ai::AiTask,
        input: SendInput,
    ) -> Result<(rt_app::ai::AiContext, Vec<String>, String), AppError> {
        self.with_session(|s| {
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            let context =
                rt_app::ai::AiContext::new(&s.workspace, &lock(&self.responses), task, &input)?;
            let secrets = rt_app::ai::secrets_in_scope(s.workspace.collection(), &input);
            Ok((context, secrets, s.id.clone()))
        })
    }

    /// Wave 16: `f` on the active workspace and the kept responses with the input of a send
    /// (session variables, keychain secrets: only for masking) and the collection id. The
    /// AI calls build their context in `f`; no value leaves here except for masking.
    pub fn with_ai_scope<T>(
        &self,
        input: SendInput,
        f: impl FnOnce(&Workspace, &ResponseStore, &SendInput, &str) -> T,
    ) -> Result<T, AppError> {
        self.with_session(|s| {
            let input = self.services.session.input_for(&s.id, input);
            let input = self.with_secrets(s, input)?;
            Ok(f(&s.workspace, &lock(&self.responses), &input, &s.id))
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

    /// `f` on the open workspace and the kept responses (the OpenAPI response check,
    /// `openapi.rs`). Locks the session, then the responses, as `ai_context` does.
    pub fn with_responses<T>(
        &self,
        f: impl FnOnce(&Workspace, &ResponseStore) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.with_session(|s| f(&s.workspace, &lock(&self.responses)))
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

/// What the recent list keeps for a root: the collection's name if the root is one, else the
/// folder's name.
fn root_info(found: &Found, session: Option<&Session>) -> CollectionInfo {
    let is_collection = found.collections.len() == 1 && found.collections[0].dir.is_empty();
    let name = match (is_collection, session) {
        (true, Some(s)) => s.name().unwrap_or_else(|| found.name.clone()),
        (true, None) => found.collections[0].name.clone(),
        (false, _) => found.name.clone(),
    };
    CollectionInfo {
        id: found.id.clone(),
        name,
        location: found.id.clone(),
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
