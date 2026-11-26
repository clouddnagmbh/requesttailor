// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Seam N8: a `Repo` opened with another namespace keeps its backups and its action journal
//! there and leaves `refs/requesttailor/…` and `.git/requesttailor/` alone; undo reads its
//! own journal. RequestTailor itself always uses `rt_app::git::REF_NAMESPACE`.

mod common;
mod wizard;

use common::*;
use rt_app::git::REF_NAMESPACE;
use tk_git_types::RefNamespace;
use wizard::*;

const OTHER: RefNamespace = RefNamespace::new("othertool", &["requesttailor"]);

#[test]
fn requesttailor_keeps_its_own_namespace() {
    assert_eq!(REF_NAMESPACE.name(), "requesttailor");
    assert!(REF_NAMESPACE.reads().is_empty());
    assert_eq!(rt_app::rescue::BACKUP_PREFIX, REF_NAMESPACE.backup_prefix());
    let f = fixture("ns-default");
    assert_eq!(f.repo().namespace(), REF_NAMESPACE);
}

#[test]
fn backups_and_the_journal_go_to_the_repos_namespace() {
    let f = fixture("ns-other");
    let repo = f.repo().with_namespace(OTHER);
    apply(
        &repo,
        &act(r#"{"action":"create-branch","name":"other","start":"HEAD"}"#),
    );
    let deleted = apply(
        &repo,
        &act(r#"{"action":"delete-branch","name":"other","force":true}"#),
    );

    let git = f.dir().join(".git");
    assert!(git.join("othertool/actions.jsonl").is_file());
    assert!(!git.join("requesttailor").exists());
    let mut refs = Vec::new();
    for r in gix::open(f.dir())
        .unwrap()
        .references()
        .unwrap()
        .all()
        .unwrap()
    {
        refs.push(r.unwrap().name().as_bstr().to_string());
    }
    assert!(
        refs.iter().any(|r| r.starts_with("refs/othertool/backup/")),
        "{refs:?}"
    );
    assert!(
        !refs.iter().any(|r| r.starts_with("refs/requesttailor/")),
        "{refs:?}"
    );

    // Undo finds the entry in this namespace's journal; RequestTailor's sees none.
    assert!(f.repo().last_action().unwrap().is_none());
    repo.undo(&deleted.undo.unwrap().id, false).unwrap();
    assert!(ref_id(f.dir(), "refs/heads/other").is_some());
}
