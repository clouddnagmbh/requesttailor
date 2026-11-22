// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Fetching a pull request's head for review (wave 12, strand E): the head and the target
//! branch land in `refs/requesttailor/review/...` without a checkout and without touching
//! the user's branches. Local remote here; over HTTP the desktop test
//! (`apps/desktop/src-tauri/tests/review.rs`) does it against the fake Git host.

mod common;

use common::*;
use rt_app::review::{ReviewKind, review_base_ref, review_ref};
use rt_git::NoCredentials;

/// A bare remote whose `main` is at the first commit and whose `refs/pull/1/head` is a second
/// commit on top of it (that nobody has fetched), plus a clone of it.
fn setup() -> (Tmp, Tmp, String, String) {
    let author = tmp("review-author");
    new_repo(author.path(), "Ada");
    write(
        author.path(),
        "opencollection.yml",
        "opencollection: 1.0.0\n",
    );
    write(author.path(), "a.yml", "one\n");
    let remote = tmp("review-remote");
    bare(remote.path());
    add_remote(author.path(), remote.path().to_str().unwrap());
    let work = reopen(author.path());
    let first = work
        .commit("first", &paths(&["opencollection.yml", "a.yml"]), None)
        .unwrap()
        .oid;
    write(author.path(), "a.yml", "two\n");
    let second = work.commit("second", &paths(&["a.yml"]), None).unwrap().oid;
    work.push(&NoCredentials).unwrap();
    // The server side: main stays at the first commit, the PR head is the second.
    let repo = gix::open_opts(remote.path(), gix::open::Options::isolated()).unwrap();
    use gix::refs::transaction::PreviousValue::Any;
    repo.reference(
        "refs/heads/main",
        first.parse::<gix::ObjectId>().unwrap(),
        Any,
        "t",
    )
    .unwrap();
    repo.reference(
        "refs/pull/1/head",
        second.parse::<gix::ObjectId>().unwrap(),
        Any,
        "t",
    )
    .unwrap();
    (author, remote, first, second)
}

#[test]
fn fetches_head_and_target_into_review_refs() {
    let (_author, remote, first, second) = setup();
    let parent = tmp("review-clone");
    let dir = rt_git::clone(
        remote.path().to_str().unwrap(),
        parent.path(),
        &NoCredentials,
        &opts(),
    )
    .unwrap();
    let repo = reopen(&dir);
    assert!(
        repo.rev_parse(&second).is_err(),
        "the PR head is not here yet"
    );

    let got = repo
        .fetch_review(
            ReviewKind::Github,
            1,
            "refs/pull/1/head",
            "main",
            &NoCredentials,
        )
        .unwrap();
    assert_eq!(got.head_ref, review_ref(ReviewKind::Github, 1));
    assert_eq!(got.head, second);
    assert_eq!(got.merge_base, first);
    assert_eq!(
        ref_id(&dir, "refs/requesttailor/review/github/1").as_deref(),
        Some(second.as_str())
    );
    assert_eq!(
        ref_id(&dir, &review_base_ref(ReviewKind::Github, 1)).as_deref(),
        Some(first.as_str())
    );
    // No checkout, the user's branch is untouched, and the files are readable at the head.
    assert_eq!(read(&dir, "a.yml"), "one\n");
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
    assert!(repo.status().unwrap().files.is_empty());
    let tree = repo.tree_at(&got.head_ref).unwrap();
    assert_eq!(tree.files.get("a.yml").map(String::as_str), Some("two\n"));
    let before = repo.tree_at(&got.merge_base).unwrap();
    assert_eq!(before.files.get("a.yml").map(String::as_str), Some("one\n"));

    // Opening again is fine (the refs are ours and are overwritten).
    let again = repo
        .fetch_review(
            ReviewKind::Github,
            1,
            "refs/pull/1/head",
            "main",
            &NoCredentials,
        )
        .unwrap();
    assert_eq!(again, got);
}

#[test]
fn a_missing_pull_request_is_an_error_not_a_panic() {
    let (_author, remote, ..) = setup();
    let parent = tmp("review-clone2");
    let dir = rt_git::clone(
        remote.path().to_str().unwrap(),
        parent.path(),
        &NoCredentials,
        &opts(),
    )
    .unwrap();
    let e = reopen(&dir)
        .fetch_review(
            ReviewKind::Gitlab,
            9,
            "refs/merge-requests/9/head",
            "main",
            &NoCredentials,
        )
        .unwrap_err();
    assert!(e.to_string().contains("refs/merge-requests/9/head"), "{e}");
}
