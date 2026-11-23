// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 14, strand F: the commit graph of a whole repository (`GitApi.graph`, `refs`,
//! `changes`, `fileDiff`): every branch, remote branch and tag, topological order, stable
//! pages, ahead/behind, and what a commit changed (renames, binary files, any file of the
//! repository, not only collection files).

mod common;

use common::*;
use gix::ObjectId;
use gix::objs::tree::EntryKind;
use gix::refs::transaction::PreviousValue;
use rt_app::git::graph::{GitGraphQuery, GitRefKind};
use rt_app::git::{DiffLineKind, GitFileChange};

/// Writes a commit of exactly `files` (a full snapshot) with `parents`, authored and
/// committed at `time` (Unix seconds) by `who`.
fn snapshot(
    g: &gix::Repository,
    parents: &[ObjectId],
    files: &[(&str, &[u8])],
    message: &str,
    time: i64,
    who: &str,
) -> ObjectId {
    let mut editor = g.edit_tree(ObjectId::empty_tree(g.object_hash())).unwrap();
    for (path, bytes) in files {
        let id = g.write_blob(bytes).unwrap().detach();
        editor.upsert(*path, EntryKind::Blob, id).unwrap();
    }
    let tree = editor.write().unwrap().detach();
    let sig = gix::actor::Signature {
        name: who.into(),
        email: format!("{}@example.com", who.to_lowercase()).into(),
        time: gix::date::Time::new(time, 0),
    };
    let mut buf = gix::date::parse::TimeBuf::default();
    let sig = sig.to_ref(&mut buf);
    g.new_commit_as(sig, sig, message, tree, parents.iter().copied())
        .unwrap()
        .id
}

fn set_ref(g: &gix::Repository, name: &str, id: ObjectId) {
    g.reference(name, id, PreviousValue::Any, "test").unwrap();
}

