// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Blame, history search and reflog of the active repository (`GitApi.blame`, `searchHistory`,
//! `reflog`; `rt_app::git::history`), natively from `rt-git` (wave 15, strand C). Paths are
//! repository-relative with a leading `/`. Git reads many objects: every command runs on a
//! blocking thread.

use rt_app::git::history::{GitBlame, GitHistoryPage, GitHistoryQuery, GitReflogEntry};
use rt_app::{AppError, SecretStore};

use crate::commands::CommandError;
use crate::core::Core;
use crate::git::{blocking, repo};

type Result<T, E = AppError> = std::result::Result<T, E>;

pub fn blame<S: SecretStore>(core: &Core<S>, rev: &str, path: &str) -> Result<GitBlame> {
    Ok(repo(core)?.blame(rev, path)?)
}

pub fn search_history<S: SecretStore>(
    core: &Core<S>,
    query: &GitHistoryQuery,
) -> Result<GitHistoryPage> {
    Ok(repo(core)?.search_history(query)?)
}

pub fn reflog<S: SecretStore>(
    core: &Core<S>,
    name: &str,
    limit: u32,
) -> Result<Vec<GitReflogEntry>> {
    Ok(repo(core)?.reflog(name, limit)?)
}

#[tauri::command]
pub async fn git_blame(
    app: tauri::AppHandle,
    rev: String,
    path: String,
) -> Result<GitBlame, CommandError> {
    blocking(app, move |core| blame(core, &rev, &path)).await
}

#[tauri::command]
pub async fn git_search_history(
    app: tauri::AppHandle,
    query: GitHistoryQuery,
) -> Result<GitHistoryPage, CommandError> {
    blocking(app, move |core| search_history(core, &query)).await
}

#[tauri::command]
pub async fn git_reflog(
    app: tauri::AppHandle,
    name: String,
    limit: u32,
) -> Result<Vec<GitReflogEntry>, CommandError> {
    blocking(app, move |core| reflog(core, &name, limit)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rt_app::git::history::GitHistoryField;
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

    #[test]
    fn blame_search_and_reflog_through_the_shell() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _cleanup = Dir(dir.clone());
        let dir = rt_git::canonical(&dir).unwrap();
        write(
            &dir,
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: History\n",
        );
        write(&dir, "notes.txt", "one\ntwo\n");
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
        let first = crate::git::commit(
            &core,
            "init",
            &["opencollection.yml".into(), "notes.txt".into()],
        )
        .unwrap();
        write(&dir, "notes.txt", "one\nthree\n");
        let second =
            crate::git::commit(&core, "three instead of two", &["notes.txt".into()]).unwrap();

        let blame = blame(&core, "HEAD", "/notes.txt").unwrap();
        assert_eq!(blame.lines, 2);
        assert_eq!(blame.hunks[0].commit.oid, first.oid);
        assert_eq!(blame.hunks[1].commit.oid, second.oid);

        let page = search_history(
            &core,
            &GitHistoryQuery {
                content: Some("two".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        // "two" was written in the first commit and replaced in the second.
        assert_eq!(page.hits.len(), 2);
        assert_eq!(page.hits[0].matched, vec![GitHistoryField::Content]);
        assert_eq!(page.hits[0].paths, vec!["/notes.txt"]);

        let log = reflog(&core, "HEAD", 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].new, second.oid);
    }
}
