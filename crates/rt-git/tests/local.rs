// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! rt-git against local repositories: a bare remote reached through a path or `file://`,
//! no network and no `git` program.

mod common;

use common::*;
use rt_app::AppError;
use rt_app::git::{GitFile, GitFileState, GitIdentity, GitPullResult, GitResolution};
use rt_git::{NoCredentials, Repo};

fn files(repo: &Repo) -> Vec<(String, GitFileState)> {
    repo.status()
        .unwrap()
        .files
        .into_iter()
        .map(|GitFile { path, state }| (path, state))
        .collect()
}

fn git_err(e: AppError) -> String {
    match e {
        AppError::Git(m) => m,
        other => panic!("expected AppError::Git, got {other:?}"),
    }
}

#[test]
fn not_a_repository() {
    let dir = tmp("norepo");
    let status = rt_git::status(dir.path(), &opts()).unwrap();
    assert!(!status.repo);
    assert!(status.files.is_empty());
    assert!(Repo::open(dir.path(), &opts()).unwrap().is_none());
}

#[test]
fn init_commit_selected_paths_and_log() {
    let dir = tmp("commit");
    let repo = new_repo(dir.path(), "Ada");
    assert!(matches!(
        rt_git::init(dir.path(), &opts()),
        Err(AppError::Git(_))
    ));
    let status = repo.status().unwrap();
    assert!(status.repo);
    assert_eq!(status.branch, None, "no commits yet");
    assert!(repo.log(10).unwrap().is_empty());
    // The unborn branch is listed (the UI shows "main (no commits yet)").
    let branches = repo.branches().unwrap();
    assert_eq!(branches.len(), 1);
    assert_eq!(
        (branches[0].name.as_str(), branches[0].current),
        ("main", true)
    );

    write(dir.path(), "opencollection.yml", "opencollection: 1.0.0\n");
    write(dir.path(), "users/get.yml", "get\n");
    write(dir.path(), "users/post.yml", "post\n");
    assert_eq!(
        files(&repo),
        [
            ("opencollection.yml".into(), GitFileState::Untracked),
            ("users/get.yml".into(), GitFileState::Untracked),
            ("users/post.yml".into(), GitFileState::Untracked),
        ]
    );

    let first = repo
        .commit(
            "Add collection\n\nbody",
            &paths(&["opencollection.yml", "users/get.yml"]),
            None,
        )
        .unwrap();
    assert_eq!(first.summary, "Add collection");
    assert_eq!(first.author, "Ada");
    // Only the selected paths went in.
    assert_eq!(
        files(&repo),
        [("users/post.yml".into(), GitFileState::Untracked)]
    );
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));

    write(dir.path(), "users/get.yml", "get v2\n");
    std::fs::remove_file(dir.path().join("opencollection.yml")).unwrap();
    assert_eq!(
        files(&repo),
        [
            ("opencollection.yml".into(), GitFileState::Deleted),
            ("users/get.yml".into(), GitFileState::Modified),
            ("users/post.yml".into(), GitFileState::Untracked),
        ]
    );
    let diff = repo.diff("users/get.yml").unwrap();
    let lines: Vec<_> = diff.hunks[0]
        .lines
        .iter()
        .map(|l| (l.kind, l.text.as_str()))
        .collect();
    use rt_app::git::DiffLineKind::*;
    assert_eq!(lines, [(Removed, "get"), (Added, "get v2")]);

    let second = repo
        .commit(
            "Delete and change",
            &paths(&["opencollection.yml", "users/get.yml"]),
            None,
        )
        .unwrap();
    assert_eq!(
        files(&repo),
        [("users/post.yml".into(), GitFileState::Untracked)]
    );
    let log = repo.log(10).unwrap();
    assert_eq!(
        log.iter().map(|c| c.oid.clone()).collect::<Vec<_>>(),
        [second.oid, first.oid]
    );
    assert_eq!(repo.log(1).unwrap().len(), 1);

    // Nothing selected, unchanged files, bad paths.
    assert!(git_err(repo.commit("x", &[], None).unwrap_err()).contains("select"));
    assert!(
        git_err(
            repo.commit("x", &paths(&["users/get.yml"]), None)
                .unwrap_err()
        )
        .contains("nothing to commit")
    );
    assert!(
        git_err(repo.commit("x", &paths(&["../escape"]), None).unwrap_err())
            .contains("invalid path")
    );
    assert!(
        git_err(
            repo.commit("  ", &paths(&["users/post.yml"]), None)
                .unwrap_err()
        )
        .contains("message")
    );
}

