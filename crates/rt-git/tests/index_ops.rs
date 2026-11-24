// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The index as building blocks (wave 15, strand F): whole files staged and unstaged like
//! `git add` and `git reset`, `commitStaged` commits what the index holds (same tree as
//! `git commit`), refuses credentials (secret scan) and an empty index, leaves the rest of the
//! working tree alone, and concludes a merge. The app's own `commit(message, paths)` is
//! unchanged.

mod common;
mod wizard;

use common::*;
use rt_app::AppError;
use rt_app::git::index::{GitPatchSide, GitStagedCommit};
use wizard::*;

fn have_git() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
}

fn staged(message: &str) -> GitStagedCommit {
    GitStagedCommit {
        message: message.into(),
    }
}

fn copy_of(f: &Fixture, name: &str) -> Tmp {
    let to = tmp(name);
    assert!(
        std::process::Command::new("cp")
            .arg("-R")
            .arg(format!("{}/.", f.dir().display()))
            .arg(to.path())
            .status()
            .unwrap()
            .success()
    );
    to
}

fn ls(dir: &std::path::Path) -> String {
    git(dir, &["ls-files", "-s"]).unwrap()
}

#[test]
fn stage_and_unstage_whole_files_like_git() {
    if !have_git() {
        return;
    }
    let f = fixture("index-whole");
    f.write("notes.txt", "changed\n");
    f.write("added.txt", "new\n");
    std::fs::remove_file(f.dir().join("users/get.yml")).unwrap();
    let copy = copy_of(&f, "index-whole-git");
    git(
        copy.path(),
        &["add", "notes.txt", "added.txt", "users/get.yml"],
    );
    let repo = f.repo();
    repo.stage(&paths(&["/notes.txt", "/added.txt", "/users/get.yml"]))
        .unwrap();
    assert_eq!(ls(f.dir()), ls(copy.path()));
    assert_eq!(
        git(f.dir(), &["status", "--porcelain"]).unwrap(),
        "A  added.txt\nM  notes.txt\nD  users/get.yml\n"
    );

    repo.unstage(&paths(&["/added.txt", "/users/get.yml"]))
        .unwrap();
    git(copy.path(), &["reset", "-q", "added.txt", "users/get.yml"]);
    assert_eq!(ls(f.dir()), ls(copy.path()));
    assert_eq!(
        git(f.dir(), &["status", "--porcelain"]).unwrap(),
        "M  notes.txt\n D users/get.yml\n?? added.txt\n"
    );
    // The working tree never changes.
    assert_eq!(f.read("added.txt"), "new\n");
    assert!(!f.exists("users/get.yml"));
}

#[test]
fn commit_staged_commits_the_index_only() {
    if !have_git() {
        return;
    }
    let f = fixture("index-commit");
    f.write("notes.txt", "changed\n");
    f.write("other.txt", "unstaged\n");
    let repo = f.repo();
    // Nothing staged yet.
    let err = repo.commit_staged(&staged("x"), None).unwrap_err();
    assert!(err.to_string().contains("nothing is staged"), "{err}");
    repo.stage(&paths(&["/notes.txt"])).unwrap();
    assert!(repo.commit_staged(&staged("  "), None).is_err());
    let head_before = ref_id(f.dir(), "refs/heads/main").unwrap();
    let c = repo.commit_staged(&staged("Staged only"), None).unwrap();
    assert_eq!(c.summary, "Staged only");
    assert_eq!(c.author, "Ada");
    // Same tree as git makes from the same index, and a clean history.
    let tree = git(f.dir(), &["rev-parse", &format!("{}^{{tree}}", c.oid)]).unwrap();
    assert_eq!(
        git(f.dir(), &["rev-parse", "HEAD^"]).unwrap().trim(),
        head_before
    );
    let copy = copy_of(&f, "index-commit-git");
    git(copy.path(), &["reset", "-q", "--soft", "HEAD^"]);
    git(copy.path(), &["commit", "-q", "-m", "Staged only"]);
    assert_eq!(
        git(copy.path(), &["rev-parse", "HEAD^{tree}"]).unwrap(),
        tree
    );
    // The unstaged file is still just untracked, and git agrees the index is clean.
    assert_eq!(
        git(f.dir(), &["status", "--porcelain"]).unwrap(),
        "?? other.txt\n"
    );
    assert_eq!(
        git(f.dir(), &["fsck", "--no-progress"]).map(|_| ()),
        Some(())
    );
}

#[test]
fn commit_staged_refuses_credentials_but_the_commit_form_is_unchanged() {
    let f = fixture("index-secret");
    f.write(
        "keys.txt",
        "token = ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\n",
    );
    let repo = f.repo();
    repo.stage(&paths(&["/keys.txt"])).unwrap();
    let head = ref_id(f.dir(), "refs/heads/main");
    let err = repo.commit_staged(&staged("secrets"), None).unwrap_err();
    assert!(matches!(err, AppError::SecretsFound(_)), "{err:?}");
    assert_eq!(ref_id(f.dir(), "refs/heads/main"), head);
    // A marked line passes.
    f.write(
        "keys.txt",
        "token = ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789 # rt-scan:allow\n",
    );
    repo.stage(&paths(&["/keys.txt"])).unwrap();
    repo.commit_staged(&staged("allowed"), None).unwrap();
}

#[test]
fn the_index_follows_the_collection_scope_and_paths_outside_it() {
    let f = fixture("index-scope");
    // The collection sits in a sub-folder of the repository.
    f.write("docs/readme.txt", "hello\n");
    let repo = rt_git::Repo::require(&f.dir().join("users"), &opts()).unwrap();
    // Collection-relative and `/`-prefixed paths name the same file.
    f.write("users/get.yml", "changed: 1\n");
    let a = repo.index_diff("get.yml", GitPatchSide::Unstaged).unwrap();
    let b = repo
        .index_diff("/users/get.yml", GitPatchSide::Unstaged)
        .unwrap();
    assert_eq!(a.old, b.old);
    assert_eq!(a.new, b.new);
    repo.stage(&paths(&["/docs/readme.txt"])).unwrap();
    assert!(
        git(f.dir(), &["ls-files", "docs/readme.txt"])
            .unwrap()
            .contains("readme")
    );
}

#[test]
fn a_conflicted_file_is_refused_until_it_is_staged() {
    if !have_git() {
        return;
    }
    let f = fixture("index-conflict");
    git(f.dir(), &["checkout", "-q", "-b", "other"]);
    f.commit("other", &[("notes.txt", "other\n")]);
    git(f.dir(), &["checkout", "-q", "main"]);
    f.commit("main", &[("notes.txt", "main\n")]);
    // A line-level conflict made by git itself.
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(f.dir())
        .args(["merge", "other"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("HOME", f.dir())
        .output()
        .unwrap();
    let repo = f.repo();
    let err = repo
        .index_diff("/notes.txt", GitPatchSide::Unstaged)
        .unwrap_err();
    assert!(err.to_string().contains("conflict"), "{err}");
    assert!(repo.commit_staged(&staged("m"), None).is_err());
    // Resolving by hand and staging concludes the merge with two parents.
    f.write("notes.txt", "both\n");
    repo.stage(&paths(&["/notes.txt"])).unwrap();
    let c = repo.commit_staged(&staged("Merge other"), None).unwrap();
    let parents = git(f.dir(), &["rev-list", "--parents", "-n", "1", &c.oid]).unwrap();
    assert_eq!(parents.split_whitespace().count(), 3, "{parents}");
    assert!(!f.dir().join(".git/MERGE_HEAD").exists());
}
