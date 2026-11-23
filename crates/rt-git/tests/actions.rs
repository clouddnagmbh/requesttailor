// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The Git wizards' actions natively (wave 14, strand G; the same case list as
//! apps/ui/src/commands/mock-git-actions.test.ts): plan changes nothing, every block reason
//! with plain words, apply refuses what plan blocks, the beginner's way with plan and apply
//! only, merges, cherry-picks and reverts semantically for collection files and line by line
//! for other files, remotes and tags.

mod common;
mod wizard;

use common::*;
use rt_app::git::GitPullResult;
use rt_app::git::actions::{GitAction, GitBlockReason};
use rt_git::NoCredentials;
use wizard::*;

#[test]
fn plan_changes_nothing_for_any_action() {
    let f = fixture("plan-nothing");
    f.commit("second", &[("notes.txt", "second\n")]);
    let repo = f.repo();
    apply(
        &repo,
        &act(r#"{"action":"create-branch","name":"side","start":"HEAD"}"#),
    );
    apply(
        &repo,
        &act(r#"{"action":"create-tag","name":"v1","target":"HEAD"}"#),
    );
    f.write("notes.txt", "changed\n");
    apply(&repo, &act(r#"{"action":"stash","message":"wip"}"#));
    f.write(
        "users/get.yml",
        &request("POST", "https://api.test/users/1"),
    );
    f.write("new.txt", "new\n");
    let before = raw_state(f.dir());
    let head = repo.rev_parse("HEAD").unwrap();
    let actions = [
        r#"{"action":"create-branch","name":"x","start":"HEAD","switch":true}"#,
        r#"{"action":"switch","target":"side"}"#,
        r#"{"action":"switch","target":"origin/main"}"#,
        r#"{"action":"rename-branch","from":"side","to":"y"}"#,
        r#"{"action":"delete-branch","name":"side","force":true}"#,
        r#"{"action":"merge","from":"side"}"#,
        r#"{"action":"pull"}"#,
        r#"{"action":"push"}"#,
        r#"{"action":"publish","branch":"side","remote":"origin"}"#,
        r#"{"action":"fetch"}"#,
        r#"{"action":"add-remote","name":"backup","url":"https://example.com/x.git"}"#,
        r#"{"action":"remove-remote","name":"origin"}"#,
        r#"{"action":"rename-remote","from":"origin","to":"up"}"#,
        r#"{"action":"set-remote-url","name":"origin","url":"https://example.com/y.git"}"#,
        r#"{"action":"stash"}"#,
        r#"{"action":"unstash","index":0}"#,
        r#"{"action":"drop-stash","index":0}"#,
        r#"{"action":"reset","target":"HEAD~1","mode":"discard"}"#,
        r#"{"action":"reset","target":"HEAD~1","mode":"keep-changes"}"#,
        r#"{"action":"discard","paths":[]}"#,
        r#"{"action":"create-tag","name":"v2","target":"HEAD","message":"Two"}"#,
        r#"{"action":"delete-tag","name":"v1"}"#,
        r#"{"action":"push-tag","name":"v1","remote":"origin"}"#,
    ];
    for a in actions {
        f.repo().plan(&act(a), false).unwrap();
    }
    for a in [
        GitAction::Revert {
            commit: head.clone(),
        },
        GitAction::CherryPick { commit: head },
    ] {
        f.repo().plan(&a, false).unwrap();
    }
    assert_eq!(raw_state(f.dir()), before);
}

#[test]
fn blocked_with_the_reason_and_plain_words() {
    let f = fixture("blocked");
    let repo = f.repo();
    let cases = [
        (
            r#"{"action":"create-branch","name":"my branch","start":"HEAD"}"#,
            GitBlockReason::InvalidName,
        ),
        (
            r#"{"action":"create-branch","name":"main","start":"HEAD"}"#,
            GitBlockReason::Exists,
        ),
        (
            r#"{"action":"create-branch","name":"x","start":"nope"}"#,
            GitBlockReason::NotFound,
        ),
        (
            r#"{"action":"delete-branch","name":"main"}"#,
            GitBlockReason::CurrentBranch,
        ),
        (
            r#"{"action":"delete-branch","name":"nope"}"#,
            GitBlockReason::NotFound,
        ),
        (
            r#"{"action":"merge","from":"main"}"#,
            GitBlockReason::NothingToDo,
        ),
        (r#"{"action":"push"}"#, GitBlockReason::NothingToDo),
        (r#"{"action":"stash"}"#, GitBlockReason::NothingToDo),
        (
            r#"{"action":"switch","target":"main"}"#,
            GitBlockReason::NothingToDo,
        ),
        (
            r#"{"action":"discard","paths":[]}"#,
            GitBlockReason::NothingToDo,
        ),
        (
            r#"{"action":"unstash","index":0}"#,
            GitBlockReason::NotFound,
        ),
        (
            r#"{"action":"add-remote","name":"origin","url":"x"}"#,
            GitBlockReason::Exists,
        ),
        (
            r#"{"action":"add-remote","name":"other","url":" "}"#,
            GitBlockReason::InvalidName,
        ),
        (
            r#"{"action":"remove-remote","name":"nope"}"#,
            GitBlockReason::NotFound,
        ),
        (
            r#"{"action":"delete-tag","name":"nope"}"#,
            GitBlockReason::NotFound,
        ),
        (
            r#"{"action":"publish","branch":"main","remote":"origin"}"#,
            GitBlockReason::NothingToDo,
        ),
        (
            r#"{"action":"reset","target":"HEAD","mode":"keep-changes"}"#,
            GitBlockReason::NothingToDo,
        ),
    ];
    for (a, want) in cases {
        let p = plan(&repo, &act(a));
        let b = p.blocked.unwrap_or_else(|| panic!("{a} is blocked"));
        assert_eq!(b.reason, want, "{a}");
        assert!(
            !b.message.is_empty() && !b.message.contains("refs/"),
            "{a}: {}",
            b.message
        );
    }

    // Unsaved edits in the app block whatever changes files.
    let p = repo
        .plan(&act(r#"{"action":"switch","target":"HEAD~0"}"#), true)
        .unwrap();
    assert_eq!(p.blocked.unwrap().reason, GitBlockReason::UnsavedChanges);

    // apply refuses with the plan's message.
    let err = repo
        .apply(
            &act(r#"{"action":"create-branch","name":"main","start":"HEAD"}"#),
            &options(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("exists already"), "{err}");

    // Local changes: merging is refused, switching to a branch that changes the file too.
    apply(
        &repo,
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit("other notes", &[("notes.txt", "other\n")]);
    apply(&repo, &act(r#"{"action":"switch","target":"main"}"#));
    f.commit(
        "main get",
        &[("users/get.yml", &request("PUT", "https://api.test/users/1"))],
    );
    f.write("notes.txt", "local\n");
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#)),
        Some(GitBlockReason::LocalChanges)
    );
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"switch","target":"other"}"#)),
        Some(GitBlockReason::LocalChanges)
    );
    f.write("notes.txt", NOTES);

    // Not merged: commits only on `other`; force confirms.
    let p = plan(
        &f.repo(),
        &act(r#"{"action":"delete-branch","name":"other"}"#),
    );
    assert_eq!(p.blocked.unwrap().reason, GitBlockReason::NotMerged);
    let p = plan(
        &f.repo(),
        &act(r#"{"action":"delete-branch","name":"other","force":true}"#),
    );
    assert!(p.blocked.is_none() && p.destructive && p.backup, "{p:?}");
    assert!(
        p.warnings[0].contains("1 commit is only on other"),
        "{:?}",
        p.warnings
    );

    // No upstream, detached.
    apply(&f.repo(), &act(r#"{"action":"switch","target":"other"}"#));
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"push"}"#)),
        Some(GitBlockReason::NoUpstream)
    );
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"pull"}"#)),
        Some(GitBlockReason::NoUpstream)
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"HEAD~1"}"#));
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"merge","from":"main"}"#)),
        Some(GitBlockReason::Detached)
    );
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"revert","commit":"HEAD"}"#)),
        Some(GitBlockReason::Detached)
    );
}

#[test]
fn diverged_rejected_and_in_progress() {
    let f = fixture("diverged");
    // A colleague pushes; we commit too.
    let parent = tmp("diverged-clone");
    let other = rt_git::clone(
        &rt_git::local_url(f.remote.path()),
        parent.path(),
        &NoCredentials,
        &opts(),
    )
    .unwrap();
    set_user(&other, "Bob");
    write(&other, "notes.txt", "bob\n");
    reopen(&other)
        .commit("bob", &paths(&["notes.txt"]), None)
        .unwrap();
    reopen(&other).push(&NoCredentials).unwrap();
    apply(&f.repo(), &act(r#"{"action":"fetch"}"#));
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"push"}"#)),
        Some(GitBlockReason::Rejected)
    );
    f.commit("ada", &[("notes.txt", "ada\n")]);
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"push"}"#)),
        Some(GitBlockReason::Diverged)
    );

    // Pull merges and stops on the conflict in notes.txt: a merge in progress blocks.
    let r = apply(&f.repo(), &act(r#"{"action":"pull"}"#));
    assert_eq!(
        r.outcome,
        Some(GitPullResult::Conflicts {
            paths: paths(&["notes.txt"])
        })
    );
    assert!(r.undo.is_some() && r.reload);
    for a in [
        r#"{"action":"merge","from":"origin/main"}"#,
        r#"{"action":"switch","target":"HEAD~1"}"#,
        r#"{"action":"stash"}"#,
    ] {
        assert_eq!(
            reason(&f.repo(), &act(a)),
            Some(GitBlockReason::InProgress),
            "{a}"
        );
    }
}

#[test]
fn the_beginners_way_with_plan_and_apply_only() {
    let f = fixture("beginner");
    let repo = f.repo();
    let p = plan(
        &repo,
        &act(r#"{"action":"create-branch","name":"feature/login","start":"HEAD","switch":true}"#),
    );
    assert!(p.blocked.is_none());
    assert!(
        p.steps[0].starts_with("Creates the branch feature/login at "),
        "{:?}",
        p.steps
    );
    apply(&repo, &p.action);
    assert_eq!(
        f.repo().status().unwrap().branch.as_deref(),
        Some("feature/login")
    );

    f.commit("Login notes", &[("notes.txt", "login\n")]);
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    assert_eq!(f.read("notes.txt"), NOTES);

    let p = plan(
        &f.repo(),
        &act(r#"{"action":"merge","from":"feature/login"}"#),
    );
    let m = p.merge.clone().unwrap();
    assert!(m.fast_forward);
    assert_eq!(m.commits[0].summary, "Login notes");
    assert_eq!(p.files, paths(&["/notes.txt"]));
    let r = apply(&f.repo(), &p.action);
    assert!(r.reload);
    assert_eq!(r.outcome, Some(GitPullResult::FastForward { commits: 1 }));
    assert_eq!(f.read("notes.txt"), "login\n");
    assert_eq!(f.repo().status().unwrap().ahead, 1);

    let p = plan(&f.repo(), &act(r#"{"action":"push"}"#));
    assert!(p.network && !p.undoable);
    assert!(p.steps.join(" ").contains("cannot be undone"));
    let r = apply(&f.repo(), &p.action);
    assert!(r.undo.is_none());
    assert_eq!(f.repo().status().unwrap().ahead, 0);
    assert_eq!(
        ref_id(f.remote.path(), "refs/heads/main"),
        Some(f.repo().rev_parse("HEAD").unwrap())
    );
}

#[test]
fn publishes_a_branch_and_fetches() {
    let f = fixture("publish");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"feature/login","start":"HEAD","switch":true}"#),
    );
    f.commit("login", &[("notes.txt", "login\n")]);
    let p = plan(
        &f.repo(),
        &act(r#"{"action":"publish","branch":"feature/login","remote":"origin"}"#),
    );
    assert!(p.network && !p.undoable && p.blocked.is_none(), "{p:?}");
    apply(&f.repo(), &p.action);
    assert_eq!(
        f.repo().status().unwrap().upstream.as_deref(),
        Some("origin/feature/login")
    );
    assert!(ref_id(f.remote.path(), "refs/heads/feature/login").is_some());

    // A colleague adds a commit; fetch makes us one behind.
    let parent = tmp("publish-clone");
    let other = rt_git::clone(
        &rt_git::local_url(f.remote.path()),
        parent.path(),
        &NoCredentials,
        &opts(),
    )
    .unwrap();
    set_user(&other, "Bob");
    reopen(&other).checkout("origin/feature/login").unwrap();
    write(&other, "notes.txt", "bob\n");
    reopen(&other)
        .commit("bob", &paths(&["notes.txt"]), None)
        .unwrap();
    reopen(&other).push(&NoCredentials).unwrap();
    assert_eq!(f.repo().status().unwrap().behind, 0);
    apply(&f.repo(), &act(r#"{"action":"fetch"}"#));
    assert_eq!(f.repo().status().unwrap().behind, 1);
    let r = apply(&f.repo(), &act(r#"{"action":"pull"}"#));
    assert_eq!(r.outcome, Some(GitPullResult::FastForward { commits: 1 }));
    assert_eq!(f.read("notes.txt"), "bob\n");
}

/// `other` and `main` each change something since the fork; returns nothing, leaves `main`
/// checked out.
fn fork(f: &Fixture, theirs: &[(&str, &str)], ours: &[(&str, &str)]) {
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit("theirs", theirs);
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit("ours", ours);
}

#[test]
fn merges_fields_of_one_request_without_a_conflict() {
    let f = fixture("merge-fields");
    fork(
        &f,
        &[(
            "users/get.yml",
            &request("POST", "https://api.test/users/1"),
        )],
        &[(
            "users/get.yml",
            &request("GET", "https://api.test/v2/users/1"),
        )],
    );
    let p = plan(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    let m = p.merge.clone().unwrap();
    assert!(!m.fast_forward);
    assert_eq!(m.files.len(), 1, "{m:?}");
    assert_eq!(m.files[0].path, "/users/get.yml");
    assert!(
        m.files[0].semantic && m.files[0].conflicts.is_empty() && m.files[0].clean >= 1,
        "{m:?}"
    );
    assert_eq!(p.refs[0].after.as_deref(), Some(""), "a new commit");
    let r = apply(&f.repo(), &p.action);
    assert!(
        matches!(r.outcome, Some(GitPullResult::Merged { .. })),
        "{r:?}"
    );
    assert_eq!(
        f.read("users/get.yml"),
        request("POST", "https://api.test/v2/users/1")
    );
    assert_eq!(f.repo().log(1).unwrap()[0].summary, "Merge other into main");
}

#[test]
fn a_conflict_shows_in_the_plan_and_stops_in_the_conflict_views() {
    let f = fixture("merge-conflict");
    fork(
        &f,
        &[
            (
                "users/get.yml",
                &request("GET", "https://theirs.test/users/1"),
            ),
            (
                "notes.txt",
                "one\nTHEIRS\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        ],
        &[
            (
                "users/get.yml",
                &request("GET", "https://ours.test/users/1"),
            ),
            (
                "notes.txt",
                "one\nOURS\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        ],
    );
    let p = plan(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    let files = p.merge.unwrap().files;
    let get = files.iter().find(|x| x.path == "/users/get.yml").unwrap();
    assert!(get.semantic);
    assert_eq!(get.conflicts, paths(&["url"]), "{get:?}");
    let notes = files.iter().find(|x| x.path == "/notes.txt").unwrap();
    assert!(!notes.semantic);
    assert_eq!(notes.conflicts.len(), 1);
    assert!(notes.conflicts[0].starts_with("lines "), "{notes:?}");
    assert!(
        p.steps
            .iter()
            .any(|s| s.contains("2 files need your decision")),
        "{:?}",
        p.steps
    );

    let r = apply(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    assert_eq!(
        r.outcome,
        Some(GitPullResult::Conflicts {
            paths: paths(&["notes.txt", "users/get.yml"])
        })
    );
    let repo = f.repo();
    assert!(repo.status().unwrap().merging);
    let state = repo.merge_state().unwrap();
    assert!(state.merging);
    repo.resolve("notes.txt", rt_app::git::GitResolution::Theirs)
        .unwrap();
    repo.resolve("users/get.yml", rt_app::git::GitResolution::Ours)
        .unwrap();
    repo.commit("Merge other into main", &[], None).unwrap();
    assert!(!f.repo().status().unwrap().merging);
}

#[test]
fn cherry_picks_and_reverts_semantically_and_line_by_line() {
    let f = fixture("pick");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    let picked = f.commit(
        "Page 2",
        &[
            (
                "users/get.yml",
                &request("GET", "https://api.test/users/1?page=2"),
            ),
            (
                "notes.txt",
                "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
            ),
        ],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    let mine = f.commit(
        "Mine",
        &[
            (
                "users/get.yml",
                &request("PATCH", "https://api.test/users/1"),
            ),
            (
                "notes.txt",
                "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        ],
    );

    let pick = GitAction::CherryPick {
        commit: picked.clone(),
    };
    let p = plan(&f.repo(), &pick);
    let files = p.merge.clone().unwrap().files;
    assert_eq!(
        files
            .iter()
            .map(|x| (x.path.as_str(), x.semantic, x.conflicts.len()))
            .collect::<Vec<_>>(),
        [("/notes.txt", false, 0), ("/users/get.yml", true, 0)]
    );
    let r = apply(&f.repo(), &pick);
    assert!(
        matches!(r.outcome, Some(GitPullResult::Merged { .. })),
        "{r:?}"
    );
    assert_eq!(
        f.read("users/get.yml"),
        request("PATCH", "https://api.test/users/1?page=2")
    );
    assert_eq!(
        f.read("notes.txt"),
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n"
    );
    let log = f.repo().log(3).unwrap();
    assert_eq!(log[0].summary, "Page 2");
    assert_eq!(log[0].author, "Ada");
    let again = plan(&f.repo(), &pick);
    assert_eq!(
        again.blocked.as_ref().map(|b| b.reason),
        Some(GitBlockReason::NothingToDo),
        "{again:#?}"
    );

    apply(&f.repo(), &GitAction::Revert { commit: mine });
    assert_eq!(
        f.read("users/get.yml"),
        request("GET", "https://api.test/users/1?page=2")
    );
    assert_eq!(
        f.read("notes.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n"
    );
    assert_eq!(f.repo().log(1).unwrap()[0].summary, "Revert \"Mine\"");
    // One parent each.
    let head = gix::open_opts(f.dir(), gix::open::Options::isolated()).unwrap();
    let id = head.head_id().unwrap();
    assert_eq!(head.find_commit(id).unwrap().parent_ids().count(), 1);
    assert_eq!(
        reason(&f.repo(), &act(r#"{"action":"revert","commit":"HEAD~4"}"#)),
        Some(GitBlockReason::NotFound)
    );
}

#[test]
fn a_conflicting_cherry_pick_stops_and_concludes_with_one_parent() {
    let f = fixture("pick-conflict");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    let picked = f.commit(
        "theirs",
        &[(
            "users/get.yml",
            &request("GET", "https://theirs.test/users/1"),
        )],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit(
        "ours",
        &[(
            "users/get.yml",
            &request("GET", "https://ours.test/users/1"),
        )],
    );
    let r = apply(&f.repo(), &GitAction::CherryPick { commit: picked });
    assert_eq!(
        r.outcome,
        Some(GitPullResult::Conflicts {
            paths: paths(&["users/get.yml"])
        })
    );
    assert!(f.dir().join(".git/CHERRY_PICK_HEAD").exists());
    assert!(!f.dir().join(".git/MERGE_HEAD").exists());
    let repo = f.repo();
    assert!(repo.status().unwrap().merging);
    assert!(repo.merge_state().unwrap().merging);
    repo.resolve("users/get.yml", rt_app::git::GitResolution::Theirs)
        .unwrap();
    repo.commit("theirs", &[], None).unwrap();
    assert!(!f.dir().join(".git/CHERRY_PICK_HEAD").exists());
    let g = gix::open_opts(f.dir(), gix::open::Options::isolated()).unwrap();
    assert_eq!(
        g.find_commit(g.head_id().unwrap())
            .unwrap()
            .parent_ids()
            .count(),
        1
    );
    assert_eq!(
        f.read("users/get.yml"),
        request("GET", "https://theirs.test/users/1")
    );
}

#[test]
fn merges_collections_anywhere_in_the_repository() {
    // The collection lives in a subfolder; another file sits at the root.
    let remote = tmp("nested-remote");
    bare(remote.path());
    let work = tmp("nested-work");
    new_repo(work.path(), "Ada");
    add_remote(work.path(), remote.path().to_str().unwrap());
    write(work.path(), "api/opencollection.yml", COLLECTION);
    write(
        work.path(),
        "api/users/get.yml",
        &request("GET", "https://api.test/users/1"),
    );
    write(work.path(), "README.md", "readme\n");
    // Open the repository from the root: the collection is not its scope.
    let root = reopen(work.path());
    root.commit(
        "init",
        &paths(&["api/opencollection.yml", "api/users/get.yml", "README.md"]),
        None,
    )
    .unwrap();
    let f = Fixture { remote, work };
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit(
        "POST",
        &[(
            "api/users/get.yml",
            &request("POST", "https://api.test/users/1"),
        )],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit(
        "v2",
        &[(
            "api/users/get.yml",
            &request("GET", "https://api.test/v2/users/1"),
        )],
    );
    let p = plan(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    let files = p.merge.unwrap().files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "/api/users/get.yml");
    assert!(
        files[0].semantic && files[0].conflicts.is_empty(),
        "{files:?}"
    );
    apply(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    assert_eq!(
        f.read("api/users/get.yml"),
        request("POST", "https://api.test/v2/users/1")
    );
}

#[test]
fn remotes_and_tags() {
    let f = fixture("remotes");
    let repo = f.repo();
    let url = f.remote.path().to_str().unwrap().replace('\\', "/");
    assert_eq!(
        repo.remotes()
            .unwrap()
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["origin"]
    );
    apply(
        &repo,
        &act(
            r#"{"action":"add-remote","name":"backup","url":"https://ada:secret@example.com/acme/api.git"}"#,
        ),
    );
    let list = f.repo().remotes().unwrap();
    assert_eq!(list[0].name, "backup");
    assert_eq!(
        list[0].url, "https://example.com/acme/api.git",
        "no credentials"
    );
    apply(
        &f.repo(),
        &act(
            r#"{"action":"set-remote-url","name":"backup","url":"https://example.com/acme/other.git"}"#,
        ),
    );
    assert_eq!(
        f.repo().remotes().unwrap()[0].url,
        "https://example.com/acme/other.git"
    );
    apply(
        &f.repo(),
        &act(r#"{"action":"remove-remote","name":"backup"}"#),
    );
    assert_eq!(f.repo().remotes().unwrap().len(), 1);

    // Renaming origin moves its remote-tracking branches and the upstream.
    apply(
        &f.repo(),
        &act(r#"{"action":"rename-remote","from":"origin","to":"upstream"}"#),
    );
    assert_eq!(f.repo().remotes().unwrap()[0].name, "upstream");
    assert_eq!(
        f.repo().status().unwrap().upstream.as_deref(),
        Some("upstream/main")
    );
    assert!(ref_id(f.dir(), "refs/remotes/upstream/main").is_some());
    assert!(ref_id(f.dir(), "refs/remotes/origin/main").is_none());
    apply(
        &f.repo(),
        &act(r#"{"action":"rename-remote","from":"upstream","to":"origin"}"#),
    );
    assert_eq!(f.repo().remotes().unwrap()[0].url, url);

    // An annotated tag, pushed; a second push has nothing to do; delete and undo.
    apply(
        &f.repo(),
        &act(r#"{"action":"create-tag","name":"v1","target":"HEAD","message":"First"}"#),
    );
    let head = f.repo().rev_parse("HEAD").unwrap();
    assert_eq!(f.repo().rev_parse("v1").unwrap(), head);
    let raw = |dir: &std::path::Path| {
        let g = gix::open_opts(dir, gix::open::Options::isolated()).unwrap();
        let r = g.find_reference("refs/tags/v1").unwrap();
        r.target().try_id().map(|id| id.to_string())
    };
    let tag_object = raw(f.dir()).unwrap();
    assert_ne!(tag_object, head, "an annotated tag object");
    let p = plan(
        &f.repo(),
        &act(r#"{"action":"push-tag","name":"v1","remote":"origin"}"#),
    );
    assert!(p.network && !p.undoable && p.blocked.is_none(), "{p:?}");
    apply(&f.repo(), &p.action);
    assert_eq!(
        ref_id(f.remote.path(), "refs/tags/v1"),
        Some(head.clone()),
        "peeled to the commit"
    );
    assert_eq!(
        reason(&f.repo(), &p.action),
        Some(GitBlockReason::NothingToDo)
    );
    let p = plan(&f.repo(), &act(r#"{"action":"delete-tag","name":"v1"}"#));
    assert_eq!(p.warnings, ["The server keeps its copy of the tag."]);
    let del = apply(&f.repo(), &p.action);
    assert!(f.repo().rev_parse("v1").is_err());
    f.repo().undo(&del.undo.unwrap().id, false).unwrap();
    assert_eq!(f.repo().rev_parse("v1").unwrap(), head);
    assert_eq!(raw(f.dir()), Some(tag_object), "annotated again");
}

#[test]
fn switching_to_a_remote_branch_creates_a_tracking_branch() {
    let f = fixture("track");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"feature","start":"HEAD","switch":true}"#),
    );
    f.commit("feature", &[("notes.txt", "feature\n")]);
    apply(
        &f.repo(),
        &act(r#"{"action":"publish","branch":"feature","remote":"origin"}"#),
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    apply(
        &f.repo(),
        &act(r#"{"action":"delete-branch","name":"feature"}"#),
    );
    let p = plan(
        &f.repo(),
        &act(r#"{"action":"switch","target":"origin/feature"}"#),
    );
    assert!(
        p.steps[0].contains("Creates the branch feature from origin/feature"),
        "{:?}",
        p.steps
    );
    apply(&f.repo(), &p.action);
    let s = f.repo().status().unwrap();
    assert_eq!(
        (s.branch.as_deref(), s.upstream.as_deref()),
        (Some("feature"), Some("origin/feature"))
    );
    assert_eq!(f.read("notes.txt"), "feature\n");
    // Detached: a tag.
    apply(
        &f.repo(),
        &act(r#"{"action":"create-tag","name":"t","target":"HEAD~1"}"#),
    );
    let p = plan(&f.repo(), &act(r#"{"action":"switch","target":"t"}"#));
    assert_eq!(p.warnings, ["t is not a branch."]);
    let r = apply(&f.repo(), &p.action);
    assert!(r.message.contains("not on a branch"), "{}", r.message);
    assert_eq!(f.repo().status().unwrap().branch, None);
    assert_eq!(f.read("notes.txt"), NOTES);
}
