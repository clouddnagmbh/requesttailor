// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Seam N3: a `Repo` works on a scope. `Repo::open` scopes to the collection folder and
//! counts the collection's files (RequestTailor, `CollectionFiles`); `Repo::open_repository`
//! takes the whole repository from any folder inside it (GitTailor), and without a file
//! filter every file counts; another filter plugged in decides alone.

mod common;
mod wizard;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use rt_git::Repo;
use tk_git_types::ScopeFiles;
use wizard::*;

fn files(repo: &Repo) -> Vec<String> {
    repo.tree_at("HEAD").unwrap().files.into_keys().collect()
}

fn with_docs(name: &str) -> Fixture {
    let f = fixture(name);
    f.commit("docs", &[("docs/readme.txt", "hello\n")]);
    f
}

#[test]
fn open_scopes_to_the_collection_folder_and_counts_collection_files() {
    let f = with_docs("scope-collection");
    let root = f.repo();
    assert!(root.scope().is_whole());
    assert_eq!(files(&root), ["opencollection.yml", "users/get.yml"]);

    let users = Repo::require(&f.dir().join("users"), &opts()).unwrap();
    assert_eq!(users.scope().prefix(), "users");
    assert_eq!(files(&users), ["get.yml"]);
}

#[test]
fn open_repository_takes_the_whole_repository_from_any_folder() {
    let f = with_docs("scope-whole");
    let repo = Repo::open_repository(&f.dir().join("users"), &opts())
        .unwrap()
        .expect("a repository")
        .with_files(None);
    assert!(repo.scope().is_whole());
    assert_eq!(repo.root(), repo.workdir());
    assert_eq!(
        files(&repo),
        [
            "docs/readme.txt",
            "notes.txt",
            "opencollection.yml",
            "users/get.yml"
        ]
    );

    // Paths are repository-relative: a file outside any collection commits by its path.
    f.write("docs/readme.txt", "changed\n");
    repo.commit("docs", &paths(&["docs/readme.txt"]), None)
        .unwrap();
    assert_eq!(
        repo.show("HEAD", "docs/readme.txt").unwrap().as_deref(),
        Some("changed\n")
    );

    let outside = tmp("scope-outside");
    assert!(
        Repo::open_repository(outside.path(), &opts())
            .unwrap()
            .is_none()
    );
}

/// Counts text files only, and drops everything below `docs/`.
struct TextOnly;

impl ScopeFiles for TextOnly {
    fn includes(&self, rel: &str) -> bool {
        rel.ends_with(".txt")
    }
    fn drop_nested(&self, files: &mut BTreeMap<String, String>) {
        files.retain(|p, _| !p.starts_with("docs/"));
    }
}

#[test]
fn a_plugged_in_filter_decides_which_files_count() {
    let f = with_docs("scope-filter");
    let repo = f.repo().with_files(Some(Arc::new(TextOnly)));
    assert_eq!(files(&repo), ["notes.txt"]);
}
