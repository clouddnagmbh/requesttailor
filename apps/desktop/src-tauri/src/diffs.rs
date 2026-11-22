// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic diffs (`DiffApi`): the saved files, a commit or two revisions, read with `rt-git` (`Repo::tree_at`) and compared with `rt_app::diffs::diff`.
//!
//! Wave 12, strand A. The functions take a [`Core`] so they can be tested without a window;
//! the Tauri commands run them on a blocking thread. The saved files are read from the
//! collection folder (unsaved edits in the editor are not part of the diff); a repository
//! without a commit yet counts as an empty collection on the old side.

use rt_app::diffs::CollectionDiff;
use rt_app::{AppError, SecretStore};
use rt_format::FileSet;
use rt_git::{Options, Repo};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Repo::require(&root, &Options::default())
}

fn files(repo: &Repo, rev: &str) -> Result<FileSet> {
    Ok(repo.tree_at(rev)?.into_file_set())
}

/// `rev`'s parent; the empty collection for a root commit.
fn parent_files(repo: &Repo, rev: &str) -> Result<FileSet> {
    match repo.rev_parse(&format!("{rev}^")) {
        Ok(_) => files(repo, &format!("{rev}^")),
        Err(_) => Ok(FileSet::default()),
    }
}

/// The saved files against `HEAD`.
pub fn working<S: SecretStore>(core: &Core<S>) -> Result<CollectionDiff> {
    let repo = repo(core)?;
    let old = match repo.rev_parse("HEAD") {
        Ok(_) => files(&repo, "HEAD")?,
        Err(_) => FileSet::default(),
    };
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    let new = FileSet::from_dir(&root)
        .map_err(|e| AppError::Shell(format!("cannot read the collection: {e}")))?;
    rt_app::diffs::diff(&old, &new)
}

/// A commit against its first parent.
pub fn commit<S: SecretStore>(core: &Core<S>, rev: &str) -> Result<CollectionDiff> {
    let repo = repo(core)?;
    rt_app::diffs::diff(&parent_files(&repo, rev)?, &files(&repo, rev)?)
}

/// `from` → `to`.
pub fn between<S: SecretStore>(core: &Core<S>, from: &str, to: &str) -> Result<CollectionDiff> {
    let repo = repo(core)?;
    rt_app::diffs::diff(&files(&repo, from)?, &files(&repo, to)?)
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("the diff failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn diff_working(app: tauri::AppHandle) -> Result<CollectionDiff, CommandError> {
    blocking(app, working).await
}

#[tauri::command]
pub async fn diff_commit(
    app: tauri::AppHandle,
    rev: String,
) -> Result<CollectionDiff, CommandError> {
    blocking(app, move |core| commit(core, &rev)).await
}

#[tauri::command]
pub async fn diff_between(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<CollectionDiff, CommandError> {
    blocking(app, move |core| between(core, &from, &to)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use rt_app::diffs::{ChangeKind, ChangeTarget};
    use rt_app::{AppError, SecretKey, SecretStore};

    use super::{between, commit, working};
    use crate::core::Core;

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn working_commit_and_between() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-diffs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = TempDir(rt_git::canonical(&dir).unwrap());
        std::fs::write(
            dir.0.join("opencollection.yml"),
            "opencollection: \"1.0.0\"\ninfo:\n  name: Diffs\n",
        )
        .unwrap();
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&dir.0).unwrap();
        crate::git::init(&core).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.0.join(".git/config"))
            .map(|mut f| {
                use std::io::Write as _;
                f.write_all(b"[user]\n\tname = Ada\n\temail = ada@example.com\n")
            })
            .unwrap()
            .unwrap();

        // Before the first commit everything is new.
        let ping = core.create_request("", "Ping").unwrap();
        core.save().unwrap();
        let first = working(&core).unwrap();
        assert!(first.changes.iter().all(|c| c.change == ChangeKind::Added));
        let all: Vec<String> = crate::git::status(&core)
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        crate::git::commit(&core, "one", &all).unwrap();
        assert!(working(&core).unwrap().is_empty());

        // A saved edit shows against HEAD, an unsaved one does not.
        let mut spec = core.get_request(&ping).unwrap();
        spec.url = "https://example.com/changed".into();
        core.update_request(&ping, spec).unwrap();
        assert!(
            working(&core).unwrap().is_empty(),
            "unsaved edits are not on disk"
        );
        core.save().unwrap();
        let d = working(&core).unwrap();
        assert_eq!(d.changes.len(), 1);
        assert_eq!(
            d.changes[0].target,
            ChangeTarget::Request { path: ping.clone() }
        );
        assert_eq!(d.changes[0].fields[0].path, ["url"]);
        crate::git::commit(&core, "two", std::slice::from_ref(&ping)).unwrap();

        // The commit against its parent equals the range.
        assert_eq!(
            commit(&core, "HEAD").unwrap(),
            between(&core, "HEAD~1", "HEAD").unwrap()
        );
        let root = commit(&core, "HEAD~1").unwrap();
        assert!(root.changes.iter().all(|c| c.change == ChangeKind::Added));
        assert!(commit(&core, "nope").is_err());
    }
}
