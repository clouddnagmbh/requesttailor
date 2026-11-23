// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Commit-level building blocks of the active repository (`GitApi.commitInfo`, `pick`,
//! `writeTree`, `writeCommit`) and writing a resolved text conflict (`resolveText`);
//! `rt_app::git::plumbing`. Wave 15, strand D: natively in `rt-git`.
//!
//! The building blocks move no ref and touch no file. `resolveText` refuses while the app has
//! unsaved changes (like every resolve) and reopens the collections of the repository
//! afterwards, unless other files still carry conflict markers.

use rt_app::git::GitFileState;
use rt_app::git::plumbing::{
    GitCommitDetails, GitCommitWrite, GitPickRequest, GitPickResult, GitTreeWrite,
};
use rt_app::{AppError, SecretStore};

use crate::commands::CommandError;
use crate::core::Core;
use crate::git::{app_identity, blocking, repo};

type Result<T, E = AppError> = std::result::Result<T, E>;

pub fn commit_info<S: SecretStore>(core: &Core<S>, rev: &str) -> Result<GitCommitDetails> {
    repo(core)?.commit_info(rev)
}

pub fn pick<S: SecretStore>(core: &Core<S>, request: &GitPickRequest) -> Result<GitPickResult> {
    repo(core)?.pick(request)
}

pub fn write_tree<S: SecretStore>(core: &Core<S>, write: &GitTreeWrite) -> Result<String> {
    repo(core)?.write_tree(write)
}

pub fn write_commit<S: SecretStore>(core: &Core<S>, write: &GitCommitWrite) -> Result<String> {
    let identity = app_identity(core)?;
    repo(core)?.write_commit(write, identity.as_ref())
}

pub fn resolve_text<S: SecretStore>(core: &Core<S>, path: &str, text: &str) -> Result<()> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(
            "save or discard your changes before you resolve a conflict".into(),
        ));
    }
    let repo = repo(core)?;
    repo.resolve_text(path, text)?;
    let remaining = repo
        .status()?
        .files
        .iter()
        .any(|f| f.state == GitFileState::Conflicted);
    match core.reload() {
        Ok(_) => Ok(()),
        // Other files still carry conflict markers.
        Err(_) if remaining => Ok(()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn git_commit_info(
    app: tauri::AppHandle,
    rev: String,
) -> Result<GitCommitDetails, CommandError> {
    blocking(app, move |core| commit_info(core, &rev)).await
}

#[tauri::command]
pub async fn git_pick(
    app: tauri::AppHandle,
    request: GitPickRequest,
) -> Result<GitPickResult, CommandError> {
    blocking(app, move |core| pick(core, &request)).await
}

#[tauri::command]
pub async fn git_write_tree(
    app: tauri::AppHandle,
    write: GitTreeWrite,
) -> Result<String, CommandError> {
    blocking(app, move |core| write_tree(core, &write)).await
}

#[tauri::command]
pub async fn git_write_commit(
    app: tauri::AppHandle,
    write: GitCommitWrite,
) -> Result<String, CommandError> {
    blocking(app, move |core| write_commit(core, &write)).await
}

#[tauri::command]
pub async fn git_resolve_text(
    app: tauri::AppHandle,
    path: String,
    text: String,
) -> Result<(), CommandError> {
    blocking(app, move |core| resolve_text(core, &path, &text)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rt_app::git::actions::GitAction;
    use rt_app::git::plumbing::{GitTreeContent, GitTreeFile};
    use rt_app::{SecretKey, SecretStore};

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

    fn act(json: &str) -> GitAction {
        serde_json::from_str(json).unwrap()
    }

    const HEALTH: &str = "info:\n  name: Health\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://api.test/health\n";
    const NOTES: &str = "one\ntwo\nthree\n";

    #[test]
    fn building_blocks_and_a_text_conflict_through_the_shell() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-plumbing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _cleanup = Dir(dir.clone());
        let dir = rt_git::canonical(&dir).unwrap();
        write(
            &dir,
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: Plumbing\n",
        );
        write(&dir, "health.yml", HEALTH);
        write(&dir, "notes.txt", NOTES);
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
        let all = [
            "opencollection.yml".to_owned(),
            "health.yml".to_owned(),
            "notes.txt".to_owned(),
        ];
        let first = crate::git::commit(&core, "init", &all).unwrap();

        // Building blocks: no ref moves.
        let info = commit_info(&core, "HEAD").unwrap();
        assert_eq!(info.oid, first.oid);
        let tree = write_tree(
            &core,
            &GitTreeWrite {
                base: "HEAD".into(),
                files: vec![GitTreeFile {
                    path: "/notes.txt".into(),
                    content: GitTreeContent::Text {
                        text: "one\nTWO\nthree\n".into(),
                    },
                    executable: None,
                }],
            },
        )
        .unwrap();
        let built = write_commit(
            &core,
            &GitCommitWrite {
                tree,
                parents: vec![first.oid.clone()],
                message: "built\n".into(),
                author: None,
                committer: None,
            },
        )
        .unwrap();
        assert_eq!(commit_info(&core, "HEAD").unwrap().oid, first.oid);
        assert_eq!(commit_info(&core, &built).unwrap().message, "built\n");

        // A text conflict: both sides change line two; resolve with the view's text.
        crate::git_actions::apply(
            &core,
            &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
        )
        .unwrap();
        write(&dir, "notes.txt", "one\ntheirs\nthree\n");
        crate::git::commit(&core, "theirs", &["notes.txt".into()]).unwrap();
        crate::git_actions::apply(&core, &act(r#"{"action":"switch","target":"main"}"#)).unwrap();
        write(&dir, "notes.txt", "one\nours\nthree\n");
        crate::git::commit(&core, "ours", &["notes.txt".into()]).unwrap();
        crate::git_actions::apply(&core, &act(r#"{"action":"merge","from":"other"}"#)).unwrap();
        let c = crate::git::conflict(&core, "notes.txt").unwrap();
        assert_eq!(c.theirs.as_deref(), Some("one\ntheirs\nthree\n"));

        let mut edited = core.get_request("health.yml").unwrap();
        edited.url = "https://api.test/edited".into();
        core.update_request("health.yml", edited).unwrap();
        assert!(resolve_text(&core, "notes.txt", "x\n").is_err(), "unsaved changes block it");
        core.reload().unwrap();

        resolve_text(&core, "notes.txt", "one\nboth\nthree\n").unwrap();
        let merge = crate::git::commit(&core, "Merge other", &[]).unwrap();
        assert_eq!(commit_info(&core, &merge.oid).unwrap().parents.len(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "one\nboth\nthree\n"
        );
    }
}
