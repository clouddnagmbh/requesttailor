// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! rt-git over smart HTTP against a local server: `git http-backend` behind a small test
//! server on 127.0.0.1 (ports 47821–47829). Fetch goes through gitoxide, push through our own
//! send-pack client. Skipped with a message when `git` is not installed; the code under test
//! never runs `git`, only the test server does. `git fsck` checks what we pushed.

mod common;

use std::path::Path;
use std::process::Command;

use common::*;
use rt_app::AppError;
use rt_app::git::GitPullResult;
use rt_git::{NoCredentials, StoreCredentials};

/// A bare repository `name` served by `server`.
fn served_bare(server: &GitHttp, name: &str) -> std::path::PathBuf {
    let dir = server.root.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    bare(&dir);
    dir
}

/// `git status` agrees with us: nothing to commit (our index is what Git expects).
fn git_clean(work: &Path) {
    let out = Command::new("git")
        .arg("-C")
        .arg(work)
        .args(["status", "--porcelain"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "",
        "git status in {}",
        work.display()
    );
}

fn fsck(bare: &Path) {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["fsck", "--strict", "--no-dangling"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git fsck failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn work_repo(name: &str, url: &str) -> Tmp {
    let work = tmp(name);
    new_repo(work.path(), "Ada");
    add_remote(work.path(), url);
    write(work.path(), "opencollection.yml", "name: over http\n");
    write(work.path(), "users/get.yml", "one\ntwo\nthree\n");
    reopen(work.path())
        .commit(
            "init",
            &paths(&["opencollection.yml", "users/get.yml"]),
            None,
        )
        .unwrap();
    work
}

#[test]
fn push_clone_and_pull_without_credentials() {
    let server = require_git_http!();
    let bare_dir = served_bare(server, "open-flow.git");
    let url = server.url("open-flow.git", false);
    let work = work_repo("http-open-a", &url);
    let a = reopen(work.path());

    a.push(&NoCredentials).unwrap();
    assert_eq!(
        ref_id(&bare_dir, "refs/heads/main"),
        Some(a.log(1).unwrap()[0].oid.clone())
    );
    fsck(&bare_dir);
    let a = reopen(work.path());
    assert_eq!(a.status().unwrap().upstream.as_deref(), Some("origin/main"));

    let parent = tmp("http-open-b");
    let b_dir = rt_git::clone(&url, parent.path(), &NoCredentials, &opts()).unwrap();
    assert_eq!(b_dir.file_name().unwrap(), "open-flow");
    set_user(&b_dir, "Bob");
    assert_eq!(read(&b_dir, "users/get.yml"), "one\ntwo\nthree\n");
    let b = reopen(&b_dir);
    git_clean(&b_dir);
    assert_eq!(b.status().unwrap().branch.as_deref(), Some("main"));

    // Fast-forward over HTTP; the second push only sends the new objects.
    write(work.path(), "users/get.yml", "one\ntwo by Ada\nthree\n");
    a.commit("edit", &paths(&["users/get.yml"]), None).unwrap();
    a.push(&NoCredentials).unwrap();
    fsck(&bare_dir);
    assert_eq!(
        b.pull(&NoCredentials, None).unwrap(),
        GitPullResult::FastForward { commits: 1 }
    );
    assert_eq!(read(&b_dir, "users/get.yml"), "one\ntwo by Ada\nthree\n");

    // Diverged: pushing is refused, pull merges, the merge commit pushes.
    write(work.path(), "a.yml", "Ada\n");
    a.commit("Ada", &paths(&["a.yml"]), None).unwrap();
    a.push(&NoCredentials).unwrap();
    write(&b_dir, "b.yml", "Bob\n");
    b.commit("Bob", &paths(&["b.yml"]), None).unwrap();
    let err = b.push(&NoCredentials).map_err(AppError::from).unwrap_err();
    assert!(
        matches!(&err, AppError::Git(m) if m.contains("pull first")),
        "{err:?}"
    );
    assert!(matches!(
        b.pull(&NoCredentials, None).unwrap(),
        GitPullResult::Merged { .. }
    ));
    git_clean(&b_dir);
    b.push(&NoCredentials).unwrap();
    fsck(&bare_dir);
    assert_eq!(
        ref_id(&bare_dir, "refs/heads/main"),
        ref_id(&b_dir, "refs/heads/main")
    );

    // A new branch pointing at a commit the remote already has: an empty pack.
    b.create_branch("same", true).unwrap();
    reopen(&b_dir).push(&NoCredentials).unwrap();
    assert_eq!(
        ref_id(&bare_dir, "refs/heads/same"),
        ref_id(&b_dir, "refs/heads/main")
    );
    fsck(&bare_dir);
}

#[test]
fn credentials_are_asked_for_and_used() {
    let server = require_git_http!();
    let bare_dir = served_bare(server, "auth-flow.git");
    let url = server.url("auth-flow.git", true);
    let work = work_repo("http-auth-a", &url);
    let a = reopen(work.path());
    let host = server.host();

    // No credentials: the remote's 401 becomes GitAuth(host), for push and for fetch.
    assert_eq!(
        a.push(&NoCredentials).map_err(AppError::from).unwrap_err(),
        AppError::GitAuth(host.clone())
    );
    assert_eq!(
        a.pull(&NoCredentials, None)
            .map_err(AppError::from)
            .unwrap_err(),
        AppError::GitAuth(host.clone())
    );
    let parent = tmp("http-auth-b");
    assert_eq!(
        rt_git::clone(&url, parent.path(), &NoCredentials, &opts())
            .map_err(AppError::from)
            .unwrap_err(),
        AppError::GitAuth(host.clone())
    );
    assert!(
        !parent.path().join("auth-flow").exists(),
        "a failed clone leaves nothing behind"
    );

    // Wrong credentials: the same.
    let store = MemorySecrets::default();
    rt_git::credentials::set(&store, &host, USER, "wrong").unwrap();
    assert_eq!(
        a.push(&StoreCredentials(&store))
            .map_err(AppError::from)
            .unwrap_err(),
        AppError::GitAuth(host.clone())
    );
    assert_eq!(
        a.pull(&StoreCredentials(&store), None)
            .map_err(AppError::from)
            .unwrap_err(),
        AppError::GitAuth(host.clone())
    );

    // Right credentials: push, clone and pull work.
    rt_git::credentials::set(&store, &host, USER, PASSWORD).unwrap();
    a.push(&StoreCredentials(&store)).unwrap();
    fsck(&bare_dir);
    let b_dir = rt_git::clone(&url, parent.path(), &StoreCredentials(&store), &opts()).unwrap();
    assert_eq!(read(&b_dir, "opencollection.yml"), "name: over http\n");
    write(work.path(), "users/get.yml", "changed\n");
    a.commit("change", &paths(&["users/get.yml"]), None)
        .unwrap();
    a.push(&StoreCredentials(&store)).unwrap();
    assert_eq!(
        reopen(&b_dir)
            .pull(&StoreCredentials(&store), None)
            .unwrap(),
        GitPullResult::FastForward { commits: 1 }
    );
    // The URL in the repository configuration never holds the password.
    let config = std::fs::read_to_string(b_dir.join(".git/config")).unwrap();
    assert!(!config.contains(PASSWORD));
}

#[test]
fn credentials_never_go_over_plain_http_to_other_hosts() {
    // No server needed: the refusal happens before anything is sent with credentials.
    let work = tmp("http-insecure");
    new_repo(work.path(), "Ada");
    add_remote(work.path(), "http://git.example.invalid/repo.git");
    let store = MemorySecrets::default();
    rt_git::credentials::set(&store, "git.example.invalid", "ada", "pw").unwrap();
    write(work.path(), "a.yml", "a\n");
    let repo = reopen(work.path());
    repo.commit("a", &paths(&["a.yml"]), None).unwrap();
    // The host does not resolve; what matters is that no credentials are offered.
    let err = repo
        .push(&StoreCredentials(&store))
        .map_err(AppError::from)
        .unwrap_err();
    assert!(!matches!(err, AppError::GitAuth(_)), "{err:?}");

    let ssh = tmp("ssh");
    new_repo(ssh.path(), "Ada");
    add_remote(ssh.path(), "ssh://git@example.invalid/repo.git");
    let repo = reopen(ssh.path());
    write(ssh.path(), "a.yml", "a\n");
    repo.commit("a", &paths(&["a.yml"]), None).unwrap();
    assert!(matches!(
        repo.push(&NoCredentials).map_err(AppError::from),
        Err(AppError::Unsupported(_))
    ));
}
