// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 15, strand F: a distribution's `rt_git::sign::Signing` (managed on the builder, given
//! to `Core::with_signing`) signs every commit the shell writes (commit, commit-staged) and
//! `verify` asks its verifier; a core without it commits unsigned and answers `unverified` /
//! `unsigned`. The index commands and `git config` go through the same functions the Tauri
//! commands call (`rt_desktop::git_index`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use rt_app::git::index::{GitPatchSide, GitStagedCommit};
use rt_app::git::signing::{GitSignatureCheck, GitSignatureKind, GitSignatureState};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_desktop::{Core, git, git_index};
use rt_git::GitError;
use rt_git::sign::{CommitSigner, SignatureVerifier, Signing};

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("rt-desktop-sign-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(rt_git::canonical(&dir).unwrap())
}

fn armor(payload: &[u8]) -> String {
    let sum: u64 = payload.iter().map(|b| u64::from(*b)).sum();
    format!("-----BEGIN SSH SIGNATURE-----\n{sum:x}\n-----END SSH SIGNATURE-----\n")
}

struct Signer;

impl CommitSigner for Signer {
    fn sign(&self, _workdir: &Path, payload: &[u8]) -> Result<Option<String>, GitError> {
        Ok(Some(armor(payload)))
    }
}

struct Verifier;

impl SignatureVerifier for Verifier {
    fn verify(
        &self,
        _workdir: &Path,
        oid: &str,
        payload: &[u8],
        signature: &str,
    ) -> Result<GitSignatureCheck, GitError> {
        Ok(GitSignatureCheck {
            oid: oid.to_owned(),
            state: if signature == armor(payload) {
                GitSignatureState::Good
            } else {
                GitSignatureState::Bad
            },
            kind: Some(GitSignatureKind::Ssh),
            signer: Some("ada".into()),
            key: None,
        })
    }
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn collection(name: &str, signing: Option<Signing>) -> (TempDir, Core<MemorySecrets>) {
    let work = temp(name);
    std::fs::write(
        work.0.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Signed\n",
    )
    .unwrap();
    let mut core = Core::new(MemorySecrets::default()).unwrap();
    if let Some(s) = signing {
        core = core.with_signing(s);
    }
    core.open(&work.0).unwrap();
    git::init(&core).unwrap();
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(work.0.join(".git/config"))
        .unwrap();
    std::io::Write::write_all(
        &mut config,
        b"[user]\n\tname = Ada\n\temail = ada@example.com\n",
    )
    .unwrap();
    (work, core)
}

#[test]
fn managed_signing_signs_the_shells_commits_and_verify_asks_the_verifier() {
    let signing = Signing {
        signer: Some(Arc::new(Signer)),
        verifier: Some(Arc::new(Verifier)),
    };
    let (work, core) = collection("signed", Some(signing));
    let all = vec!["opencollection.yml".to_owned()];
    let c = git::commit(&core, "First", &all).unwrap();
    let raw = git_out(&work.0, &["cat-file", "commit", &c.oid]);
    assert!(
        raw.contains("\ngpgsig -----BEGIN SSH SIGNATURE-----\n"),
        "{raw}"
    );

    // The index route signs too.
    std::fs::write(work.0.join("notes.txt"), "a\n").unwrap();
    git_index::stage(&core, &["/notes.txt".to_owned()]).unwrap();
    let staged = git_index::commit_staged(
        &core,
        &GitStagedCommit {
            message: "Second".into(),
        },
    )
    .unwrap();
    assert!(git_out(&work.0, &["cat-file", "commit", &staged.oid]).contains("\ngpgsig "));
    assert_eq!(git_out(&work.0, &["status", "--porcelain"]), "");

    let checks = git_index::verify(&core, &[c.oid.clone(), staged.oid.clone()]).unwrap();
    assert!(checks.iter().all(|c| c.state == GitSignatureState::Good));
}

#[test]
fn without_signing_commits_are_unsigned_and_verify_says_so() {
    let (work, core) = collection("plain", None);
    let c = git::commit(&core, "First", &["opencollection.yml".to_owned()]).unwrap();
    assert!(!git_out(&work.0, &["cat-file", "commit", &c.oid]).contains("gpgsig"));
    let checks = git_index::verify(&core, &[c.oid]).unwrap();
    assert_eq!(checks[0].state, GitSignatureState::Unsigned);
}

#[test]
fn the_index_commands_and_git_config_work_on_the_active_repository() {
    let (work, core) = collection("index", None);
    git::commit(&core, "First", &["opencollection.yml".to_owned()]).unwrap();
    std::fs::write(work.0.join("notes.txt"), "one\ntwo\n").unwrap();
    let d = git_index::index_diff(&core, "/notes.txt", GitPatchSide::Unstaged).unwrap();
    assert_eq!(d.old, "");
    assert_eq!(d.diff.hunks.len(), 1);
    git_index::stage(&core, &["/notes.txt".to_owned()]).unwrap();
    assert_eq!(
        git_out(&work.0, &["diff", "--cached", "--name-only"]),
        "notes.txt\n"
    );
    git_index::unstage(&core, &["/notes.txt".to_owned()]).unwrap();
    assert_eq!(git_out(&work.0, &["diff", "--cached", "--name-only"]), "");
    // `git config`: the repository's own configuration.
    assert_eq!(
        git_index::config(&core, "user.name").unwrap().as_deref(),
        Some("Ada")
    );
    assert_eq!(git_index::config(&core, "commit.template").unwrap(), None);
}