#[test]
fn identity_from_config_then_fallback() {
    let dir = tmp("identity");
    rt_git::init(dir.path(), &opts()).unwrap();
    let repo = reopen(dir.path());
    assert_eq!(repo.config_identity(), None);
    write(dir.path(), "a.yml", "a\n");
    let err = repo.commit("first", &paths(&["a.yml"]), None).unwrap_err();
    assert!(git_err(err).contains("identity"));
    let app = GitIdentity {
        name: "App User".into(),
        email: "app@example.com".into(),
    };
    let c = repo
        .commit("first", &paths(&["a.yml"]), Some(&app))
        .unwrap();
    assert_eq!(c.author, "App User");

    // Git configuration wins over the app's identity.
    set_user(dir.path(), "Configured");
    let repo = reopen(dir.path());
    assert_eq!(repo.config_identity().unwrap().name, "Configured");
    write(dir.path(), "a.yml", "b\n");
    let c = repo
        .commit("second", &paths(&["a.yml"]), Some(&app))
        .unwrap();
    assert_eq!(c.author, "Configured");

    // The app's identity file.
    let data = tmp("identity-data");
    assert_eq!(rt_git::identity::load(Some(data.path())).unwrap(), None);
    rt_git::identity::save(Some(data.path()), &app).unwrap();
    assert_eq!(
        rt_git::identity::load(Some(data.path())).unwrap(),
        Some(app)
    );
    assert!(
        rt_git::identity::save(
            Some(data.path()),
            &GitIdentity {
                name: "".into(),
                email: "x".into()
            }
        )
        .is_err()
    );
}

#[test]
fn status_is_scoped_to_a_collection_inside_a_larger_repository() {
    let dir = tmp("scoped");
    let top = new_repo(dir.path(), "Ada");
    write(dir.path(), "README.md", "readme\n");
    write(dir.path(), "api/opencollection.yml", "c\n");
    write(dir.path(), "api/users/get.yml", "get\n");
    write(dir.path(), "apiary/other.yml", "not the collection\n");
    write(dir.path(), ".gitignore", "*.log\nbuild/\n");
    top.commit(
        "init",
        &paths(&[
            "README.md",
            "api/opencollection.yml",
            "api/users/get.yml",
            "apiary/other.yml",
            ".gitignore",
        ]),
        None,
    )
    .unwrap();

    let api = reopen(&dir.path().join("api"));
    assert_eq!(api.root(), dir.path().join("api"));
    assert_eq!(api.workdir(), dir.path());
    assert!(files(&api).is_empty());

    write(dir.path(), "README.md", "changed outside\n");
    write(dir.path(), "apiary/other.yml", "changed outside\n");
    write(dir.path(), "outside.txt", "new outside\n");
    write(dir.path(), "api/users/get.yml", "get v2\n");
    write(dir.path(), "api/users/new.yml", "new\n");
    write(dir.path(), "api/debug.log", "ignored\n");
    write(dir.path(), "api/build/out.yml", "ignored folder\n");
    std::fs::remove_file(dir.path().join("api/opencollection.yml")).unwrap();
    assert_eq!(
        files(&api),
        [
            ("opencollection.yml".into(), GitFileState::Deleted),
            ("users/get.yml".into(), GitFileState::Modified),
            ("users/new.yml".into(), GitFileState::Untracked),
        ]
    );

    // Commits from the collection take collection-relative paths and leave the rest alone.
    api.commit(
        "api only",
        &paths(&["users/get.yml", "users/new.yml"]),
        None,
    )
    .unwrap();
    assert_eq!(
        files(&api),
        [("opencollection.yml".into(), GitFileState::Deleted)]
    );
    let top = reopen(dir.path());
    let outside: Vec<_> = files(&top).into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        outside,
        [
            "README.md",
            "api/opencollection.yml",
            "apiary/other.yml",
            "outside.txt"
        ]
    );
    assert!(api.diff("../README.md").is_err());
}