fn git(dir: &std::path::Path) -> gix::Repository {
    gix::open_opts(dir, gix::open::Options::isolated()).unwrap()
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01";
const COLLECTION: &[u8] = b"opencollection: 1.0.0\ninfo:\n  name: Billing\n";

struct Fixture {
    _t: Tmp,
    _bare: Tmp,
    work: std::path::PathBuf,
    c1: ObjectId,
    c2: ObjectId,
    f1: ObjectId,
    f2: ObjectId,
    merge: ObjectId,
    c4: ObjectId,
    docs: ObjectId,
    remote_only: ObjectId,
    local_only: ObjectId,
}

/// ```text
///  local_only (main, ahead 1)      remote_only (origin/main, behind 1)
///        \                        /
///         c4  (rename api/post.yml → api/create.yml, README edited)
///          |
///        merge  (v1, annotated)
///        /    \
///      c2      f2 (feature, origin/feature)
///       |      |
///       |      f1 (logo.png, api/post.yml)
///        \    /
///         c1  (v0, lightweight)            docs (second root, branch docs)
/// ```
fn fixture() -> Fixture {
    let t = tmp("graph");
    let bare_dir = tmp("graph-remote");
    bare(bare_dir.path());
    let work = t.path().to_path_buf();
    rt_git::init(&work, &opts()).unwrap();
    set_user(&work, "Ada");
    add_remote(&work, &rt_git::local_url(bare_dir.path()));
    let g = git(&work);

    let readme1: &[u8] = b"# Billing\n";
    let c1 = snapshot(
        &g,
        &[],
        &[
            ("README.md", readme1),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v1\n"),
            ("api/post.yml", b"method: POST\nurl: /v1/pay\nbody: x\n"),
        ],
        "Start the billing API",
        1_000,
        "Ada",
    );
    let c2 = snapshot(
        &g,
        &[c1],
        &[
            ("README.md", readme1),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v2\n"),
            ("api/post.yml", b"method: POST\nurl: /v1/pay\nbody: x\n"),
        ],
        "Version 2 of get",
        2_000,
        "Ada",
    );
    let f1 = snapshot(
        &g,
        &[c1],
        &[
            ("README.md", readme1),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v1\n"),
            ("api/post.yml", b"method: POST\nurl: /v1/pay\nbody: x\n"),
            ("logo.png", PNG),
        ],
        "Add a logo",
        1_500,
        "Grace",
    );
    let f2 = snapshot(
        &g,
        &[f1],
        &[
            ("README.md", readme1),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v1\n"),
            ("api/post.yml", b"method: POST\nurl: /v1/pay\nbody: y\n"),
            ("logo.png", PNG),
        ],
        "Body y",
        2_500,
        "Grace",
    );
    let merged: Vec<(&str, &[u8])> = vec![
        ("README.md", readme1),
        ("api/opencollection.yml", COLLECTION),
        ("api/get.yml", b"url: /v2\n"),
        ("api/post.yml", b"method: POST\nurl: /v1/pay\nbody: y\n"),
        ("logo.png", PNG),
    ];
    let merge = snapshot(&g, &[c2, f2], &merged, "Merge feature", 3_000, "Ada");
    let c4 = snapshot(
        &g,
        &[merge],
        &[
            ("README.md", b"# Billing\n\nPayments.\n"),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v2\n"),
            ("api/create.yml", b"method: POST\nurl: /v1/pay\nbody: y\n"),
            ("logo.png", PNG),
        ],
        "Rename post to create",
        4_000,
        "Ada",
    );
    let docs = snapshot(&g, &[], &[("guide.md", b"# Guide\n")], "Docs", 500, "Linus");
    set_ref(&g, "refs/heads/main", c4);
    set_ref(&g, "refs/heads/feature", f2);
    set_ref(&g, "refs/heads/docs", docs);
    set_ref(&g, "refs/tags/v0", c1);
    g.tag(
        "v1",
        merge,
        gix::objs::Kind::Commit,
        None,
        "First release\n\nWith notes.\n",
        PreviousValue::MustNotExist,
    )
    .unwrap();

    // The remote: main and feature pushed to a bare repository, then fetched.
    rt_git::local::push(&g, bare_dir.path(), &opts(), c4, "refs/heads/main").unwrap();
    rt_git::local::push(&g, bare_dir.path(), &opts(), f2, "refs/heads/feature").unwrap();
    append_config(
        &work,
        "[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
    );
    // Someone else pushes to main; we commit locally: main is ahead 1 and behind 1.
    let b = git(bare_dir.path());
    let remote_only = snapshot(
        &b,
        &[c4],
        &[("README.md", b"# Billing (remote)\n")],
        "Remote edit",
        5_000,
        "Linus",
    );
    set_ref(&b, "refs/heads/main", remote_only);
    rt_git::local::fetch(&g, "origin", bare_dir.path(), &opts()).unwrap();
    let local_only = snapshot(
        &g,
        &[c4],
        &[
            ("README.md", b"# Billing\n\nPayments.\n"),
            ("api/opencollection.yml", COLLECTION),
            ("api/get.yml", b"url: /v2\n"),
            ("api/create.yml", b"method: POST\nurl: /v1/pay\nbody: y\n"),
            ("logo.png", PNG),
        ],
        "Local edit",
        6_000,
        "Ada",
    );
    set_ref(&g, "refs/heads/main", local_only);
    // The work tree: README edited, not committed.
    write(&work, "README.md", "# Billing\n\nPayments in EUR.\n");
    Fixture {
        _t: t,
        _bare: bare_dir,
        work,
        c1,
        c2,
        f1,
        f2,
        merge,
        c4,
        docs,
        remote_only,
        local_only,
    }
}

fn ids(page: &rt_app::git::graph::GitGraphPage) -> Vec<String> {
    page.commits.iter().map(|c| c.commit.oid.clone()).collect()
}

#[test]
fn all_refs_with_kinds_upstream_ahead_behind_and_tag_messages() {
    let f = fixture();
    let repo = reopen(&f.work);
    let refs = repo.refs().unwrap();
    let names: Vec<(&str, GitRefKind)> = refs.iter().map(|r| (r.full.as_str(), r.kind)).collect();
    assert_eq!(
        names,
        vec![
            ("refs/heads/docs", GitRefKind::Local),
            ("refs/heads/feature", GitRefKind::Local),
            ("refs/heads/main", GitRefKind::Local),
            ("refs/remotes/origin/feature", GitRefKind::Remote),
            ("refs/remotes/origin/main", GitRefKind::Remote),
            ("refs/tags/v0", GitRefKind::Tag),
            ("refs/tags/v1", GitRefKind::Tag),
        ]
    );
    let main = refs.iter().find(|r| r.name == "main").unwrap();
    assert!(main.current);
    assert_eq!(main.target, f.local_only.to_string());
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!((main.ahead, main.behind), (1, 1));
    let feature = refs.iter().find(|r| r.name == "feature").unwrap();
    assert!(!feature.current);
    assert_eq!(feature.upstream, None, "no upstream configured");
    assert_eq!((feature.ahead, feature.behind), (0, 0));
    let origin = refs.iter().find(|r| r.name == "origin/main").unwrap();
    assert_eq!(origin.target, f.remote_only.to_string());
    // Annotated tags are peeled to their commit and carry the first line of the message.
    let v1 = refs.iter().find(|r| r.name == "v1").unwrap();
    assert_eq!(v1.target, f.merge.to_string());
    assert_eq!(v1.message.as_deref(), Some("First release"));
    let v0 = refs.iter().find(|r| r.name == "v0").unwrap();
    assert_eq!(v0.target, f.c1.to_string());
    assert_eq!(v0.message, None, "lightweight");
}

#[test]
fn the_graph_is_topological_newest_first_over_all_refs() {
    let f = fixture();
    let repo = reopen(&f.work);
    let page = repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 100,
            refs: None,
        })
        .unwrap();
    // Children before parents, ties by commit time; both roots and the remote-only commit.
    let expected: Vec<String> = [
        f.local_only,
        f.remote_only,
        f.c4,
        f.merge,
        f.f2,
        f.c2,
        f.f1,
        f.c1,
        f.docs,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(ids(&page), expected);
    assert!(!page.more);
    assert_eq!(page.head.as_deref(), Some(f.local_only.to_string().as_str()));
    assert!(!page.detached);
    assert_eq!(page.refs.len(), 7);

    let merge = &page.commits[3];
    assert_eq!(merge.commit.summary, "Merge feature");
    assert_eq!(merge.parents, vec![f.c2.to_string(), f.f2.to_string()]);
    assert_eq!(merge.refs, vec!["refs/tags/v1".to_owned()]);
    assert_eq!(merge.commit.author, "Ada");
    assert_eq!(merge.email, "ada@example.com");
    assert_eq!(merge.commit.time, 3_000);
    let f2 = &page.commits[4];
    assert_eq!(
        f2.refs,
        vec![
            "refs/heads/feature".to_owned(),
            "refs/remotes/origin/feature".to_owned()
        ]
    );
    assert!(page.commits[8].parents.is_empty(), "a second root");
}

