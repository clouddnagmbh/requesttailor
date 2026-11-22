// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Committing files onto a branch without a checkout (`Repo::commit_to_branch`, wave 13).

mod common;

use common::*;
use rt_git::TreeFile;

#[test]
fn files_go_onto_a_new_branch_and_nothing_else_moves() {
    let t = tmp("tree-commit");
    let root = t.path().join("api");
    std::fs::create_dir_all(&root).unwrap();
    rt_git::init(t.path(), &opts()).unwrap();
    set_user(t.path(), "Ada");
    let repo = rt_git::Repo::require(&root, &opts()).unwrap();
    write(&root, "opencollection.yml", "opencollection: 1.0.0\n");
    write(&root, "get.yml", "url: /v1\n");
    let main = repo
        .commit("first", &paths(&["opencollection.yml", "get.yml"]), None)
        .unwrap();
    // Local work in progress must survive untouched.
    write(&root, "get.yml", "url: /v2-unsaved\n");
    let status_before = repo.status().unwrap();
    let index_before = std::fs::read(t.path().join(".git/index")).unwrap();

    let branch = "recordings/2026-11-22/app-orders";
    let commit = repo
        .commit_to_branch(
            branch,
            "HEAD",
            &[
                TreeFile::write("Recordings/orders/folder.yml", "info:\n  name: orders\n"),
                TreeFile::write("Recordings/orders/trace.json", b"{}".to_vec()),
                TreeFile::write("/NOTES.md", "at the repository root\n"),
                TreeFile::remove("get.yml"),
                TreeFile::remove("never-there.yml"),
            ],
            "Recording: orders",
            None,
        )
        .unwrap();
    assert_eq!(commit.summary, "Recording: orders");
    assert_eq!(
        ref_id(t.path(), &format!("refs/heads/{branch}")).unwrap(),
        commit.oid
    );
    assert_eq!(repo.rev_parse("HEAD").unwrap(), main.oid, "HEAD stays");
    assert_eq!(
        read(&root, "get.yml"),
        "url: /v2-unsaved\n",
        "working tree stays"
    );
    assert!(!root.join("Recordings").exists());
    assert_eq!(
        std::fs::read(t.path().join(".git/index")).unwrap(),
        index_before,
        "index stays"
    );
    assert_eq!(repo.status().unwrap(), status_before);

    let tree = repo.tree_at(branch).unwrap();
    let files: Vec<&str> = tree.files.keys().map(String::as_str).collect();
    assert_eq!(
        files,
        vec!["Recordings/orders/folder.yml", "opencollection.yml"]
    );
    assert_eq!(
        repo.show(branch, "Recordings/orders/trace.json")
            .unwrap()
            .as_deref(),
        Some("{}")
    );
    assert_eq!(
        repo.show(branch, "/NOTES.md").unwrap().as_deref(),
        Some("at the repository root\n")
    );
    assert_eq!(repo.commits(Some("HEAD"), branch, 10).unwrap().len(), 1);
    assert!(
        repo.branches()
            .unwrap()
            .iter()
            .any(|b| b.name == branch && !b.current)
    );

    // A second commit goes on top of the branch's tip, whatever `start` says.
    let second = repo
        .commit_to_branch(
            branch,
            "no-such-rev",
            &[TreeFile::write("Recordings/orders/b.yml", "url: /b\n")],
            "more",
            None,
        )
        .unwrap();
    assert_eq!(repo.rev_parse(&format!("{branch}~1")).unwrap(), commit.oid);
    assert_eq!(repo.rev_parse(branch).unwrap(), second.oid);
}

#[test]
fn unsafe_and_empty_commits_are_refused() {
    let t = tmp("tree-commit-refused");
    let repo = new_repo(t.path(), "Ada");
    write(t.path(), "opencollection.yml", "opencollection: 1.0.0\n");
    repo.commit("first", &paths(&["opencollection.yml"]), None)
        .unwrap();
    let file = [TreeFile::write("a.yml", "url: /a\n")];
    let current = repo
        .branches()
        .unwrap()
        .into_iter()
        .find(|b| b.current)
        .unwrap()
        .name;
    for (branch, start, files, message) in [
        (current.as_str(), "HEAD", &file[..], "x"),
        ("bad name", "HEAD", &file[..], "x"),
        ("ok", "HEAD", &file[..], "  "),
        ("ok", "HEAD", &[][..], "x"),
        ("ok", "no-such-rev", &file[..], "x"),
        (
            "ok",
            "HEAD",
            &[TreeFile::write("../escape.yml", "x")][..],
            "x",
        ),
        (
            "ok",
            "HEAD",
            &[TreeFile::write(".git/config", "x")][..],
            "x",
        ),
        ("ok", "HEAD", &[TreeFile::remove("missing.yml")][..], "x"),
    ] {
        assert!(
            repo.commit_to_branch(branch, start, files, message, None)
                .is_err(),
            "{branch:?} {start:?} {files:?} {message:?}"
        );
    }
    assert!(
        ref_id(t.path(), "refs/heads/ok").is_none(),
        "nothing was written"
    );
}
