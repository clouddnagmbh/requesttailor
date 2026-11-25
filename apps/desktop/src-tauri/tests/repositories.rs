// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 15, strand A: repositories in the collection set of the desktop core, without a window.
//! A folder without a collection in Git is a repository root; repositories keep sidebar order;
//! the active repository follows the sidebar choice and the active collection, and every `git_*`
//! command acts on it; clone and init add folders to the set; the sidebar's summary.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::git::repos::{GitCloneRequest, GitOperation};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_desktop::{Core, git, repositories};

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

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("rt-desktop-repos-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(dir: &Path, name: &str) {
    write(
        &dir.join("opencollection.yml"),
        &format!("opencollection: \"1.0.0\"\ninfo:\n  name: {name}\n"),
    );
}

/// `git init` with an identity, as the app's users have one.
fn repo(dir: &Path) {
    gix::init(dir).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(".git/config"))
        .unwrap();
    f.write_all(b"[user]\n\tname = Ada\n\temail = ada@example.com\n")
        .unwrap();
}

fn core() -> Core<MemorySecrets> {
    Core::new(MemorySecrets::default()).unwrap()
}

fn changed(core: &Core<MemorySecrets>) -> Vec<String> {
    git::status(core)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.path)
        .collect()
}

#[test]
fn a_folder_without_collection_in_git_opens_as_a_repository_root() {
    let notes = temp("notes");
    repo(&notes.0);
    write(&notes.0.join("README.md"), "# Notes\n");
    let core = core();

    let set = repositories::open(&core, &notes.0).unwrap();
    // No collection, no scaffold: the folder gets no opencollection.yml.
    assert!(set.collections.is_empty());
    assert_eq!(set.active, None);
    assert_eq!(set.roots.len(), 1);
    assert!(set.roots[0].collections.is_empty());
    let id = notes.0.to_string_lossy().into_owned();
    assert_eq!(set.repositories.len(), 1);
    assert_eq!(set.repositories[0].id, id);
    assert!(set.repositories[0].collections.is_empty());
    assert_eq!(set.active_repository.as_deref(), Some(id.as_str()));
    assert!(!notes.0.join("opencollection.yml").exists());

    // Git acts on it without any collection open.
    assert_eq!(changed(&core), ["README.md"]);
    assert_eq!(core.active_repository().unwrap(), notes.0);
}

#[test]
fn open_path_adds_a_checkout_without_a_picker_and_refuses_other_folders() {
    let first = temp("first");
    repo(&first.0);
    write(&first.0.join("a.txt"), "a\n");
    let core = core();
    repositories::open(&core, &first.0).unwrap();

    let second = temp("second");
    repo(&second.0);
    write(&second.0.join("b.txt"), "b\n");
    let entry = repositories::open_path(&core, &second.0).unwrap();
    let id = second.0.to_string_lossy().into_owned();
    assert_eq!(entry.id, id);
    let set = core.collection_set();
    assert_eq!(set.repositories.len(), 2);
    assert_eq!(set.active_repository.as_deref(), Some(id.as_str()));
    assert_eq!(changed(&core), ["b.txt"]);

    let plain = temp("plain-path");
    assert!(repositories::open_path(&core, &plain.0).is_err());
    assert!(repositories::open_path(&core, &plain.0.join("missing")).is_err());
}

#[test]
fn outside_git_a_folder_without_collection_is_not_a_repository_root() {
    // As before wave 15: no collection and no repository, nothing to add (a new collection is
    // made with New…, which scaffolds it).
    let plain = temp("plain");
    let core = core();
    assert!(repositories::open(&core, &plain.0).is_err());
    let set = core.collection_set();
    assert!(set.roots.is_empty() && set.repositories.is_empty());
    assert_eq!(set.active_repository, None);
}

