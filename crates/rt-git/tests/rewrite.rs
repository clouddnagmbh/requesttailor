// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand G (acceptance item 7): a plaintext token in three commits on two
//! branches; the history scan finds it once, with every commit and branch; the rewrite (after
//! the user confirmed, in the app) replaces it by `{{githubToken}}` in every commit, keeps
//! backups of the old tips, leaves trees without the value as they were, and a new scan finds
//! nothing. Everything runs in throwaway repositories in the temp folder.
//!
//! `apps/web/src/git/rewrite.test.ts` builds the same repository with isomorphic-git and
//! expects the same rewritten trees ([`TREES`]).

mod common;

use rt_git::RepoExt as _;
use std::cell::RefCell;

use common::*;
use rt_app::rescue::{BACKUP_PREFIX, RescuePlan};

/// Assembled at run time, so secret scanners of the repository stay quiet. The web test
/// builds the same string.
fn token() -> String {
    ["ghp_", "R3scueT0ken", "AbCdEfGhIjKlMnOpQrStUv", "0123"].concat()
}

/// Root trees of `main` and `feature` after the rewrite (the web test expects the same).
const TREES: [&str; 2] = [
    "df5c807f6280c3bf3af57dece1f26fce124c4201",
    "102752dec589d21c9d0a916b79576b0924536fc0",
];

fn request(url: &str, token: &str) -> String {
    format!(
        "info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: {url}\n  headers:\n    - name: Authorization\n      value: Bearer {token}\n"
    )
}

const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: Rescue\n";

struct Fixture {
    _tmp: Tmp,
    dir: std::path::PathBuf,
    c1: String,
    c2: String,
    c3: String,
}

/// c1 (main): collection, request with the token, notes. c2 (main): notes changed.
/// c3 (feature, from c2): the request's URL changed. `HEAD` is `main` (c2), pushed to a
/// bare remote; tag `v1` on c1.
fn fixture(name: &str) -> Fixture {
    let t = tmp(name);
    let dir = t.path().join("work");
    let remote = t.path().join("remote.git");
    std::fs::create_dir_all(&dir).unwrap();
    bare(&remote);
    // The token is committed on purpose; the app's own commit would refuse it (seam N2).
    let repo = new_repo(&dir, "Ada").with_scanner(None);
    let token = token();
    write(&dir, "opencollection.yml", COLLECTION);
    write(
        &dir,
        "users/get-user.yml",
        &request("https://api.example.com/users/1", &token),
    );
    write(&dir, "docs/notes.yml", "info:\n  name: Notes\n");
    let c1 = repo
        .commit(
            "Add the user request",
            &paths(&["opencollection.yml", "users/get-user.yml", "docs/notes.yml"]),
            None,
        )
        .unwrap();
    write(&dir, "docs/notes.yml", "info:\n  name: Notes v2\n");
    let c2 = repo
        .commit("Notes", &paths(&["docs/notes.yml"]), None)
        .unwrap();
    repo.create_branch("feature", true).unwrap();
    write(
        &dir,
        "users/get-user.yml",
        &request("https://api.example.com/users/2", &token),
    );
    let c3 = repo
        .commit("Second user", &paths(&["users/get-user.yml"]), None)
        .unwrap();
    repo.checkout("main").unwrap();
    add_remote(&dir, &rt_git::local_url(&remote));
    reopen(&dir).push(&rt_git::NoCredentials).unwrap();
    let g = gix::open_opts(&dir, gix::open::Options::isolated()).unwrap();
    g.reference(
        "refs/tags/v1",
        gix::ObjectId::from_hex(c1.oid.as_bytes()).unwrap(),
        gix::refs::transaction::PreviousValue::MustNotExist,
        "tag",
    )
    .unwrap();
    Fixture {
        _tmp: t,
        dir,
        c1: c1.oid,
        c2: c2.oid,
        c3: c3.oid,
    }
}

fn tree_of(dir: &std::path::Path, commit: &str) -> String {
    let g = gix::open_opts(dir, gix::open::Options::isolated()).unwrap();
    let c = g
        .find_commit(gix::ObjectId::from_hex(commit.as_bytes()).unwrap())
        .unwrap();
    c.tree_id().unwrap().to_string()
}

fn subtree(dir: &std::path::Path, commit: &str, path: &str) -> String {
    let g = gix::open_opts(dir, gix::open::Options::isolated()).unwrap();
    let c = g
        .find_commit(gix::ObjectId::from_hex(commit.as_bytes()).unwrap())
        .unwrap();
    let tree = c.tree().unwrap();
    tree.lookup_entry_by_path(path)
        .unwrap()
        .unwrap()
        .object_id()
        .to_string()
}