#[test]
fn pages_are_stable_and_say_when_more_follow() {
    let f = fixture();
    let repo = reopen(&f.work);
    let all = ids(&repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 100,
            refs: None,
        })
        .unwrap());
    let mut paged = Vec::new();
    let mut skip = 0;
    loop {
        let page = repo
            .commit_graph(&GitGraphQuery {
                skip,
                limit: 4,
                refs: None,
            })
            .unwrap();
        skip += page.commits.len() as u32;
        paged.extend(ids(&page));
        assert_eq!(page.refs.len(), 7, "refs on every page");
        if !page.more {
            break;
        }
    }
    assert_eq!(paged, all);
    let past = repo
        .commit_graph(&GitGraphQuery {
            skip: 50,
            limit: 4,
            refs: None,
        })
        .unwrap();
    assert!(past.commits.is_empty() && !past.more);
}

#[test]
fn the_graph_can_start_from_chosen_refs() {
    let f = fixture();
    let repo = reopen(&f.work);
    let page = repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 100,
            refs: Some(vec!["refs/heads/feature".into()]),
        })
        .unwrap();
    assert_eq!(
        ids(&page),
        vec![f.f2.to_string(), f.f1.to_string(), f.c1.to_string()]
    );
    assert!(
        repo.commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 1,
            refs: Some(vec!["refs/heads/nope".into()]),
        })
        .is_err()
    );
}

