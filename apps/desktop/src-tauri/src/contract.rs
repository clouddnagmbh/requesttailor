// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OpenAPI contract (`ContractApi`, `rt_app::contract`): the spec from the work tree or
//! `Repo::show`, check, spec changes between revisions, changelog.
//!
//! Wave 12, strand F. The functions take a [`Core`] and run the shared steps of
//! `rt_app::contract`; the Tauri commands below only move them to a blocking thread (Git and
//! file I/O). "Saved files" (`rev` = `None`) are the files on disk; a revision is read from
//! Git without a checkout.

use std::path::{Path, PathBuf};

use rt_app::SecretStore;
use rt_app::contract::{ContractReport, ContractSpec, ReadFile, Side, SpecChanges, safe_path};
use rt_app::{AppError, contract};
use rt_format::FileSet;
use rt_git::{Options, Repo};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

fn root<S: SecretStore>(core: &Core<S>) -> Result<PathBuf> {
    core.root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))
}

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    Repo::require(&root(core)?, &Options::default())
}

/// Reads a spec path from the work tree: relative to the collection, or with a leading `/`
/// relative to the repository root (the collection's folder if there is no repository).
pub(crate) fn read_disk(root: &Path, work: &Path, path: &str) -> Result<Option<Vec<u8>>> {
    safe_path(path)?;
    let file = match path.strip_prefix('/') {
        Some(rest) => work.join(rest),
        None => root.join(path),
    };
    match std::fs::read(&file) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) if file.is_dir() => {
            let _ = e;
            Ok(None)
        }
        Err(e) => Err(AppError::Shell(format!("cannot read {path}: {e}"))),
    }
}

pub(crate) fn work_dir(root: &Path) -> PathBuf {
    match Repo::open(root, &Options::default()) {
        Ok(Some(repo)) => repo.workdir().to_path_buf(),
        _ => root.to_path_buf(),
    }
}

/// The spec of the open collection's saved files, if there is one.
pub fn spec<S: SecretStore>(core: &Core<S>) -> Result<Option<ContractSpec>> {
    let root = root(core)?;
    let work = work_dir(&root);
    let files = FileSet::from_dir(&root).map_err(|e| AppError::Shell(e.to_string()))?;
    let read = |p: &str| read_disk(&root, &work, p);
    Ok(contract::load(&files, &read)?.map(|l| l.spec))
}

/// The collection against its spec, at `rev` (`None`: the saved files).
pub fn check<S: SecretStore>(core: &Core<S>, rev: Option<&str>) -> Result<ContractReport> {
    match rev {
        None => {
            let root = root(core)?;
            let work = work_dir(&root);
            let files = FileSet::from_dir(&root).map_err(|e| AppError::Shell(e.to_string()))?;
            contract::check(&files, &|p: &str| read_disk(&root, &work, p))
        }
        Some(rev) => {
            let repo = repo(core)?;
            let files = repo.tree_at(rev)?.into_file_set();
            contract::check(&files, &|p: &str| {
                Ok(repo.show(rev, p)?.map(String::into_bytes))
            })
        }
    }
}

fn with_sides<S: SecretStore, T>(
    core: &Core<S>,
    from: &str,
    to: &str,
    f: impl FnOnce(&Side<'_>, &Side<'_>) -> Result<T>,
) -> Result<T> {
    let repo = repo(core)?;
    let (old_files, new_files) = (
        repo.tree_at(from)?.into_file_set(),
        repo.tree_at(to)?.into_file_set(),
    );
    let read_old =
        |p: &str| -> Result<Option<Vec<u8>>> { Ok(repo.show(from, p)?.map(String::into_bytes)) };
    let read_new =
        |p: &str| -> Result<Option<Vec<u8>>> { Ok(repo.show(to, p)?.map(String::into_bytes)) };
    let old: ReadFile<'_> = &read_old;
    let new: ReadFile<'_> = &read_new;
    f(
        &Side {
            files: &old_files,
            read: old,
        },
        &Side {
            files: &new_files,
            read: new,
        },
    )
}

pub fn changes<S: SecretStore>(core: &Core<S>, from: &str, to: &str) -> Result<SpecChanges> {
    with_sides(core, from, to, contract::changes)
}

pub fn changelog<S: SecretStore>(core: &Core<S>, from: &str, to: &str) -> Result<String> {
    with_sides(core, from, to, |old, new| {
        contract::changelog(old, new, from, to)
    })
}

