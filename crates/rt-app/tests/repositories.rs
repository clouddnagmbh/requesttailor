// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 15, strand A: repositories in the collection set. The rules every shell shares: what a
//! root is (collections, a repository root, or a new collection), which repositories the set
//! lists and in which order, which one is active, and what the sidebar shows per repository.

use rt_app::collections::CollectionEntry;
use rt_app::git::GitStatus;
use rt_app::git::repos::{
    GitOperation, RepoRoot, RepositoryEntry, RootKind, active_repository, repositories, root_kind,
    summarize,
};

fn entry(id: &str, root: &str, repo: Option<&str>) -> CollectionEntry {
    CollectionEntry {
        id: id.into(),
        name: id.into(),
        location: id.into(),
        root: root.into(),
        dir: String::new(),
        repo: repo.map(Into::into),
        dirty: 0,
    }
}

fn describe(id: &str) -> (String, String, bool) {
    let name = id.rsplit(['/', '\\']).next().unwrap_or(id).to_owned();
    (name, id.to_owned(), id.ends_with("-linked"))
}

#[test]
fn a_folder_without_collection_is_a_repository_root_only_inside_git() {
    assert_eq!(root_kind(true, Some("/work")), RootKind::Collections);
    assert_eq!(root_kind(true, None), RootKind::Collections);
    assert_eq!(root_kind(false, Some("/work")), RootKind::Repository);
    // Outside Git it stays what it was before wave 15: a new collection.
    assert_eq!(root_kind(false, None), RootKind::NewCollection);
}

#[test]
fn repositories_with_collections_come_first_then_repository_roots() {
    let entries = vec![
        entry("/work/a", "/work", Some("/work")),
        entry("/work/b", "/work", Some("/work")),
        entry("/scratch", "/scratch", None),
        entry("/other/c", "/other", Some("/other")),
    ];
    let roots = vec![
        RepoRoot {
            root: "/notes".into(),
            repo: "/notes".into(),
        },
        RepoRoot {
            root: "/work".into(),
            repo: "/work".into(),
        },
    ];
    let list = repositories(&entries, &roots, describe);
    let shown: Vec<_> = list
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.name.as_str(),
                r.root.as_str(),
                r.collections.len(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("/work", "work", "/work", 2),
            ("/other", "other", "/other", 1),
            ("/notes", "notes", "/notes", 0),
        ]
    );
}

#[test]
fn nested_repositories_are_separate_entries() {
    // A collection that is a repository of its own inside an outer repository root.
    let entries = vec![entry("/outer/inner", "/outer", Some("/outer/inner"))];
    let roots = vec![RepoRoot {
        root: "/outer".into(),
        repo: "/outer".into(),
    }];
    let list = repositories(&entries, &roots, describe);
    let ids: Vec<_> = list.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["/outer/inner", "/outer"]);
    assert_eq!(list[0].collections, ["/outer/inner"]);
    assert!(list[1].collections.is_empty());
}

#[test]
fn a_repository_reached_from_two_roots_is_listed_once_under_the_first() {
    let entries = vec![entry("/w/a", "/w", Some("/w"))];
    let roots = vec![
        RepoRoot {
            root: "/w/sub".into(),
            repo: "/w".into(),
        },
        RepoRoot {
            root: "/v".into(),
            repo: "/v".into(),
        },
        RepoRoot {
            root: "/v/also".into(),
            repo: "/v".into(),
        },
    ];
    let list = repositories(&entries, &roots, describe);
    let shown: Vec<_> = list
        .iter()
        .map(|r| (r.id.as_str(), r.root.as_str()))
        .collect();
    assert_eq!(shown, [("/w", "/w"), ("/v", "/v")]);
}

#[test]
fn linked_worktrees_are_marked() {
    let roots = vec![RepoRoot {
        root: "/wt".into(),
        repo: "/wt-linked".into(),
    }];
    let list = repositories(&[], &roots, describe);
    assert!(list[0].linked);
}

#[test]
fn the_active_repository_follows_the_choice_then_the_active_collection() {
    let entries = vec![
        entry("/work/a", "/work", Some("/work")),
        entry("/scratch", "/scratch", None),
    ];
    let roots = vec![RepoRoot {
        root: "/notes".into(),
        repo: "/notes".into(),
    }];
    let list: Vec<RepositoryEntry> = repositories(&entries, &roots, describe);

    // Nothing chosen: the active collection's repository, none outside Git.
    assert_eq!(
        active_repository(&list, &entries, None, Some("/work/a")).as_deref(),
        Some("/work")
    );
    assert_eq!(
        active_repository(&list, &entries, None, Some("/scratch")),
        None
    );
    assert_eq!(active_repository(&list, &entries, None, None), None);
    // A repository chosen in the sidebar wins, also without any collection.
    assert_eq!(
        active_repository(&list, &entries, Some("/notes"), Some("/work/a")).as_deref(),
        Some("/notes")
    );
    assert_eq!(
        active_repository(&list, &entries, Some("/notes"), None).as_deref(),
        Some("/notes")
    );
    // A choice that is no longer in the set is ignored.
    assert_eq!(
        active_repository(&list, &entries, Some("/gone"), Some("/work/a")).as_deref(),
        Some("/work")
    );
}

#[test]
fn the_summary_counts_the_whole_working_tree() {
    let status = GitStatus {
        repo: true,
        branch: Some("main".into()),
        upstream: Some("origin/main".into()),
        ahead: 2,
        behind: 1,
        files: vec![
            rt_app::git::GitFile {
                path: "a".into(),
                state: rt_app::git::GitFileState::Modified,
            },
            rt_app::git::GitFile {
                path: "b".into(),
                state: rt_app::git::GitFileState::Untracked,
            },
        ],
        merging: true,
    };
    let s = summarize("/work", &status, Some(GitOperation::Merge));
    assert_eq!(s.id, "/work");
    assert_eq!(s.branch.as_deref(), Some("main"));
    assert_eq!((s.ahead, s.behind, s.changes), (2, 1, 2));
    assert_eq!(s.operation, Some(GitOperation::Merge));
    // A merge flag without a known kind is still shown as a merge.
    assert_eq!(
        summarize("/work", &status, None).operation,
        Some(GitOperation::Merge)
    );
    let calm = GitStatus {
        merging: false,
        ..status
    };
    assert_eq!(summarize("/work", &calm, None).operation, None);
    // The JSON leaves `operation` out when nothing is half done.
    let json = serde_json::to_value(summarize("/work", &calm, None)).unwrap();
    assert!(json.get("operation").is_none());
}