#[test]
fn branches_create_switch_and_refuse_to_lose_work() {
    let dir = tmp("branches");
    let repo = new_repo(dir.path(), "Ada");
    write(dir.path(), "shared.yml", "shared\n");
    write(dir.path(), "main.yml", "main\n");
    write(dir.path(), "notes.txt", "notes\n");
    repo.commit(
        "init",
        &paths(&["shared.yml", "main.yml", "notes.txt"]),
        None,
    )
    .unwrap();
    assert!(git_err(repo.create_branch("bad name", false).unwrap_err()).contains("invalid"));

    repo.create_branch("feature", true).unwrap();
    assert!(git_err(repo.create_branch("feature", false).unwrap_err()).contains("exists"));
    let repo = reopen(dir.path());
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("feature"));
    write(dir.path(), "shared.yml", "shared on feature\n");
    write(dir.path(), "feature/only.yml", "feature\n");
    std::fs::remove_file(dir.path().join("main.yml")).unwrap();
    repo.commit(
        "feature work",
        &paths(&["shared.yml", "feature/only.yml", "main.yml"]),
        None,
    )
    .unwrap();

    let names: Vec<_> = repo
        .branches()
        .unwrap()
        .into_iter()
        .map(|b| (b.name, b.current))
        .collect();
    assert_eq!(names, [("feature".into(), true), ("main".into(), false)]);

    repo.checkout("main").unwrap();
    let repo = reopen(dir.path());
    assert_eq!(read(dir.path(), "shared.yml"), "shared\n");
    assert_eq!(read(dir.path(), "main.yml"), "main\n");
    assert!(
        !dir.path().join("feature").exists(),
        "emptied folders go away"
    );
    assert!(files(&repo).is_empty());

    // A local change to a file that differs between the branches blocks the switch ...
    write(dir.path(), "shared.yml", "local work\n");
    let err = git_err(repo.checkout("feature").unwrap_err());
    assert!(err.contains("shared.yml"), "{err}");
    assert_eq!(read(dir.path(), "shared.yml"), "local work\n");
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
    // ... an untracked file in the way too ...
    write(dir.path(), "shared.yml", "shared\n");
    write(dir.path(), "feature/only.yml", "untracked here\n");
    let err = git_err(repo.checkout("feature").unwrap_err());
    assert!(err.contains("feature/only.yml"), "{err}");
    std::fs::remove_dir_all(dir.path().join("feature")).unwrap();
    // ... but a change to a file both branches share is carried over.
    write(dir.path(), "notes.txt", "local notes\n");
    repo.checkout("feature").unwrap();
    let repo = reopen(dir.path());
    assert_eq!(read(dir.path(), "shared.yml"), "shared on feature\n");
    assert_eq!(read(dir.path(), "notes.txt"), "local notes\n");
    assert!(!dir.path().join("main.yml").exists());
    assert_eq!(files(&repo), [("notes.txt".into(), GitFileState::Modified)]);
    assert!(git_err(repo.checkout("nope").unwrap_err()).contains("no branch"));
}

/// A bare remote and a first clone that pushed `main` to it.
fn remote_with_main(name: &str) -> (Tmp, Tmp, std::path::PathBuf) {
    let remote = tmp(&format!("{name}-remote"));
    bare(remote.path());
    let work = tmp(&format!("{name}-a"));
    new_repo(work.path(), "Ada");
    add_remote(work.path(), remote.path().to_str().unwrap());
    let a = reopen(work.path());
    write(work.path(), "opencollection.yml", "name: shared\n");
    write(work.path(), "users/get.yml", "line 1\nline 2\nline 3\n");
    a.commit(
        "init",
        &paths(&["opencollection.yml", "users/get.yml"]),
        None,
    )
    .unwrap();
    a.push(&NoCredentials).unwrap();
    let a_dir = work.path().to_path_buf();
    (remote, work, a_dir)
}

