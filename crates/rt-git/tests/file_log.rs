// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand D: `fileLog` follows renames and moves. The web shell's
//! `apps/web/src/git/file-log.test.ts` builds the same history and expects the same log.

mod common;

use common::*;
use rt_app::git::GitFileChange::{self, *};

fn request(url: &str, extra: &str) -> String {
    format!(
        "info:\n  name: Users\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {url}\n  auth: inherit\n{extra}"
    )
}

fn mv(root: &std::path::Path, from: &str, to: &str) {
    let text = read(root, from);
    std::fs::remove_file(root.join(from)).unwrap();
    write(root, to, &text);
}

#[test]
fn history_follows_renames_and_moves() {
    let t = tmp("file-log");
    let root = t.path();
    rt_git::init(root, &opts()).unwrap();
    set_user(root, "Ada");
    let repo = rt_git::Repo::require(root, &opts()).unwrap();
    let commit = |msg: &str, files: &[&str]| repo.commit(msg, &paths(files), None).unwrap().oid;

    write(root, "opencollection.yml", "opencollection: 1.0.0\n");
    write(root, "other.yml", &request("{{base}}/other", ""));
    let c1 = {
        write(root, "users/get.yml", &request("{{base}}/users", ""));
        commit("add", &["opencollection.yml", "other.yml", "users/get.yml"])
    };
    write(root, "users/get.yml", &request("{{base}}/users?x=1", ""));
    let c2 = commit("edit", &["users/get.yml"]);
    // A pure rename.
    mv(root, "users/get.yml", "users/list.yml");
    let c3 = commit("rename", &["users/get.yml", "users/list.yml"]);
    write(root, "users/list.yml", &request("{{base}}/users?x=2", ""));
    let c4 = commit("edit again", &["users/list.yml"]);
    // A move into another folder with a small edit.
    mv(root, "users/list.yml", "api/list.yml");
    write(root, "api/list.yml", &request("{{base}}/api/users?x=2", ""));
    let c5 = commit("move", &["users/list.yml", "api/list.yml"]);
    write(
        root,
        "api/list.yml",
        &request("{{base}}/api/users?x=3", "  timeout: 5\n"),
    );
    let c6 = commit("edit last", &["api/list.yml"]);

    let log = repo.file_log("api/list.yml", 50).unwrap();
    let got: Vec<(&str, GitFileChange, &str, Option<&str>)> = log
        .iter()
        .map(|c| {
            (
                c.commit.oid.as_str(),
                c.change,
                c.path.as_str(),
                c.from.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (c6.as_str(), Modified, "api/list.yml", None),
            (c5.as_str(), Renamed, "api/list.yml", Some("users/list.yml")),
            (c4.as_str(), Modified, "users/list.yml", None),
            (
                c3.as_str(),
                Renamed,
                "users/list.yml",
                Some("users/get.yml")
            ),
            (c2.as_str(), Modified, "users/get.yml", None),
            (c1.as_str(), Added, "users/get.yml", None),
        ]
    );
    // The limit counts entries; an unrelated file has its own history.
    assert_eq!(repo.file_log("api/list.yml", 3).unwrap().len(), 3);
    let other = repo.file_log("other.yml", 50).unwrap();
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].change, Added);
    // A file that never existed has no history.
    assert!(repo.file_log("nope.yml", 10).unwrap().is_empty());
}

#[test]
fn an_unrelated_new_file_is_not_a_rename() {
    let t = tmp("file-log-added");
    let root = t.path();
    rt_git::init(root, &opts()).unwrap();
    set_user(root, "Ada");
    let repo = rt_git::Repo::require(root, &opts()).unwrap();
    write(root, "a.yml", "one\ntwo\nthree\nfour\n");
    repo.commit("a", &paths(&["a.yml"]), None).unwrap();
    std::fs::remove_file(root.join("a.yml")).unwrap();
    write(root, "b.yml", "completely\ndifferent\ntext\nhere\n");
    let c2 = repo
        .commit("swap", &paths(&["a.yml", "b.yml"]), None)
        .unwrap()
        .oid;
    let log = repo.file_log("b.yml", 10).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].commit.oid.as_str(), log[0].change),
        (c2.as_str(), Added)
    );
}
