// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The commit graph of the active collection's repository (`GitApi.graph`, `refs`, `changes`,
//! `fileDiff`; `rt_app::git::graph`), natively from `rt-git/src/graph.rs` (wave 14, strand F).
//!
//! The whole repository, not only the collection: paths are repository-relative with a
//! leading `/`. `changes` names, for every file, the collection of the set it belongs to
//! (`GitChangedFile::collection`), so the UI shows the semantic diff for it; every collection
//! of the set inside this repository counts, not only the active one.

use std::path::Path;

use rt_app::git::GitDiff;
use rt_app::git::graph::{
    CollectionAt, GitChangedFile, GitGraphPage, GitGraphQuery, GitRef, assign_collections,
};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Ok(Repo::require(&root, &Options::default())?)
}

/// The set's collections inside `workdir`, by their folder relative to it.
pub(crate) fn collections_in<S: SecretStore>(core: &Core<S>, workdir: &Path) -> Vec<CollectionAt> {
    core.collection_set()
        .collections
        .iter()
        .filter_map(|entry| {
            let path = core.path_of(&entry.id).ok()?;
            let path = rt_git::canonical(&path).unwrap_or(path);
            let rel = path.strip_prefix(workdir).ok()?;
            let dir = rel
                .components()
                .map(|c| c.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()?
                .join("/");
            Some(CollectionAt {
                id: entry.id.clone(),
                dir,
            })
        })
        .collect()
}

pub fn graph<S: SecretStore>(core: &Core<S>, query: &GitGraphQuery) -> Result<GitGraphPage> {
    Ok(repo(core)?.commit_graph(query)?)
}

pub fn refs<S: SecretStore>(core: &Core<S>) -> Result<Vec<GitRef>> {
    Ok(repo(core)?.refs()?)
}

pub fn changes<S: SecretStore>(
    core: &Core<S>,
    from: Option<&str>,
    to: &str,
) -> Result<Vec<GitChangedFile>> {
    let repo = repo(core)?;
    let mut files = repo.changes(from, to)?;
    assign_collections(&mut files, &collections_in(core, repo.workdir()));
    Ok(files)
}

pub fn file_diff<S: SecretStore>(
    core: &Core<S>,
    from: Option<&str>,
    to: Option<&str>,
    path: &str,
) -> Result<GitDiff> {
    Ok(repo(core)?.file_diff(from, to, path)?)
}

/// Runs `f` on a blocking thread (Git reads many objects).
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
pub async fn git_graph(
    app: tauri::AppHandle,
    query: GitGraphQuery,
) -> Result<GitGraphPage, CommandError> {
    blocking(app, move |core| graph(core, &query)).await
}

#[tauri::command]
pub async fn git_refs(app: tauri::AppHandle) -> Result<Vec<GitRef>, CommandError> {
    blocking(app, refs).await
}

#[tauri::command]
pub async fn git_changes(
    app: tauri::AppHandle,
    from: Option<String>,
    to: String,
) -> Result<Vec<GitChangedFile>, CommandError> {
    blocking(app, move |core| changes(core, from.as_deref(), &to)).await
}

#[tauri::command]
pub async fn git_file_diff(
    app: tauri::AppHandle,
    from: Option<String>,
    to: Option<String>,
    path: String,
) -> Result<GitDiff, CommandError> {
    blocking(app, move |core| {
        file_diff(core, from.as_deref(), to.as_deref(), &path)
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use rt_app::SecretKey;
    use rt_app::git::GitFileChange;
    use rt_app::git::graph::GitRefKind;

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

    const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: {name}\n";
    const REQUEST: &str = "info:\n  name: Get\n  type: http\nhttp:\n  method: GET\n  url: {url}\n";

    /// A repository with two collections (`billing/`, `users/`) and a README, two commits.
    #[test]
    fn graph_and_changes_of_the_whole_repository_name_every_collection() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-graph-{}", std::process::id()));
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
                &COLLECTION.replace("{name}", name),
            );
            write(
                &work,
                &format!("{name}/get.yml"),
                &REQUEST.replace("{url}", "/v1"),
            );
        }
        write(&work, "README.md", "# APIs\n");
        let git = Repo::require(&work, &isolated).unwrap();
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
        let first = git.commit("Start", &paths, None).unwrap();
        write(&work, "billing/get.yml", &REQUEST.replace("{url}", "/v2"));
        write(&work, "users/get.yml", &REQUEST.replace("{url}", "/v2"));
        write(&work, "README.md", "# APIs\n\nTwo of them.\n");
        let second = git.commit("Version 2", &paths, None).unwrap();

        let data =
            std::env::temp_dir().join(format!("rt-desktop-graph-data-{}", std::process::id()));
        let _data = Temp(data.clone());
        let core = Core::with_data_dir(Memory::default(), Some(data)).unwrap();
        let set = core.add_root(&work).unwrap();
        assert_eq!(set.collections.len(), 2, "{set:?}");
        let id = |name: &str| {
            set.collections
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .id
                .clone()
        };
        let (billing, users) = (id("billing"), id("users"));
        core.activate(&billing).unwrap();

        let page = graph(
            &core,
            &GitGraphQuery {
                skip: 0,
                limit: 10,
                refs: None,
            },
        )
        .unwrap();
        let ids: Vec<&str> = page.commits.iter().map(|c| c.commit.oid.as_str()).collect();
        assert_eq!(ids, vec![second.oid.as_str(), first.oid.as_str()]);
        assert_eq!(page.commits[0].refs, vec!["refs/heads/main".to_owned()]);
        let refs = refs(&core).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!((refs[0].kind, refs[0].current), (GitRefKind::Local, true));

        // Both collections of the repository are named, whichever is active; the README not.
        let files = changes(&core, None, &second.oid).unwrap();
        let named: Vec<(&str, GitFileChange, Option<&str>)> = files
            .iter()
            .map(|f| (f.path.as_str(), f.change, f.collection.as_deref()))
            .collect();
        assert_eq!(
            named,
            vec![
                ("/README.md", GitFileChange::Modified, None),
                (
                    "/billing/get.yml",
                    GitFileChange::Modified,
                    Some(billing.as_str())
                ),
                (
                    "/users/get.yml",
                    GitFileChange::Modified,
                    Some(users.as_str())
                ),
            ]
        );
        core.activate(&users).unwrap();
        assert_eq!(changes(&core, None, &second.oid).unwrap(), files);

        let readme = file_diff(&core, None, Some(&second.oid), "/README.md").unwrap();
        assert_eq!(readme.path, "/README.md");
        assert_eq!(readme.hunks.len(), 1);
        let work_tree = file_diff(&core, None, None, "/README.md").unwrap();
        assert!(work_tree.hunks.is_empty(), "nothing changed since");
    }
}