fn clone_as(remote: &std::path::Path, user: &str, parent: &Tmp) -> std::path::PathBuf {
    let url = rt_git::local_url(remote);
    let dir = rt_git::clone(&url, parent.path(), &NoCredentials, &opts()).unwrap();
    set_user(&dir, user);
    dir
}

#[test]
fn push_sets_the_upstream_and_clone_checks_out() {
    let (remote, _work, a_dir) = remote_with_main("push");
    let a = reopen(&a_dir);
    let status = a.status().unwrap();
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert_eq!(
        ref_id(remote.path(), "refs/heads/main"),
        Some(a.log(1).unwrap()[0].oid.clone())
    );

    let parent = tmp("push-b");
    let b_dir = clone_as(remote.path(), "Bob", &parent);
    assert_eq!(
        b_dir,
        parent.path().join(remote.path().file_name().unwrap())
    );
    assert_eq!(read(&b_dir, "users/get.yml"), "line 1\nline 2\nline 3\n");
    let b = reopen(&b_dir);
    let status = b.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert!(status.files.is_empty());
    let branches: Vec<_> = b
        .branches()
        .unwrap()
        .into_iter()
        .map(|b| (b.name, b.remote))
        .collect();
    assert_eq!(
        branches,
        [("main".into(), false), ("origin/main".into(), true)]
    );

    // Ahead after a local commit; pushing a new branch sets its upstream.
    write(&b_dir, "users/get.yml", "changed\n");
    b.commit("change", &paths(&["users/get.yml"]), None)
        .unwrap();
    assert_eq!(b.status().unwrap().ahead, 1);
    b.create_branch("topic", true).unwrap();
    let b = reopen(&b_dir);
    b.push(&NoCredentials).unwrap();
    assert!(ref_id(remote.path(), "refs/heads/topic").is_some());
    assert_eq!(
        ref_id(remote.path(), "refs/heads/main"),
        ref_id(&a_dir, "refs/heads/main")
    );

    // Cloning into a folder that is not empty is refused.
    let err = rt_git::clone(
        &rt_git::local_url(remote.path()),
        parent.path(),
        &NoCredentials,
        &opts(),
    )
    .unwrap_err();
    assert!(git_err(err).contains("not empty"));
}

#[test]
fn pull_fast_forward_then_merge() {
    let (remote, _work, a_dir) = remote_with_main("pull");
    let parent = tmp("pull-b");
    let b_dir = clone_as(remote.path(), "Bob", &parent);
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));

    assert_eq!(
        b.pull(&NoCredentials, None).unwrap(),
        GitPullResult::UpToDate
    );

    write(&a_dir, "users/post.yml", "post\n");
    a.commit("add post", &paths(&["users/post.yml"]), None)
        .unwrap();
    write(&a_dir, "users/get.yml", "line 1\nline 2 by Ada\nline 3\n");
    a.commit("edit get", &paths(&["users/get.yml"]), None)
        .unwrap();
    a.push(&NoCredentials).unwrap();

    assert_eq!(
        b.pull(&NoCredentials, None).unwrap(),
        GitPullResult::FastForward { commits: 2 }
    );
    assert_eq!(read(&b_dir, "users/post.yml"), "post\n");
    assert!(b.status().unwrap().files.is_empty());

    // Diverge on different files: a merge commit.
    write(&a_dir, "a.yml", "from Ada\n");
    a.commit("Ada's file", &paths(&["a.yml"]), None).unwrap();
    a.push(&NoCredentials).unwrap();
    write(&b_dir, "b.yml", "from Bob\n");
    b.commit("Bob's file", &paths(&["b.yml"]), None).unwrap();
    // Pushing now is refused: the remote has a commit Bob does not have.
    assert!(git_err(b.push(&NoCredentials).unwrap_err()).contains("pull first"));
    let GitPullResult::Merged { oid } = b.pull(&NoCredentials, None).unwrap() else {
        panic!("expected a merge");
    };
    assert_eq!(b.log(1).unwrap()[0].oid, oid);
    assert_eq!(read(&b_dir, "a.yml"), "from Ada\n");
    assert_eq!(read(&b_dir, "b.yml"), "from Bob\n");
    let status = b.status().unwrap();
    assert!(status.files.is_empty());
    assert_eq!((status.ahead, status.behind), (2, 0));
    b.push(&NoCredentials).unwrap();
    assert_eq!(
        a.pull(&NoCredentials, None).unwrap(),
        GitPullResult::FastForward { commits: 2 }
    );
    assert_eq!(read(&a_dir, "b.yml"), "from Bob\n");
}