#[test]
fn a_detached_head_is_a_ref_of_its_own() {
    let f = fixture();
    std::fs::write(f.work.join(".git/HEAD"), format!("{}\n", f.c2)).unwrap();
    let repo = reopen(&f.work);
    let refs = repo.refs().unwrap();
    let head = refs.iter().find(|r| r.kind == GitRefKind::Head).unwrap();
    assert_eq!(
        (head.full.as_str(), head.name.as_str(), head.current),
        ("HEAD", "HEAD", true)
    );
    assert_eq!(head.target, f.c2.to_string());
    assert!(refs.iter().filter(|r| r.current).count() == 1, "no branch is current");
    let page = repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 100,
            refs: None,
        })
        .unwrap();
    assert!(page.detached);
    assert_eq!(page.head.as_deref(), Some(f.c2.to_string().as_str()));
    let c2 = page
        .commits
        .iter()
        .find(|c| c.commit.oid == f.c2.to_string())
        .unwrap();
    assert_eq!(c2.refs, vec!["HEAD".to_owned()]);
}

#[test]
fn an_empty_repository_has_an_empty_graph() {
    let t = tmp("graph-empty");
    let repo = new_repo(t.path(), "Ada");
    let page = repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 10,
            refs: None,
        })
        .unwrap();
    assert!(page.commits.is_empty() && page.refs.is_empty());
    assert_eq!(page.head, None);
    assert!(!page.detached && !page.more);
}

#[test]
fn changes_of_a_commit_with_renames_binary_files_and_any_path() {
    let f = fixture();
    let repo = reopen(&f.work);
    // Against the first parent by default; paths are repository-relative with a leading `/`.
    let c4 = repo.changes(None, &f.c4.to_string()).unwrap();
    let got: Vec<(&str, GitFileChange, Option<&str>)> = c4
        .iter()
        .map(|c| (c.path.as_str(), c.change, c.from.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("/README.md", GitFileChange::Modified, None),
            (
                "/api/create.yml",
                GitFileChange::Renamed,
                Some("/api/post.yml")
            ),
        ]
    );
    assert!(c4.iter().all(|c| !c.binary && c.collection.is_none()));

    let f1 = repo.changes(None, &f.f1.to_string()).unwrap();
    assert_eq!(f1.len(), 1);
    assert_eq!(
        (f1[0].path.as_str(), f1[0].change, f1[0].binary),
        ("/logo.png", GitFileChange::Added, true)
    );

    // A root commit: everything added.
    let root = repo.changes(None, &f.docs.to_string()).unwrap();
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].change, GitFileChange::Added);

    // A range, and a merge against its first parent (what the feature brought in).
    let range = repo
        .changes(Some(&f.c1.to_string()), &f.c4.to_string())
        .unwrap();
    let paths: Vec<&str> = range.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/README.md", "/api/create.yml", "/api/get.yml", "/logo.png"]
    );
    let merge = repo.changes(None, &f.merge.to_string()).unwrap();
    let paths: Vec<&str> = merge.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, vec!["/api/post.yml", "/logo.png"]);
    // A rename with edits is found by similarity.
    let edited = repo
        .changes(Some(&f.c2.to_string()), &f.c4.to_string())
        .unwrap();
    let create = edited.iter().find(|c| c.path == "/api/create.yml").unwrap();
    assert_eq!(create.change, GitFileChange::Renamed);
    assert_eq!(create.from.as_deref(), Some("/api/post.yml"));
    assert!(edited.iter().all(|c| c.path != "/api/post.yml"));
}

#[test]
fn collection_files_are_named_by_the_shell() {
    let f = fixture();
    let repo = reopen(&f.work);
    let mut files = repo.changes(Some(&f.c1.to_string()), &f.c4.to_string()).unwrap();
    rt_app::git::graph::assign_collections(
        &mut files,
        &[rt_app::git::graph::CollectionAt {
            id: "billing".into(),
            dir: "api".into(),
        }],
    );
    let named: Vec<(&str, Option<&str>)> = files
        .iter()
        .map(|c| (c.path.as_str(), c.collection.as_deref()))
        .collect();
    assert_eq!(
        named,
        vec![
            ("/README.md", None),
            ("/api/create.yml", Some("billing")),
            ("/api/get.yml", Some("billing")),
            ("/logo.png", None),
        ]
    );
}