#[test]
fn repositories_follow_the_sidebar_choice_and_the_active_collection() {
    let work = temp("work");
    repo(&work.0);
    collection(&work.0.join("api"), "API");
    write(&work.0.join("api/in-collection.txt"), "x");
    let notes = temp("notes2");
    repo(&notes.0);
    write(&notes.0.join("todo.md"), "- ship\n");

    let core = core();
    repositories::open(&core, &work.0).unwrap();
    let set = repositories::open(&core, &notes.0).unwrap();
    let work_id = work.0.to_string_lossy().into_owned();
    let notes_id = notes.0.to_string_lossy().into_owned();
    // Repositories that hold collections come first, then repository roots.
    let ids: Vec<_> = set.repositories.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, [work_id.clone(), notes_id.clone()]);
    assert_eq!(set.repositories[0].collections.len(), 1);
    // Opening the repository root chose it.
    assert_eq!(set.active_repository.as_deref(), Some(notes_id.as_str()));
    assert_eq!(changed(&core), ["todo.md"]);

    // Choosing the other repository moves Git there; the active collection stays.
    let entry = repositories::activate(&core, &work_id).unwrap();
    assert_eq!(entry.id, work_id);
    assert_eq!(
        core.collection_set().active_repository.as_deref(),
        Some(work_id.as_str())
    );
    let files = changed(&core);
    assert!(
        files.contains(&"api/opencollection.yml".to_owned()),
        "{files:?}"
    );
    assert!(!files.contains(&"todo.md".to_owned()));

    // A collection chosen in the sidebar brings Git back to its repository.
    repositories::activate(&core, &notes_id).unwrap();
    assert_eq!(changed(&core), ["todo.md"]);
    let api = core.collection_set().collections[0].id.clone();
    core.activate(&api).unwrap();
    assert_eq!(
        core.collection_set().active_repository.as_deref(),
        Some(work_id.as_str())
    );
    // Collection paths again: relative to the collection.
    let files = changed(&core);
    assert!(
        files.contains(&"opencollection.yml".to_owned()),
        "{files:?}"
    );

    assert!(repositories::activate(&core, "/nowhere").is_err());
}

#[test]
fn nested_repositories_are_listed_separately() {
    let outer = temp("outer");
    repo(&outer.0);
    write(&outer.0.join("notes.md"), "x");
    let inner = outer.0.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    repo(&inner);
    write(&inner.join("inner.md"), "y");

    let core = core();
    repositories::open(&core, &inner).unwrap();
    let set = repositories::open(&core, &outer.0).unwrap();
    let ids: Vec<_> = set.repositories.iter().map(|r| r.id.clone()).collect();
    assert_eq!(
        ids,
        [
            inner.to_string_lossy().into_owned(),
            outer.0.to_string_lossy().into_owned()
        ]
    );
    // Each status is its own working tree: the outer one does not list the inner files.
    assert!(changed(&core).contains(&"notes.md".to_owned()));
    assert!(!changed(&core).iter().any(|p| p.contains("inner.md")));
    repositories::activate(&core, &inner.to_string_lossy()).unwrap();
    assert_eq!(changed(&core), ["inner.md"]);
}

#[test]
fn removing_a_root_removes_its_repository() {
    let notes = temp("notes3");
    repo(&notes.0);
    let work = temp("work3");
    collection(&work.0, "W");
    let core = core();
    repositories::open(&core, &work.0).unwrap();
    let set = repositories::open(&core, &notes.0).unwrap();
    assert_eq!(set.repositories.len(), 1);
    let root = set
        .roots
        .iter()
        .find(|r| r.collections.is_empty())
        .unwrap()
        .id
        .clone();
    let set = core.remove_root(&root).unwrap();
    assert!(set.repositories.is_empty());
    assert_eq!(set.active_repository, None);
}