#[test]
fn pull_with_conflict_resolve_and_commit() {
    let (remote, _work, a_dir) = remote_with_main("conflict");
    let parent = tmp("conflict-b");
    let b_dir = clone_as(remote.path(), "Bob", &parent);
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));

    write(&a_dir, "users/get.yml", "line 1\nline 2 by Ada\nline 3\n");
    write(&a_dir, "ada.yml", "ada\n");
    a.commit("Ada", &paths(&["users/get.yml", "ada.yml"]), None)
        .unwrap();
    a.push(&NoCredentials).unwrap();
    write(&b_dir, "users/get.yml", "line 1\nline 2 by Bob\nline 3\n");
    b.commit("Bob", &paths(&["users/get.yml"]), None).unwrap();

    // Local changes in a file the merge would touch block the pull.
    write(&b_dir, "ada.yml", "untracked in the way\n");
    assert!(git_err(b.pull(&NoCredentials, None).unwrap_err()).contains("ada.yml"));
    std::fs::remove_file(b_dir.join("ada.yml")).unwrap();

    let result = b.pull(&NoCredentials, None).unwrap();
    assert_eq!(
        result,
        GitPullResult::Conflicts {
            paths: paths(&["users/get.yml"])
        }
    );
    let status = b.status().unwrap();
    assert!(status.merging);
    // Against HEAD: the merged-in file is new (tracked by the merge), the other one conflicted.
    assert_eq!(
        status.files,
        [
            GitFile {
                path: "ada.yml".into(),
                state: GitFileState::Modified
            },
            GitFile {
                path: "users/get.yml".into(),
                state: GitFileState::Conflicted
            },
        ]
    );
    assert!(read(&b_dir, "users/get.yml").contains("<<<<<<<"));
    assert_eq!(
        read(&b_dir, "ada.yml"),
        "ada\n",
        "the clean part of the merge is applied"
    );

    let conflict = b.conflict("users/get.yml").unwrap();
    assert_eq!(conflict.base.as_deref(), Some("line 1\nline 2\nline 3\n"));
    assert_eq!(
        conflict.ours.as_deref(),
        Some("line 1\nline 2 by Bob\nline 3\n")
    );
    assert_eq!(
        conflict.theirs.as_deref(),
        Some("line 1\nline 2 by Ada\nline 3\n")
    );
    assert!(b.conflict("ada.yml").is_err());

    // Nothing else while the merge is open.
    assert!(git_err(b.checkout("main").unwrap_err()).contains("merge"));
    assert!(git_err(b.pull(&NoCredentials, None).unwrap_err()).contains("merge"));
    assert!(git_err(b.push(&NoCredentials).unwrap_err()).contains("merge"));
    assert!(git_err(b.commit("too early", &[], None).unwrap_err()).contains("conflicts"));

    b.resolve("users/get.yml", GitResolution::Theirs).unwrap();
    assert_eq!(
        read(&b_dir, "users/get.yml"),
        "line 1\nline 2 by Ada\nline 3\n"
    );
    let status = b.status().unwrap();
    assert!(status.merging);
    assert!(
        status
            .files
            .iter()
            .all(|f| f.state != GitFileState::Conflicted),
        "{:?}",
        status.files
    );

    let merge = b.commit("Merge Ada's work", &[], None).unwrap();
    let status = b.status().unwrap();
    assert!(!status.merging);
    assert!(status.files.is_empty(), "{:?}", status.files);
    assert_eq!(b.log(10).unwrap()[0].oid, merge.oid);
    b.push(&NoCredentials).unwrap();
    assert_eq!(
        a.pull(&NoCredentials, None).unwrap(),
        GitPullResult::FastForward { commits: 2 }
    );
    assert_eq!(
        read(&a_dir, "users/get.yml"),
        "line 1\nline 2 by Ada\nline 3\n"
    );
}