#[test]
fn file_diffs_between_revisions_and_against_the_work_tree() {
    let f = fixture();
    let repo = reopen(&f.work);
    let lines = |d: &rt_app::git::GitDiff| -> Vec<(DiffLineKind, String)> {
        d.hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| (l.kind, l.text.clone())))
            .collect()
    };
    let get = repo
        .file_diff(Some(&f.c1.to_string()), Some(&f.c2.to_string()), "/api/get.yml")
        .unwrap();
    assert_eq!(get.path, "/api/get.yml");
    assert_eq!(
        lines(&get),
        vec![
            (DiffLineKind::Removed, "url: /v1".into()),
            (DiffLineKind::Added, "url: /v2".into())
        ]
    );
    // `from` omitted: the first parent of `to`.
    let same = repo.file_diff(None, Some(&f.c2.to_string()), "/api/get.yml").unwrap();
    assert_eq!(same, get);
    // A renamed file diffs against its old path.
    let renamed = repo
        .file_diff(None, Some(&f.c4.to_string()), "/api/create.yml")
        .unwrap();
    assert!(renamed.hunks.is_empty(), "renamed without edits");
    let renamed = repo
        .file_diff(Some(&f.c2.to_string()), Some(&f.c4.to_string()), "/api/create.yml")
        .unwrap();
    assert_eq!(
        lines(&renamed)
            .into_iter()
            .filter(|(k, _)| *k != DiffLineKind::Context)
            .collect::<Vec<_>>(),
        vec![
            (DiffLineKind::Removed, "body: x".into()),
            (DiffLineKind::Added, "body: y".into())
        ]
    );
    // Binary files have no hunks.
    let logo = repo
        .file_diff(None, Some(&f.f1.to_string()), "/logo.png")
        .unwrap();
    assert!(logo.binary && logo.hunks.is_empty());
    // `to` omitted: the work tree, against `from` (or `HEAD`).
    let work = repo.file_diff(None, None, "/README.md").unwrap();
    assert_eq!(
        lines(&work)
            .into_iter()
            .filter(|(k, _)| *k != DiffLineKind::Context)
            .collect::<Vec<_>>(),
        vec![
            (DiffLineKind::Removed, "Payments.".into()),
            (DiffLineKind::Added, "Payments in EUR.".into())
        ]
    );
    let work = repo
        .file_diff(Some(&f.c1.to_string()), None, "/README.md")
        .unwrap();
    assert_eq!(work.hunks.len(), 1);
    // Paths may not leave the repository.
    assert!(repo.file_diff(None, None, "/../x").is_err());
    assert!(repo.file_diff(None, None, "/.git/config").is_err());
}

#[test]
fn ten_thousand_commits_page_quickly() {
    let t = tmp("graph-big");
    let work = t.path().to_path_buf();
    rt_git::init(&work, &opts()).unwrap();
    set_user(&work, "Ada");
    let g = git(&work);
    let mut tip = snapshot(&g, &[], &[("a.txt", b"0")], "c0", 1, "Ada");
    let mut side = tip;
    for i in 1..10_000i64 {
        // Every 50th commit starts a side branch that merges back 10 commits later.
        if i % 50 == 0 {
            side = tip;
        }
        if i % 50 > 0 && i % 50 < 10 {
            side = snapshot(&g, &[side], &[("a.txt", b"s")], "side", i, "Grace");
            continue;
        }
        let parents: Vec<ObjectId> = if i % 50 == 10 { vec![tip, side] } else { vec![tip] };
        tip = snapshot(&g, &parents, &[("a.txt", b"m")], "main", i, "Ada");
    }
    set_ref(&g, "refs/heads/main", tip);
    let repo = reopen(&work);
    let started = std::time::Instant::now();
    let page = repo
        .commit_graph(&GitGraphQuery {
            skip: 0,
            limit: 2000,
            refs: None,
        })
        .unwrap();
    let first = started.elapsed();
    assert_eq!(page.commits.len(), 2000);
    assert!(page.more);
    let last = repo
        .commit_graph(&GitGraphQuery {
            skip: 8000,
            limit: 2000,
            refs: None,
        })
        .unwrap();
    assert_eq!(last.commits.len(), 2000);
    assert!(!last.more);
    // Generous for debug builds on CI; the release budget is checked by the perf run.
    assert!(
        first.as_millis() < 5_000,
        "first page of 10 000 commits took {first:?}"
    );
}