/// The facts of a range for writers of release notes (`rt_app::contract::ChangelogFacts`):
/// what `rt changelog` hands a writer. A desktop build adds no sections of its own.
pub fn changelog_facts<S: SecretStore>(
    core: &Core<S>,
    from: &str,
    to: &str,
) -> Result<contract::ChangelogFacts> {
    let commits = repo(core)?.changelog_commits(from, to)?;
    with_sides(core, from, to, |old, new| {
        contract::changelog_facts(old, new, from, to, &[], commits)
    })
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("the contract check failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn contract_spec(app: tauri::AppHandle) -> Result<Option<ContractSpec>, CommandError> {
    blocking(app, spec).await
}

#[tauri::command]
pub async fn contract_check(
    app: tauri::AppHandle,
    rev: Option<String>,
) -> Result<ContractReport, CommandError> {
    blocking(app, move |core| check(core, rev.as_deref())).await
}

#[tauri::command]
pub async fn contract_changes(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<SpecChanges, CommandError> {
    blocking(app, move |core| changes(core, &from, &to)).await
}

#[tauri::command]
pub async fn contract_changelog(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<String, CommandError> {
    blocking(app, move |core| changelog(core, &from, &to)).await
}

#[tauri::command]
pub async fn contract_changelog_facts(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<contract::ChangelogFacts, CommandError> {
    blocking(app, move |core| changelog_facts(core, &from, &to)).await
}

/// Wave 17: the spec the contract names for `workspace` (the file `opencollection.yml`
/// configures, unsaved edits included, else the first of the usual names that exists in the
/// collection folder, staged files included, or at the repository root), written with `text`,
/// unsaved. Returns its path.
pub(crate) fn stage_spec_in(
    root: &Path,
    workspace: &mut rt_app::Workspace,
    text: &str,
) -> Result<String> {
    let work = work_dir(root);
    let exists = |path: &str| -> Result<bool> {
        if workspace.staged_spec_path(path) {
            return Ok(true);
        }
        if !path.starts_with('/') && workspace.companion_file(path).is_some() {
            return Ok(true);
        }
        Ok(read_disk(root, &work, path)?.is_some())
    };
    let path = match contract::configured_spec(workspace)? {
        Some(configured) => {
            safe_path(&configured)?;
            if !exists(&configured)? {
                return Err(AppError::Format(format!(
                    "the spec {configured} named in opencollection.yml does not exist"
                )));
            }
            configured
        }
        None => {
            let mut found = None;
            for candidate in contract::SPEC_CANDIDATES {
                if exists(candidate)? {
                    found = Some((*candidate).to_owned());
                    break;
                }
            }
            found.ok_or_else(|| {
                AppError::Format(
                    "no OpenAPI spec found: add openapi.yaml next to the collection or at the \
                     repository root, or name it in opencollection.yml under \
                     extensions.requesttailor.contract.spec"
                        .into(),
                )
            })?
        }
    };
    contract::stage_spec(workspace, &path, text)
}

/// Wave 18 (interface, strand H): changes of a GraphQL or Protobuf schema between revisions.
#[tauri::command]
pub async fn contract_schema_changes(
    _app: tauri::AppHandle,
    kind: rt_app::contract::SchemaKind,
    from: String,
    to: String,
) -> Result<rt_app::contract::SpecChanges, CommandError> {
    let _ = (kind, from, to);
    Err(rt_app::AppError::Unsupported(
        "GraphQL and Protobuf contracts are not implemented yet (wave 18, strand H).".into(),
    )
    .into())
}

/// Wave 17: writes the spec of the collection (`ContractApi.stageSpec`), unsaved; `save`
/// writes it, the commit scans it.
#[tauri::command]
pub async fn contract_stage_spec(
    app: tauri::AppHandle,
    text: String,
) -> Result<String, CommandError> {
    // Parsing and the existence checks are quick; no blocking thread needed, but the file
    // reads are file I/O, so keep them off the async runtime like the other commands.
    blocking(app, move |core| core.stage_spec(&text)).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;

    use rt_app::{SecretKey, SecretStore};

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

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.test"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testdata/contract")
    }

    /// A repository with the fixture collection in `api/` and the spec at the repository root
    /// (`/specs/v1.yaml` configured), v1 then v3.
    #[test]
    fn spec_check_changes_and_changelog_over_a_repository() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = {
            std::fs::create_dir_all(&dir).unwrap();
            rt_git::canonical(&dir).unwrap()
        };
        let col = dir.join("api");
        FileSet::from_dir(&fixtures().join("collection"))
            .unwrap()
            .write_to_dir(&col)
            .unwrap();
        std::fs::write(
            col.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: Orders\nextensions:\n  requesttailor:\n    contract:\n      spec: /openapi.yaml\n",
        )
        .unwrap();
        git(&dir, &["init", "-q"]);
        std::fs::write(
            dir.join("openapi.yaml"),
            std::fs::read(fixtures().join("specs/v1.yaml")).unwrap(),
        )
        .unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "v1"]);
        std::fs::write(
            dir.join("openapi.yaml"),
            std::fs::read(fixtures().join("specs/v3.yaml")).unwrap(),
        )
        .unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "v3"]);

        let core = Core::new(Memory::default()).unwrap();
        core.open(&col).unwrap();

        let spec = spec(&core).unwrap().unwrap();
        assert_eq!(
            (spec.path.as_str(), spec.version.as_str(), spec.configured),
            ("/openapi.yaml", "2.0.0", true)
        );
        // Saved files: against v3 the delete request has no operation any more.
        let saved = check(&core, None).unwrap();
        assert!(
            saved
                .findings
                .iter()
                .any(|f| f.request == "orders/delete-order.yml"),
            "{saved:#?}"
        );
        // At the first commit everything fits.
        let old = check(&core, Some("HEAD~1")).unwrap();
        assert_eq!(old.findings, vec![], "{old:#?}");
        assert_eq!(old.covered, 5);

        let c = changes(&core, "HEAD~1", "HEAD").unwrap();
        assert_eq!(c.from_version.as_deref(), Some("1.0.0"));
        assert!(c.breaking().count() >= 5);
        let text = changelog(&core, "HEAD~1", "HEAD").unwrap();
        assert!(
            text.contains("## Breaking changes") && text.contains("orders/delete-order.yml"),
            "{text}"
        );
        // The facts carry the same changelog and the commits of the range.
        let facts = changelog_facts(&core, "HEAD~1", "HEAD").unwrap();
        assert_eq!(facts.markdown, text);
        assert_eq!(
            facts
                .commits
                .iter()
                .map(|c| c.summary.as_str())
                .collect::<Vec<_>>(),
            ["v3"]
        );
        assert!(facts.spec.breaking().count() >= 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