#[test]
fn resolve_with_ours_and_commit_a_conflicted_file_directly() {
    let (remote, _work, a_dir) = remote_with_main("ours");
    let parent = tmp("ours-b");
    let b_dir = clone_as(remote.path(), "Bob", &parent);
    let (a, b) = (reopen(&a_dir), reopen(&b_dir));
    write(&a_dir, "users/get.yml", "Ada\n");
    write(&a_dir, "opencollection.yml", "name: Ada\n");
    a.commit(
        "Ada",
        &paths(&["users/get.yml", "opencollection.yml"]),
        None,
    )
    .unwrap();
    a.push(&NoCredentials).unwrap();
    write(&b_dir, "users/get.yml", "Bob\n");
    write(&b_dir, "opencollection.yml", "name: Bob\n");
    b.commit(
        "Bob",
        &paths(&["users/get.yml", "opencollection.yml"]),
        None,
    )
    .unwrap();
    let GitPullResult::Conflicts { paths: conflicted } = b.pull(&NoCredentials, None).unwrap()
    else {
        panic!("expected conflicts");
    };
    assert_eq!(conflicted, paths(&["opencollection.yml", "users/get.yml"]));
    b.resolve("users/get.yml", GitResolution::Ours).unwrap();
    assert_eq!(read(&b_dir, "users/get.yml"), "Bob\n");
    // Editing a conflicted file by hand and committing it counts as resolving it.
    write(&b_dir, "opencollection.yml", "name: Both\n");
    b.commit("merge", &paths(&["opencollection.yml"]), None)
        .unwrap();
    assert!(!b.status().unwrap().merging);
    b.push(&NoCredentials).unwrap();
}

#[test]
fn checkout_of_a_remote_branch_creates_a_tracking_branch() {
    let (remote, _work, a_dir) = remote_with_main("track");
    let a = reopen(&a_dir);
    a.create_branch("feature", true).unwrap();
    let a = reopen(&a_dir);
    write(&a_dir, "feature.yml", "feature\n");
    a.commit("feature", &paths(&["feature.yml"]), None).unwrap();
    a.push(&NoCredentials).unwrap();

    let parent = tmp("track-b");
    let b_dir = clone_as(remote.path(), "Bob", &parent);
    let b = reopen(&b_dir);
    assert!(!b_dir.join("feature.yml").exists());
    b.checkout("origin/feature").unwrap();
    let b = reopen(&b_dir);
    let status = b.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("feature"));
    assert_eq!(status.upstream.as_deref(), Some("origin/feature"));
    assert_eq!(read(&b_dir, "feature.yml"), "feature\n");
    b.checkout("main").unwrap();
    assert!(!b_dir.join("feature.yml").exists());
}

#[test]
fn credentials_live_in_the_secret_store_under_their_own_collection() {
    let store = MemorySecrets::default();
    assert!(!rt_git::credentials::has(&store, "github.com").unwrap());
    rt_git::credentials::set(&store, "github.com", "ada", "token").unwrap();
    assert!(rt_git::credentials::has(&store, "github.com").unwrap());
    let creds = rt_git::credentials::get(&store, "github.com")
        .unwrap()
        .unwrap();
    assert_eq!(
        (creds.username.as_str(), creds.password.as_str()),
        ("ada", "token")
    );
    assert!(
        !format!("{creds:?}").contains("token"),
        "passwords never print"
    );
    let keys: Vec<_> = store.0.lock().unwrap().keys().cloned().collect();
    assert!(
        keys.iter()
            .all(|k| k.collection == "git:credentials" && k.environment == "github.com")
    );
    rt_git::credentials::delete(&store, "github.com").unwrap();
    assert!(store.0.lock().unwrap().is_empty());
    assert!(rt_git::credentials::set(&store, "evil/host", "a", "b").is_err());
}
