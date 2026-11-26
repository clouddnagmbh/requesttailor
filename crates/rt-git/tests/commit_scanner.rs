// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Seam N2: both commit forms (`commit` with paths and `commitStaged`) hand the new and
//! changed files to the repository's commit scanner and refuse with its message before any
//! ref moves. RequestTailor's default (`SecretScanner`) refuses credentials; without a
//! scanner nothing is checked; another scanner plugged in decides alone. Fake credentials are
//! assembled at run time (gitleaks stays clean).

mod common;
mod wizard;

use std::sync::{Arc, Mutex};

use common::*;
use rt_app::git::index::GitStagedCommit;
use rt_git::{GitError, Repo};
use tk_git_types::CommitScanner;
use wizard::*;

fn token() -> String {
    ["ghp_", "aBcDeFgHiJkLmNoPqRsTuVwXyZ", "0123456789"].concat()
}

fn staged(message: &str) -> GitStagedCommit {
    GitStagedCommit {
        message: message.into(),
    }
}

fn head(f: &Fixture) -> Option<String> {
    ref_id(f.dir(), "refs/heads/main")
}

/// Refuses files containing `FORBIDDEN` and records what it was shown.
#[derive(Default)]
struct Recording(Mutex<Vec<Vec<(String, String)>>>);

impl CommitScanner for Recording {
    fn refusal(&self, files: &[(&str, &[u8])]) -> Option<String> {
        let seen: Vec<(String, String)> = files
            .iter()
            .map(|(p, b)| ((*p).to_owned(), String::from_utf8_lossy(b).into_owned()))
            .collect();
        let refuse = seen.iter().any(|(_, t)| t.contains("FORBIDDEN"));
        self.0.lock().unwrap().push(seen);
        refuse.then(|| "no FORBIDDEN here".to_owned())
    }
}

fn with(f: &Fixture, scanner: &Arc<Recording>) -> Repo {
    f.repo()
        .with_scanner(Some(scanner.clone() as Arc<dyn CommitScanner>))
}

#[test]
fn the_default_refuses_credentials_and_none_checks_nothing() {
    let f = fixture("scanner-default");
    f.write("keys.txt", &format!("token = {}\n", token()));
    let before = head(&f);

    let err = f
        .repo()
        .commit("keys", &paths(&["keys.txt"]), None)
        .unwrap_err();
    match &err {
        GitError::SecretsFound(message) => {
            assert!(message.contains("keys.txt"), "{message}");
            assert!(!message.contains(&token()), "{message}");
        }
        other => panic!("expected SecretsFound, got {other:?}"),
    }
    assert_eq!(head(&f), before);

    f.repo()
        .with_scanner(None)
        .commit("keys", &paths(&["keys.txt"]), None)
        .unwrap();
    assert_ne!(head(&f), before);
}

#[test]
fn a_plugged_in_scanner_sees_new_and_changed_files_and_decides_both_commit_forms() {
    let f = fixture("scanner-plugged");
    let scanner = Arc::new(Recording::default());
    f.write("added.txt", "FORBIDDEN\n");
    f.write("notes.txt", "changed\n");
    std::fs::remove_file(f.dir().join("users/get.yml")).unwrap();
    let before = head(&f);
    let all = paths(&["added.txt", "notes.txt", "users/get.yml"]);

    // `commit`: the deleted file is not shown, the others as committed.
    let err = with(&f, &scanner).commit("paths", &all, None).unwrap_err();
    assert!(
        matches!(&err, GitError::SecretsFound(m) if m == "no FORBIDDEN here"),
        "{err:?}"
    );
    assert_eq!(head(&f), before);
    assert_eq!(
        scanner.0.lock().unwrap().last().unwrap(),
        &[
            ("added.txt".to_owned(), "FORBIDDEN\n".to_owned()),
            ("notes.txt".to_owned(), "changed\n".to_owned()),
        ]
    );

    // `commitStaged`: the same rule for what the index holds.
    let repo = with(&f, &scanner);
    repo.stage(&paths(&["/added.txt", "/notes.txt", "/users/get.yml"]))
        .unwrap();
    let err = repo.commit_staged(&staged("staged"), None).unwrap_err();
    assert!(matches!(err, GitError::SecretsFound(_)), "{err:?}");
    assert_eq!(head(&f), before);
    let mut shown = scanner.0.lock().unwrap().last().unwrap().clone();
    shown.sort();
    assert_eq!(
        shown,
        [
            ("added.txt".to_owned(), "FORBIDDEN\n".to_owned()),
            ("notes.txt".to_owned(), "changed\n".to_owned()),
        ]
    );

    // The plugged-in scanner decides alone: a credential passes, its own rule does not.
    f.write("added.txt", &format!("token = {}\n", token()));
    with(&f, &scanner).commit("passes", &all, None).unwrap();
    assert_ne!(head(&f), before);
}
