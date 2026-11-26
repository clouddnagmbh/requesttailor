// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The Git wizards' backend (`GitApi.plan`, `apply`, `undo`, remotes, stashes;
//! `rt_app::git::actions`). Wave 14, strand G: natively in `rt-git` (gitoxide) on the
//! repository of the open collection, backups under `refs/requesttailor/backup/`, journal in
//! `.git/requesttailor/actions.jsonl`.
//!
//! The shell's part: it knows whether the app has unsaved edits (plans then block what
//! changes files, `unsaved-changes`), passes the keychain credentials and the app's own
//! identity, and after an action or undo with `reload: true` reopens every open collection
//! of that repository ([`crate::git::reload_repository`]).

use rt_app::git::GitPullResult;
use rt_app::git::actions::{
    GitAction, GitActionResult, GitPlan, GitRemote, GitStash, GitUndoEntry,
};
use rt_app::{AppError, SecretStore};
use rt_git::{ActionOptions, Repo, StoreCredentials};

use crate::commands::CommandError;
use crate::core::Core;
use crate::git::{app_identity, blocking, reload_repository, repo};

type Result<T, E = AppError> = std::result::Result<T, E>;

pub fn remotes<S: SecretStore>(core: &Core<S>) -> Result<Vec<GitRemote>> {
    Ok(repo(core)?.remotes()?)
}

pub fn stashes<S: SecretStore>(core: &Core<S>) -> Result<Vec<GitStash>> {
    Ok(repo(core)?.stashes()?)
}

pub fn plan<S: SecretStore>(core: &Core<S>, action: &GitAction) -> Result<GitPlan> {
    Ok(repo(core)?.plan(action, core.has_unsaved_changes())?)
}

fn reload_after<S: SecretStore>(core: &Core<S>, repo: &Repo, r: &GitActionResult) -> Result<()> {
    if !r.reload {
        return Ok(());
    }
    let conflicts = matches!(r.outcome, Some(GitPullResult::Conflicts { .. }));
    reload_repository(core, repo.workdir(), conflicts)
}

pub fn apply<S: SecretStore>(core: &Core<S>, action: &GitAction) -> Result<GitActionResult> {
    let repo = repo(core)?;
    let identity = app_identity(core)?;
    let credentials = StoreCredentials(core.secrets());
    let result = repo.apply(
        action,
        &ActionOptions {
            unsaved: core.has_unsaved_changes(),
            credentials: &credentials,
            identity: identity.as_ref(),
        },
    )?;
    reload_after(core, &repo, &result)?;
    Ok(result)
}

/// The newest undoable action; `None` without an open collection or outside a repository
/// (the Git panel asks on every refresh).
pub fn last_action<S: SecretStore>(core: &Core<S>) -> Result<Option<GitUndoEntry>> {
    let Some(root) = core.root() else {
        return Ok(None);
    };
    match Repo::open(&root, &rt_git::Options::default())? {
        Some(repo) => Ok(repo.last_action()?),
        None => Ok(None),
    }
}

pub fn undo<S: SecretStore>(core: &Core<S>, id: &str) -> Result<GitActionResult> {
    let repo = repo(core)?;
    let result = repo.undo(id, core.has_unsaved_changes())?;
    reload_after(core, &repo, &result)?;
    Ok(result)
}

#[tauri::command]
pub async fn git_remotes(app: tauri::AppHandle) -> Result<Vec<GitRemote>, CommandError> {
    blocking(app, remotes).await
}

#[tauri::command]
pub async fn git_stashes(app: tauri::AppHandle) -> Result<Vec<GitStash>, CommandError> {
    blocking(app, stashes).await
}

#[tauri::command]
pub async fn git_plan(app: tauri::AppHandle, action: GitAction) -> Result<GitPlan, CommandError> {
    blocking(app, move |core| plan(core, &action)).await
}

