// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Seam N1: the Git core merges with the file merger of the `Repo`. RequestTailor's default
//! (`CollectionMerger`) merges collections on the model; without a merger every file merges
//! line by line like Git; another merger plugged in decides the files it claims, in the plan
//! and in the merge.

mod common;
mod wizard;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rt_app::git::GitPullResult;
use rt_git::Repo;
use tk_git_types::{FileMerger, GroupFiles, GroupMerge};
use wizard::*;

/// `side` and `main` both change the fixture after "init".
fn diverge(f: &Fixture, main: &[(&str, &str)], side: &[(&str, &str)]) {
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"side","start":"HEAD","switch":true}"#),
    );
    f.commit("side", side);
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit("main", main);
}

fn merge_side(repo: &Repo) -> GitPullResult {
    apply(repo, &act(r#"{"action":"merge","from":"side"}"#))
        .outcome
        .expect("a merge has an outcome")
}

/// Neighbouring lines of one request: different fields for the model, one hunk for Git.
fn neighbours(name: &str) -> Fixture {
    let f = fixture(name);
    diverge(
        &f,
        &[(
            "users/get.yml",
            &request("POST", "https://api.test/users/1"),
        )],
        &[("users/get.yml", &request("GET", "https://api.test/users/2"))],
    );
    f
}

#[test]
fn the_default_merges_collections_on_the_model() {
    let f = neighbours("merger-default");
    assert!(matches!(
        merge_side(&f.repo()),
        GitPullResult::Merged { .. }
    ));
    let merged = std::fs::read_to_string(f.dir().join("users/get.yml")).unwrap();
    assert_eq!(merged, request("POST", "https://api.test/users/2"));
}

#[test]
fn without_a_merger_git_merges_line_by_line() {
    let f = neighbours("merger-none");
    let repo = f.repo().with_merger(None);
    let plan = repo
        .plan(&act(r#"{"action":"merge","from":"side"}"#), false)
        .unwrap();
    let files = plan.merge.unwrap().files;
    assert!(files.iter().all(|file| !file.semantic), "{files:?}");
    match merge_side(&repo) {
        GitPullResult::Conflicts { paths } => assert_eq!(paths, ["users/get.yml"]),
        other => panic!("{other:?}"),
    }
    let text = std::fs::read_to_string(f.dir().join("users/get.yml")).unwrap();
    assert!(text.contains("<<<<<<<"), "{text}");
}

/// Merges `*.txt` at the root as the union of both sides' lines.
struct UnionOfLines;

impl FileMerger for UnionOfLines {
    fn groups(&self, paths: &[&str]) -> BTreeSet<String> {
        if paths
            .iter()
            .any(|p| !p.contains('/') && p.ends_with(".txt"))
        {
            BTreeSet::from([String::new()])
        } else {
            BTreeSet::new()
        }
    }

    fn claims(&self, rel: &str) -> bool {
        !rel.contains('/') && rel.ends_with(".txt")
    }

    fn merge(&self, _: &GroupFiles, ours: &GroupFiles, theirs: &GroupFiles) -> Option<GroupMerge> {
        let mut files = GroupFiles::new();
        for (path, mine) in ours {
            let mut text = String::from_utf8(mine.clone()).ok()?;
            let have: BTreeSet<String> = text.lines().map(str::to_owned).collect();
            if let Some(other) = theirs.get(path) {
                for line in String::from_utf8(other.clone()).ok()?.lines() {
                    if !have.contains(line) {
                        text.push_str(line);
                        text.push('\n');
                    }
                }
            }
            files.insert(path.clone(), text.into_bytes());
        }
        Some(GroupMerge {
            paths: files.keys().cloned().collect(),
            files,
            ..GroupMerge::default()
        })
    }

    fn changed_parts(&self, _: &GroupFiles, theirs: &GroupFiles) -> BTreeMap<String, u32> {
        theirs.keys().map(|p| (p.clone(), 7)).collect()
    }
}

#[test]
fn a_plugged_in_merger_decides_the_files_it_claims() {
    let f = fixture("merger-union");
    diverge(
        &f,
        &[("notes.txt", "one\nours\n")],
        &[("notes.txt", "one\ntheirs\n")],
    );
    let repo = f.repo().with_merger(Some(Arc::new(UnionOfLines)));

    let plan = repo
        .plan(&act(r#"{"action":"merge","from":"side"}"#), false)
        .unwrap();
    let files = plan.merge.unwrap().files;
    let notes = files.iter().find(|x| x.path == "/notes.txt").unwrap();
    assert!(notes.semantic && notes.conflicts.is_empty(), "{notes:?}");
    assert_eq!(notes.clean, 7, "the merger's count of changed parts");

    assert!(matches!(merge_side(&repo), GitPullResult::Merged { .. }));
    let merged = std::fs::read_to_string(f.dir().join("notes.txt")).unwrap();
    assert_eq!(merged, "one\nours\ntheirs\n");
}

#[test]
fn the_same_change_conflicts_for_git_without_that_merger() {
    let f = fixture("merger-union-none");
    diverge(
        &f,
        &[("notes.txt", "one\nours\n")],
        &[("notes.txt", "one\ntheirs\n")],
    );
    assert!(matches!(
        merge_side(&f.repo()),
        GitPullResult::Conflicts { .. }
    ));
}
