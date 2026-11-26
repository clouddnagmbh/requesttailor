// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The working copy, file tree and file contents of the active repository (`GitApi.workingCopy`,
//! `fileTree`, `blob`; `rt_app::git::files`), natively from `rt-git` (wave 15, strand B).
//!
//! The whole repository, not only the active collection: paths are repository-relative with a
//! leading `/`. Every collection of the set inside the repository is named on its files
//! (`GitWorkingFile::collection`), so the UI shows the semantic diff for those.

use rt_app::git::files::{GitBlob, GitTreeEntry, GitWorkingCopy};
use rt_app::git::graph::collection_of;
use rt_app::{AppError, SecretStore};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;
use crate::git::repo;
use crate::git_graph::collections_in;

type Result<T, E = AppError> = std::result::Result<T, E>;

pub fn working_copy<S: SecretStore>(core: &Core<S>) -> Result<GitWorkingCopy> {
    let repo = repo(core)?;
    let mut copy = repo.working_copy()?;
    let collections = collections_in(core, repo.workdir());
    for file in &mut copy.files {
        file.collection = collection_of(&file.path, &collections).map(str::to_owned);
    }
    Ok(copy)
}

pub fn file_tree<S: SecretStore>(
    core: &Core<S>,
    rev: Option<&str>,
    dir: &str,
) -> Result<Vec<GitTreeEntry>> {
    Ok(repo(core)?.file_tree(rev, dir)?)
}

pub fn blob<S: SecretStore>(core: &Core<S>, rev: Option<&str>, path: &str) -> Result<GitBlob> {
    Ok(repo(core)?.blob(rev, path)?)
}

/// Runs `f` on a blocking thread (the status reads every file's metadata).
async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("Git failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn git_working_copy(app: tauri::AppHandle) -> Result<GitWorkingCopy, CommandError> {
    blocking(app, working_copy).await
}

#[tauri::command]
pub async fn git_file_tree(
    app: tauri::AppHandle,
    rev: Option<String>,
    dir: String,
) -> Result<Vec<GitTreeEntry>, CommandError> {
    blocking(app, move |core| file_tree(core, rev.as_deref(), &dir)).await
}

#[tauri::command]
pub async fn git_blob(
    app: tauri::AppHandle,
    rev: Option<String>,
    path: String,
) -> Result<GitBlob, CommandError> {
    blocking(app, move |core| blob(core, rev.as_deref(), &path)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rt_app::SecretKey;
    use rt_app::git::GitFileState;
    use rt_git::{Options, Repo};

    use super::*;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for Memory {
        fn get(&self, key: &SecretKey) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct Temp(PathBuf);

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Two collections and a README in one repository: the working copy covers all of it and
    /// names the collection of each collection file.
    #[test]
    fn the_working_copy_covers_the_repository_and_names_collections() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = Temp(rt_git::canonical(&dir).unwrap());
        let work = tmp.0.clone();
        let isolated = Options { isolated: true };
        rt_git::init(&work, &isolated).unwrap();
        let mut config = std::fs::read_to_string(work.join(".git/config")).unwrap();
        config.push_str("[user]\n\tname = Ada\n\temail = ada@example.com\n");
        std::fs::write(work.join(".git/config"), config).unwrap();
        for name in ["billing", "users"] {
            write(
                &work,
                &format!("{name}/opencollection.yml"),
                &format!("opencollection: 1.0.0\ninfo:\n  name: {name}\n"),
            );
            write(
                &work,
                &format!("{name}/get.yml"),
                "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: /v1\n",
            );
        }
        write(&work, "README.md", "# APIs\n");
        let paths: Vec<String> = [
            "README.md",
            "billing/opencollection.yml",
            "billing/get.yml",
            "users/opencollection.yml",
            "users/get.yml",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        Repo::require(&work, &isolated)
            .unwrap()
            .commit("Start", &paths, None)
            .unwrap();
        write(
            &work,
            "billing/get.yml",
            "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: /v2\n",
        );
        write(&work, "README.md", "# APIs\n\nTwo.\n");
        write(&work, "notes/todo.md", "- x\n");

        let data =
            std::env::temp_dir().join(format!("rt-desktop-files-data-{}", std::process::id()));
        let _data = Temp(data.clone());
        let core = Core::with_data_dir(Memory::default(), Some(data)).unwrap();
        let set = core.add_root(&work).unwrap();
        let billing = set
            .collections
            .iter()
            .find(|c| c.name == "billing")
            .unwrap()
            .id
            .clone();
        core.activate(
            &set.collections
                .iter()
                .find(|c| c.name == "users")
                .unwrap()
                .id,
        )
        .unwrap();

        let copy = working_copy(&core).unwrap();
        let rows: Vec<(&str, GitFileState, Option<&str>)> = copy
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.state, f.collection.as_deref()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("/README.md", GitFileState::Modified, None),
                (
                    "/billing/get.yml",
                    GitFileState::Modified,
                    Some(billing.as_str())
                ),
                ("/notes/todo.md", GitFileState::Untracked, None),
            ]
        );
        let tree = file_tree(&core, None, "/").unwrap();
        assert!(tree.iter().any(|e| e.name == "billing"));
        assert_eq!(
            blob(&core, Some("HEAD"), "/README.md")
                .unwrap()
                .text
                .as_deref(),
            Some("# APIs\n")
        );
    }
}