#[tauri::command]
pub async fn git_apply(
    app: tauri::AppHandle,
    action: GitAction,
) -> Result<GitActionResult, CommandError> {
    blocking(app, move |core| apply(core, &action)).await
}

#[tauri::command]
pub async fn git_last_action(app: tauri::AppHandle) -> Result<Option<GitUndoEntry>, CommandError> {
    blocking(app, last_action).await
}

#[tauri::command]
pub async fn git_undo(app: tauri::AppHandle, id: String) -> Result<GitActionResult, CommandError> {
    blocking(app, move |core| undo(core, &id)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rt_app::git::actions::GitBlockReason;
    use rt_app::{SecretKey, TreeNode};

    use super::*;

    #[derive(Default)]
    struct Secrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for Secrets {
        fn get(&self, key: &SecretKey) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(key.clone(), value.into());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn requests(node: &TreeNode, out: &mut Vec<String>) {
        match node {
            TreeNode::Request { path, .. } => out.push(path.clone()),
            TreeNode::Folder { children, .. } => {
                for c in children {
                    requests(c, out);
                }
            }
        }
    }

    fn open_requests(core: &Core<Secrets>) -> Vec<String> {
        let mut out = Vec::new();
        requests(&core.tree().unwrap(), &mut out);
        out.sort();
        out
    }

    fn act(json: &str) -> GitAction {
        serde_json::from_str(json).unwrap()
    }

    const HEALTH: &str = "info:\n  name: Health\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://api.test/health\n";
    const USERS: &str = "info:\n  name: Users\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: https://api.test/users\n";

    #[test]
    fn plan_apply_undo_through_the_shell_reloads_the_collection() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-wizard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _cleanup = Dir(dir.clone());
        let dir = rt_git::canonical(&dir).unwrap();
        write(
            &dir,
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: Wizard\n",
        );
        write(&dir, "health.yml", HEALTH);
        rt_git::init(&dir, &rt_git::Options { isolated: true }).unwrap();
        let mut cfg = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(".git/config"))
            .unwrap();
        std::io::Write::write_all(
            &mut cfg,
            b"[user]\n\tname = Ada\n\temail = ada@example.com\n",
        )
        .unwrap();
        let core = Core::new(Secrets::default()).unwrap();
        core.open(&dir).unwrap();
        crate::git::commit(
            &core,
            "init",
            &["opencollection.yml".into(), "health.yml".into()],
        )
        .unwrap();
        assert!(last_action(&core).unwrap().is_none());

        // A new request, parked: the open collection no longer shows it; undo brings it back.
        write(&dir, "users.yml", USERS);
        core.reload().unwrap();
        assert_eq!(open_requests(&core), ["health.yml", "users.yml"]);
        let p = plan(&core, &act(r#"{"action":"stash","message":"wip"}"#)).unwrap();
        assert!(p.blocked.is_none(), "{p:?}");
        let r = apply(&core, &p.action).unwrap();
        assert!(r.reload);
        assert_eq!(open_requests(&core), ["health.yml"]);
        assert_eq!(stashes(&core).unwrap()[0].message, "wip");
        let last = last_action(&core).unwrap().unwrap();
        assert_eq!(Some(last.id.clone()), r.undo.map(|u| u.id));
        undo(&core, &last.id).unwrap();
        assert_eq!(open_requests(&core), ["health.yml", "users.yml"]);
        assert!(stashes(&core).unwrap().is_empty());
        assert!(remotes(&core).unwrap().is_empty());

        // Unsaved edits block what changes files.
        let mut edited = core.get_request("health.yml").unwrap();
        edited.url = "https://api.test/edited".into();
        core.update_request("health.yml", edited).unwrap();
        let p = plan(&core, &act(r#"{"action":"stash"}"#)).unwrap();
        assert_eq!(
            p.blocked.map(|b| b.reason),
            Some(GitBlockReason::UnsavedChanges)
        );
        assert!(apply(&core, &act(r#"{"action":"stash"}"#)).is_err());
    }
}