#[test]
fn the_summary_shows_branch_changes_and_a_half_done_merge() {
    let notes = temp("notes4");
    repo(&notes.0);
    write(&notes.0.join("a.md"), "a");
    write(&notes.0.join("b.md"), "b");
    let core = core();
    repositories::open(&core, &notes.0).unwrap();
    let id = notes.0.to_string_lossy().into_owned();

    let s = repositories::summary(&core, &id).unwrap();
    assert_eq!(s.id, id);
    assert_eq!(s.changes, 2);
    assert_eq!(s.operation, None);
    git::commit(&core, "First", &["a.md".into(), "b.md".into()]).unwrap();
    let s = repositories::summary(&core, &id).unwrap();
    assert_eq!(s.changes, 0);
    assert!(s.branch.is_some());
    assert_eq!((s.ahead, s.behind), (0, 0));

    // A rebase started on the command line is reported, not hidden.
    std::fs::create_dir_all(notes.0.join(".git/rebase-merge")).unwrap();
    let s = repositories::summary(&core, &id).unwrap();
    assert_eq!(s.operation, Some(GitOperation::Rebase));
    assert!(repositories::summary(&core, "/nowhere").is_err());
}

#[test]
fn clone_adds_the_new_folder_to_the_set_and_makes_it_active() {
    let work = temp("clone-src");
    repo(&work.0);
    write(&work.0.join("hello.md"), "hi");
    {
        let src = core();
        repositories::open(&src, &work.0).unwrap();
        git::commit(&src, "Hello", &["hello.md".into()]).unwrap();
    }

    let parent = temp("clone-parent");
    let core = core();
    let first = temp("clone-first");
    repo(&first.0);
    repositories::open(&core, &first.0).unwrap();

    let entry = repositories::clone(
        &core,
        &GitCloneRequest {
            url: rt_git::local_url(&work.0),
            parent: None,
            name: Some("copy".into()),
            branch: None,
        },
        &parent.0,
    )
    .unwrap();
    assert_eq!(entry.name, "copy");
    assert!(parent.0.join("copy/hello.md").is_file());
    let set = core.collection_set();
    assert_eq!(set.repositories.len(), 2);
    assert_eq!(set.active_repository.as_deref(), Some(entry.id.as_str()));
    assert!(changed(&core).is_empty());

    // A name that is a path is refused, so is an existing folder.
    let bad = GitCloneRequest {
        url: rt_git::local_url(&work.0),
        parent: None,
        name: Some("../x".into()),
        branch: None,
    };
    assert!(repositories::clone(&core, &bad, &parent.0).is_err());
    let again = GitCloneRequest {
        name: Some("copy".into()),
        ..bad
    };
    assert!(repositories::clone(&core, &again, &parent.0).is_err());
}

#[test]
fn init_adds_a_repository_root_and_refuses_an_existing_repository() {
    let folder = temp("init");
    write(&folder.0.join("a.txt"), "a");
    let core = core();
    let entry = repositories::init(&core, &folder.0).unwrap();
    assert_eq!(entry.id, folder.0.to_string_lossy());
    assert!(entry.collections.is_empty());
    assert!(folder.0.join(".git").exists());
    assert_eq!(
        core.collection_set().active_repository.as_deref(),
        Some(entry.id.as_str())
    );
    assert_eq!(changed(&core), ["a.txt"]);
    assert!(repositories::init(&core, &folder.0).is_err());
}

#[test]
fn a_repository_root_comes_back_with_the_remembered_set() {
    let data = temp("data");
    let notes = temp("notes5");
    repo(&notes.0);
    let work = temp("work5");
    repo(&work.0);
    collection(&work.0, "W");
    {
        let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
        core.open(&work.0).unwrap();
        repositories::open(&core, &notes.0).unwrap();
    }
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
    let info = core.restore(&work.0).unwrap().unwrap();
    assert_eq!(info.name, "W");
    let set = core.collection_set();
    assert_eq!(set.repositories.len(), 2);
    assert!(set.roots.iter().any(|r| r.collections.is_empty()));
}
