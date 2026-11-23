// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The commit building blocks (wave 15, strand D): `commit_info` reads a commit in full;
//! `pick` gives the tree `git cherry-pick`/`git revert` give (compared with the Git command
//! line when it is installed), merging collection files on the model; `write_tree` and
//! `write_commit` write objects and move no ref; `resolve_text` writes a conflict's result
//! and the next commit concludes the merge.

mod common;
mod wizard;

use std::path::Path;
use std::process::Command;

use common::*;
use rt_app::git::GitFileState;
use rt_app::git::plumbing::{
    GitCommitWrite, GitPickRequest, GitSignature, GitTreeContent, GitTreeFile, GitTreeWrite,
};
use wizard::*;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// `git` that may fail: its success and output.
fn try_git(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("HOME", dir)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn pick_req(onto: &str, commit: &str, revert: bool) -> GitPickRequest {
    GitPickRequest {
        onto: onto.into(),
        commit: commit.into(),
        revert,
        mainline: None,
    }
}

/// `main` and `other` diverge from "init": `other` changes the first line of the notes and
/// adds a request; `main` changes the last line.
fn diverged(name: &str) -> Fixture {
    let f = fixture(name);
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit(
        "other",
        &[
            ("notes.txt", &NOTES.replacen("one", "ONE", 1)),
            ("users/new.yml", &request("GET", "https://api.test/new")),
        ],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit("main", &[("notes.txt", &NOTES.replacen("ten", "TEN", 1))]);
    f
}

#[test]
fn commit_info_reads_a_commit_in_full() {
    let f = fixture("plumbing-info");
    let id = f.commit("second\n\nbody line", &[("notes.txt", "changed\n")]);
    let info = f.repo().commit_info("HEAD").unwrap();
    assert_eq!(info.oid, id);
    assert_eq!(info.parents.len(), 1);
    assert_eq!(info.message, "second\n\nbody line");
    assert_eq!(info.author.name, "Ada");
    assert_eq!(info.author.email, "ada@example.com");
    assert_eq!(info.committer.name, "Ada");
    assert!(!info.signed);
    let first = f.repo().commit_info("HEAD~1").unwrap();
    assert_eq!(info.parents, vec![first.oid.clone()]);
    assert!(first.parents.is_empty());
    assert!(f.repo().commit_info("nope").is_err());
    if have_git() {
        let (_, tree) = try_git(f.dir(), &["rev-parse", "HEAD^{tree}"]);
        assert_eq!(info.tree, tree.trim());
        let (_, time) = try_git(f.dir(), &["log", "-1", "--format=%at"]);
        assert_eq!(info.author.time.to_string(), time.trim());
    }
}

#[test]
fn pick_gives_the_tree_of_git_cherry_pick() {
    if !have_git() {
        return;
    }
    let f = diverged("plumbing-pick");
    let before = raw_state(f.dir());
    let result = f.repo().pick(&pick_req("main", "other", false)).unwrap();
    assert!(result.clean, "{result:?}");
    assert!(result.conflicts.is_empty() && result.other_conflicts.is_empty());
    // Nothing moved: no ref, no file, no index.
    assert_eq!(raw_state(f.dir()), before);

    let twin = tmp("plumbing-pick-twin");
    copy_dir(f.dir(), twin.path());
    let (ok, _) = try_git(twin.path(), &["cherry-pick", "other"]);
    assert!(ok);
    let (_, tree) = try_git(twin.path(), &["rev-parse", "HEAD^{tree}"]);
    assert_eq!(result.tree, tree.trim());
}

#[test]
fn pick_with_revert_gives_the_tree_of_git_revert() {
    if !have_git() {
        return;
    }
    let f = fixture("plumbing-revert");
    let target = f.commit(
        "change line one",
        &[("notes.txt", &NOTES.replacen("one", "ONE", 1))],
    );
    f.commit(
        "change line ten",
        &[(
            "notes.txt",
            &NOTES.replacen("one", "ONE", 1).replacen("ten", "TEN", 1),
        )],
    );
    f.commit(
        "add",
        &[("users/new.yml", &request("GET", "https://api.test/new"))],
    );
    let result = f.repo().pick(&pick_req("HEAD", &target, true)).unwrap();
    assert!(result.clean, "{result:?}");
    let twin = tmp("plumbing-revert-twin");
    copy_dir(f.dir(), twin.path());
    let (ok, _) = try_git(twin.path(), &["revert", "--no-edit", &target]);
    assert!(ok);
    let (_, tree) = try_git(twin.path(), &["rev-parse", "HEAD^{tree}"]);
    assert_eq!(result.tree, tree.trim());
}

#[test]
fn pick_merges_collection_files_on_the_model() {
    // One side changes the method, the other the URL of the same request: neighbouring lines
    // conflict for a text merge, the model merges them.
    let f = fixture("plumbing-semantic");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit(
        "method",
        &[(
            "users/get.yml",
            &request("POST", "https://api.test/users/1"),
        )],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit(
        "url",
        &[("users/get.yml", &request("GET", "https://api.test/users/2"))],
    );
    let result = f.repo().pick(&pick_req("main", "other", false)).unwrap();
    assert!(result.clean, "{result:?}");
    let merged = f
        .repo()
        .write_tree(&GitTreeWrite {
            base: result.tree.clone(),
            files: vec![],
        })
        .unwrap();
    assert_eq!(merged, result.tree);
    let text = shown(&f, &result.tree, "users/get.yml");
    assert!(
        text.contains("method: POST") && text.contains("users/2"),
        "{text}"
    );
}

/// A file of a tree, via a commit that holds it.
fn shown(f: &Fixture, tree: &str, path: &str) -> String {
    let commit = f
        .repo()
        .write_commit(
            &GitCommitWrite {
                tree: tree.into(),
                parents: vec![],
                message: "probe".into(),
                author: None,
                committer: None,
            },
            None,
        )
        .unwrap();
    f.repo()
        .show(&commit, &format!("/{path}"))
        .unwrap()
        .unwrap()
}

#[test]
fn pick_reports_a_text_conflict_with_our_version_in_the_tree() {
    let f = fixture("plumbing-conflict");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit(
        "theirs",
        &[("notes.txt", &NOTES.replacen("five", "5 theirs", 1))],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit(
        "ours",
        &[("notes.txt", &NOTES.replacen("five", "5 ours", 1))],
    );
    let result = f.repo().pick(&pick_req("main", "other", false)).unwrap();
    assert!(!result.clean);
    assert!(result.other_conflicts.is_empty());
    assert_eq!(result.conflicts.len(), 1);
    let c = &result.conflicts[0];
    assert_eq!(c.path, "/notes.txt");
    assert_eq!(c.base.as_deref(), Some(NOTES));
    assert_eq!(
        c.ours.as_deref(),
        Some(NOTES.replacen("five", "5 ours", 1).as_str())
    );
    assert_eq!(
        c.theirs.as_deref(),
        Some(NOTES.replacen("five", "5 theirs", 1).as_str())
    );
    // Our version is in the tree: no markers.
    assert_eq!(
        shown(&f, &result.tree, "notes.txt"),
        c.ours.clone().unwrap()
    );
    if have_git() {
        let twin = tmp("plumbing-conflict-twin");
        copy_dir(f.dir(), twin.path());
        let (ok, _) = try_git(twin.path(), &["cherry-pick", "other"]);
        assert!(!ok);
        let (_, names) = try_git(twin.path(), &["diff", "--name-only", "--diff-filter=U"]);
        assert_eq!(names.trim(), "notes.txt");
    }
}

#[test]
fn pick_names_conflicts_that_are_not_text() {
    let f = fixture("plumbing-binary");
    f.commit("binary", &[("image.bin", "\u{0}\u{1}A")]);
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD~1","switch":true}"#),
    );
    f.commit("theirs", &[("image.bin", "other")]);
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    f.commit("ours", &[("image.bin", "\u{0}\u{1}B")]);
    let result = f.repo().pick(&pick_req("main", "other", false)).unwrap();
    assert!(!result.clean);
    // Added by both with different content, one of them binary: not a text conflict.
    assert!(
        result
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .chain(result.other_conflicts.iter().map(String::as_str))
            .any(|p| p == "/image.bin"),
        "{result:?}"
    );
}

#[test]
fn write_tree_and_commit_move_no_ref() {
    let f = fixture("plumbing-write");
    let first = f.repo().rev_parse("HEAD").unwrap();
    f.write("loose.txt", "untracked\n");
    let before = raw_state(f.dir());
    let repo = f.repo();
    let tree = repo
        .write_tree(&GitTreeWrite {
            base: "HEAD".into(),
            files: vec![
                GitTreeFile {
                    path: "/notes.txt".into(),
                    content: GitTreeContent::Text {
                        text: "new notes\n".into(),
                    },
                    executable: None,
                },
                GitTreeFile {
                    path: "/bin/run.sh".into(),
                    content: GitTreeContent::Base64 {
                        data: "IyEvYmluL3NoCg==".into(),
                    },
                    executable: Some(true),
                },
                GitTreeFile {
                    path: "/users/copy.yml".into(),
                    content: GitTreeContent::From { rev: "HEAD".into() },
                    executable: None,
                },
                GitTreeFile {
                    path: "/opencollection.yml".into(),
                    content: GitTreeContent::Delete,
                    executable: None,
                },
            ],
        })
        .unwrap();
    // `From` of a path that does not exist there removes it again.
    let tree2 = repo
        .write_tree(&GitTreeWrite {
            base: tree.clone(),
            files: vec![GitTreeFile {
                path: "/bin/run.sh".into(),
                content: GitTreeContent::From { rev: "HEAD".into() },
                executable: None,
            }],
        })
        .unwrap();
    let author = GitSignature {
        name: "Bea".into(),
        email: "bea@example.com".into(),
        time: 1_700_000_000,
        offset: 60,
    };
    let commit = repo
        .write_commit(
            &GitCommitWrite {
                tree: tree.clone(),
                parents: vec![first.clone()],
                message: "built by hand\n".into(),
                author: Some(author.clone()),
                committer: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(raw_state(f.dir()), before, "no ref, file or index moved");
    let info = repo.commit_info(&commit).unwrap();
    assert_eq!(info.tree, tree);
    assert_eq!(info.parents, vec![first]);
    assert_eq!(info.author, author);
    assert_eq!(info.committer.name, "Ada");
    assert_eq!(info.message, "built by hand\n");
    assert_ne!(tree, tree2);
    if have_git() {
        let (_, listing) = try_git(f.dir(), &["ls-tree", "-r", &tree]);
        assert!(
            listing.contains("100755 blob") && listing.contains("\tbin/run.sh"),
            "{listing}"
        );
        assert!(
            listing.contains("\tusers/copy.yml") || !listing.contains("copy"),
            "{listing}"
        );
        assert!(!listing.contains("opencollection.yml"), "{listing}");
        let (_, text) = try_git(f.dir(), &["cat-file", "-p", &format!("{tree}:notes.txt")]);
        assert_eq!(text, "new notes\n");
        let (_, listing2) = try_git(f.dir(), &["ls-tree", "-r", "--name-only", &tree2]);
        assert!(!listing2.contains("bin/run.sh"), "{listing2}");
        let (ok, _) = try_git(f.dir(), &["fsck", "--no-dangling"]);
        assert!(ok);
        let (_, header) = try_git(
            f.dir(),
            &["log", "-1", "--format=%an <%ae> %at %ai", &commit],
        );
        assert!(
            header.starts_with("Bea <bea@example.com> 1700000000 "),
            "{header}"
        );
        assert!(header.contains("+0100"), "{header}");
    }
    // Bad input is refused.
    assert!(
        repo.write_tree(&GitTreeWrite {
            base: "HEAD".into(),
            files: vec![GitTreeFile {
                path: "/../x".into(),
                content: GitTreeContent::Delete,
                executable: None
            }]
        })
        .is_err()
    );
    assert!(
        repo.write_commit(
            &GitCommitWrite {
                tree: "0123456789012345678901234567890123456789".into(),
                parents: vec![],
                message: "x".into(),
                author: None,
                committer: None
            },
            None
        )
        .is_err()
    );
}

#[test]
fn resolve_text_ends_a_text_conflict_and_the_next_commit_concludes_the_merge() {
    let f = fixture("plumbing-resolve");
    apply(
        &f.repo(),
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD","switch":true}"#),
    );
    f.commit(
        "theirs",
        &[("notes.txt", &NOTES.replacen("five", "5 theirs", 1))],
    );
    apply(&f.repo(), &act(r#"{"action":"switch","target":"main"}"#));
    let ours = f.commit(
        "ours",
        &[("notes.txt", &NOTES.replacen("five", "5 ours", 1))],
    );
    let r = apply(&f.repo(), &act(r#"{"action":"merge","from":"other"}"#));
    assert!(r.reload);
    let status = f.repo().status().unwrap();
    assert!(status.merging);
    assert!(
        status
            .files
            .iter()
            .any(|x| x.path == "notes.txt" && x.state == GitFileState::Conflicted)
    );
    assert!(
        f.repo().resolve_text("/users/get.yml", "x").is_err(),
        "no conflict there"
    );

    let resolved = NOTES.replacen("five", "5 both", 1);
    f.repo().resolve_text("/notes.txt", &resolved).unwrap();
    assert_eq!(f.read("notes.txt"), resolved);
    assert!(
        f.repo()
            .status()
            .unwrap()
            .files
            .iter()
            .all(|x| x.state != GitFileState::Conflicted)
    );
    let c = f.repo().commit("Merge other", &[], None).unwrap();
    let info = f.repo().commit_info(&c.oid).unwrap();
    assert_eq!(info.parents.len(), 2);
    assert_eq!(info.parents[0], ours);
    assert!(!f.dir().join(".git/MERGE_HEAD").exists());
    assert_eq!(f.read("notes.txt"), resolved);
    assert!(f.repo().status().unwrap().files.is_empty());
    if have_git() {
        let (_, show) = try_git(f.dir(), &["show", &format!("{}:notes.txt", c.oid)]);
        assert_eq!(show, resolved);
        let (_, status) = try_git(f.dir(), &["status", "--porcelain"]);
        assert!(status.trim().is_empty(), "{status}");
    }
}