fn backups(dir: &std::path::Path) -> Vec<String> {
    let g = gix::open_opts(dir, gix::open::Options::isolated()).unwrap();
    let refs = g.references().unwrap();
    let mut out: Vec<String> = refs
        .prefixed(BACKUP_PREFIX.trim_end_matches('/'))
        .unwrap()
        .map(|r| r.unwrap().name().as_bstr().to_string())
        .collect();
    out.sort();
    out
}

#[test]
fn a_secret_is_rescued_from_the_history() {
    let f = fixture("rescue");
    let token = token();
    let repo = reopen(&f.dir);

    // The scan: one finding, first in c1, in all three commits, on both branches.
    let mut progress = Vec::new();
    let scan = repo.scan_history(&mut |p| progress.push(p)).unwrap();
    assert_eq!(scan.commits, 3);
    assert_eq!(scan.findings.len(), 1, "{scan:#?}");
    let finding = &scan.findings[0];
    assert_eq!(finding.id, format!("{}:users/get-user.yml:9", f.c1));
    assert_eq!(finding.first.oid, f.c1);
    assert_eq!(
        finding.commits,
        vec![f.c1.clone(), f.c2.clone(), f.c3.clone()]
    );
    assert_eq!(
        finding.branches,
        vec!["feature".to_owned(), "main".to_owned()]
    );
    assert!(finding.in_head);
    assert_eq!(finding.finding.path, "users/get-user.yml");
    assert!(finding.finding.snippet.contains("***"));
    let json = serde_json::to_string(&scan).unwrap();
    assert!(!json.contains(&token), "the value never leaves the shell");
    assert_eq!(progress.last().unwrap().scanned, 3);
    assert_eq!(progress.last().unwrap().total, 3);

    let plan = RescuePlan {
        findings: vec![finding.id.clone()],
        variable: "githubToken".into(),
        environment: "Local".into(),
    };
    let stored = RefCell::new(Vec::<String>::new());

    // Uncommitted changes: refused, nothing stored, no backups.
    write(&f.dir, "users/get-user.yml", "info:\n  name: edited\n");
    let err = repo
        .rewrite_history(&plan, 1_795_000_000, &mut |v| {
            stored.borrow_mut().push(v.to_owned());
            Ok(())
        })
        .unwrap_err();
    assert!(err.to_string().contains("commit or discard"), "{err}");
    assert!(stored.borrow().is_empty());
    assert!(backups(&f.dir).is_empty());
    write(
        &f.dir,
        "users/get-user.yml",
        &request("https://api.example.com/users/1", &token),
    );
    assert!(repo.status().unwrap().files.is_empty());

    let old_main = ref_id(&f.dir, "refs/heads/main").unwrap();
    let old_feature = ref_id(&f.dir, "refs/heads/feature").unwrap();
    let old_docs = subtree(&f.dir, &f.c2, "docs");
    let result = repo
        .rewrite_history(&plan, 1_795_000_000, &mut |v| {
            stored.borrow_mut().push(v.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        *stored.borrow(),
        vec![token.clone()],
        "the value goes to the secret store"
    );
    assert!(!serde_json::to_string(&result).unwrap().contains(&token));
    assert_eq!(result.rewritten, 3);
    assert_eq!(result.backup, format!("{BACKUP_PREFIX}1795000000/"));
    assert_eq!(result.tags, vec!["v1".to_owned()]);
    let names: Vec<(&str, bool, Option<&str>)> = result
        .branches
        .iter()
        .map(|b| (b.name.as_str(), b.pushed, b.remote.as_deref()))
        .collect();
    assert_eq!(
        names,
        vec![("feature", false, None), ("main", true, Some("origin"))]
    );
    let main = &result.branches[1];
    assert_eq!(main.old, old_main);
    assert_eq!(ref_id(&f.dir, "refs/heads/main").unwrap(), main.new);
    assert_eq!(result.branches[0].old, old_feature);

    // Backups on the old tips; the remote and the tag are untouched.
    assert_eq!(
        backups(&f.dir),
        vec![
            format!("{BACKUP_PREFIX}1795000000/feature"),
            format!("{BACKUP_PREFIX}1795000000/main"),
        ]
    );
    assert_eq!(
        ref_id(&f.dir, &format!("{}main", result.backup)).unwrap(),
        old_main
    );
    assert_eq!(
        ref_id(&f.dir, &format!("{}feature", result.backup)).unwrap(),
        old_feature
    );
    assert_eq!(
        ref_id(&f.dir, "refs/remotes/origin/main").unwrap(),
        old_main
    );
    assert_eq!(ref_id(&f.dir, "refs/tags/v1").unwrap(), f.c1);

    // A new scan finds nothing (the backups are not scanned).
    let repo = reopen(&f.dir);
    let again = repo.scan_history(&mut |_| {}).unwrap();
    assert_eq!(again.commits, 3);
    assert!(again.findings.is_empty(), "{again:#?}");

    // Trees without the value are the same objects; metadata stays.
    assert_eq!(subtree(&f.dir, &main.new, "docs"), old_docs);
    assert_eq!(
        subtree(&f.dir, &main.new, "opencollection.yml"),
        subtree(&f.dir, &f.c2, "opencollection.yml")
    );
    let old_log = reopen(&f.dir).commits(None, &old_feature, 10).unwrap();
    let new_log = repo.commits(None, "feature", 10).unwrap();
    let meta = |log: &[rt_app::git::GitCommit]| -> Vec<(String, String, i64)> {
        log.iter()
            .map(|c| (c.summary.clone(), c.author.clone(), c.time))
            .collect()
    };
    assert_eq!(meta(&new_log), meta(&old_log));
    assert!(new_log.iter().zip(&old_log).all(|(n, o)| n.oid != o.oid));

    // The files hold the variable; the work tree follows `HEAD` and is clean.
    let file = read(&f.dir, "users/get-user.yml");
    assert!(file.contains("value: Bearer {{githubToken}}"), "{file}");
    assert!(!file.contains(&token));
    assert!(
        repo.show("feature", "users/get-user.yml")
            .unwrap()
            .unwrap()
            .contains("users/2")
    );
    assert!(repo.status().unwrap().files.is_empty());
    // Git itself agrees that the objects are well-formed (when it is installed).
    if let Ok(out) = std::process::Command::new("git")
        .arg("-C")
        .arg(&f.dir)
        .args(["fsck", "--strict", "--no-dangling"])
        .output()
    {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // The rewritten trees, as the web app's rewrite produces them.
    let trees = [
        tree_of(&f.dir, &main.new),
        tree_of(&f.dir, &result.branches[0].new),
    ];
    assert_eq!(
        trees,
        TREES.map(str::to_owned),
        "update TREES here and in rewrite.test.ts"
    );

    // Nothing left to rescue.
    let err = repo
        .rewrite_history(&plan, 1_795_000_000, &mut |_| Ok(()))
        .unwrap_err();
    assert!(err.to_string().contains("scan the history again"), "{err}");
}

#[test]
fn backups_never_overwrite_an_earlier_rescue() {
    let f = fixture("rescue-twice");
    let token = token();
    let repo = reopen(&f.dir);
    // An earlier rescue in the same second left a backup of `main`.
    let g = gix::open_opts(&f.dir, gix::open::Options::isolated()).unwrap();
    g.reference(
        format!("{BACKUP_PREFIX}1795000000/main"),
        gix::ObjectId::from_hex(f.c1.as_bytes()).unwrap(),
        gix::refs::transaction::PreviousValue::MustNotExist,
        "earlier",
    )
    .unwrap();
    let scan = repo.scan_history(&mut |_| {}).unwrap();
    let plan = RescuePlan {
        findings: vec![scan.findings[0].id.clone()],
        variable: "githubToken".into(),
        environment: "Local".into(),
    };
    let result = repo
        .rewrite_history(&plan, 1_795_000_000, &mut |v| {
            assert_eq!(v, token);
            Ok(())
        })
        .unwrap();
    assert_eq!(result.backup, format!("{BACKUP_PREFIX}1795000001/"));
    assert_eq!(
        ref_id(&f.dir, &format!("{BACKUP_PREFIX}1795000000/main")).unwrap(),
        f.c1
    );

    // A wrong id or variable is refused before anything happens.
    let bad = RescuePlan {
        variable: "not a name".into(),
        ..plan.clone()
    };
    assert!(repo.rewrite_history(&bad, 1, &mut |_| Ok(())).is_err());
    let gone = RescuePlan {
        findings: vec![format!("{}:users/get-user.yml:2", f.c1)],
        ..plan
    };
    assert!(repo.rewrite_history(&gone, 1, &mut |_| Ok(())).is_err());
}
