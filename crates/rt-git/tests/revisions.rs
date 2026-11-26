// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12 interface: reading revisions without a checkout (`revParse`, `treeAt`, `show`,
//! `mergeBase`, `commits`, `fileLog`).

mod common;

use common::*;
use rt_app::git::GitFileChange;
use rt_app::git::GitTreeFiles as _;

#[test]
fn revisions_are_read_without_a_checkout() {
    let t = tmp("revisions");
    let root = t.path().join("api");
    std::fs::create_dir_all(&root).unwrap();
    // The repository starts above the collection; files outside it are never listed.
    write(t.path(), "README.md", "outside\n");
    write(t.path(), "openapi.yaml", "openapi: 3.1.0\n");
    let repo = {
        rt_git::init(t.path(), &opts()).unwrap();
        set_user(t.path(), "Ada");
        rt_git::Repo::require(&root, &opts()).unwrap()
    };
    write(&root, "opencollection.yml", "opencollection: 1.0.0\n");
    write(&root, "get.yml", "url: /v1\n");
    write(&root, "notes.txt", "not a collection file\n");
    write(&root, ".hidden/x.yml", "skipped\n");
    write(
        &root,
        "nested/opencollection.yml",
        "opencollection: 1.0.0\n",
    );
    write(&root, "nested/n.yml", "skipped\n");
    let first = repo
        .commit(
            "first",
            &paths(&[
                "opencollection.yml",
                "get.yml",
                "notes.txt",
                ".hidden/x.yml",
                "nested/opencollection.yml",
                "nested/n.yml",
            ]),
            None,
        )
        .unwrap();
    write(&root, "get.yml", "url: /v2\n");
    let second = repo.commit("second", &paths(&["get.yml"]), None).unwrap();
    write(&root, "post.yml", "method: POST\n");
    let third = repo.commit("third", &paths(&["post.yml"]), None).unwrap();

    assert_eq!(repo.rev_parse("HEAD").unwrap(), third.oid);
    assert_eq!(repo.rev_parse("HEAD~2").unwrap(), first.oid);
    assert_eq!(repo.rev_parse("HEAD^").unwrap(), second.oid);
    assert_eq!(repo.rev_parse(&third.oid[..10]).unwrap(), third.oid);
    assert!(repo.rev_parse("no-such-branch").is_err());

    let tree = repo.tree_at(&first.oid).unwrap();
    assert_eq!(tree.oid, first.oid);
    let files: Vec<&str> = tree.files.keys().map(String::as_str).collect();
    assert_eq!(files, vec!["get.yml", "opencollection.yml"]);
    assert_eq!(tree.files["get.yml"], "url: /v1\n");
    assert_eq!(
        repo.tree_at("HEAD").unwrap().to_file_set().files.len(),
        3,
        "post.yml is in HEAD"
    );

    assert_eq!(
        repo.show("HEAD~1", "get.yml").unwrap().as_deref(),
        Some("url: /v2\n")
    );
    assert_eq!(repo.show(&first.oid, "post.yml").unwrap(), None);
    // A leading `/` is relative to the repository root.
    assert_eq!(
        repo.show("HEAD", "/openapi.yaml").unwrap(),
        None,
        "never committed"
    );
    assert!(repo.show("HEAD", "../README.md").is_err());

    assert_eq!(
        repo.merge_base(&third.oid, &first.oid).unwrap().as_deref(),
        Some(first.oid.as_str())
    );

    let range: Vec<String> = repo
        .commits(Some(&first.oid), "HEAD", 10)
        .unwrap()
        .into_iter()
        .map(|c| c.oid)
        .collect();
    assert_eq!(range, vec![third.oid.clone(), second.oid.clone()]);
    assert_eq!(repo.commits(None, "HEAD", 2).unwrap().len(), 2);

    let log = repo.file_log("get.yml", 10).unwrap();
    let got: Vec<(&str, GitFileChange)> = log
        .iter()
        .map(|c| (c.commit.oid.as_str(), c.change))
        .collect();
    assert_eq!(
        got,
        vec![
            (second.oid.as_str(), GitFileChange::Modified),
            (first.oid.as_str(), GitFileChange::Added),
        ]
    );
    assert_eq!(repo.file_log("get.yml", 1).unwrap().len(), 1);
}
