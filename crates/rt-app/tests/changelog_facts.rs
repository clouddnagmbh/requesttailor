// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 16 interface: the facts a changelog writer builds on are the deterministic changelog
//! and its parts, nothing else.

use rt_app::AppError;
use rt_app::contract::{
    ChangelogCommit, ChangelogSection, Side, changelog_facts, changelog_with, pull_number,
};
use rt_format::FileSet;

fn files(requests: &[(&str, &str)]) -> FileSet {
    let mut files = FileSet::default();
    files.files.insert(
        "opencollection.yml".into(),
        b"opencollection: 1.0.0\ninfo:\n  name: shop\n".to_vec(),
    );
    for (p, t) in requests {
        files.files.insert((*p).into(), t.as_bytes().to_vec());
    }
    files
}

struct Notes;

impl ChangelogSection for Notes {
    fn section(
        &self,
        _: &Side<'_>,
        _: &Side<'_>,
        _: &str,
        _: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(Some("## Service metadata\n\n- one change\n".into()))
    }
}

#[test]
fn facts_carry_the_changelog_its_parts_sections_and_commits() {
    let old = files(&[]);
    let new = files(&[(
        "users.yml",
        "info:\n  name: Users\n  type: http\nhttp:\n  method: GET\n  url: https://api.example.com/users\n",
    )]);
    let none = |_: &str| -> Result<Option<Vec<u8>>, AppError> { Ok(None) };
    let (a, b) = (
        Side {
            files: &old,
            read: &none,
        },
        Side {
            files: &new,
            read: &none,
        },
    );
    let commit = ChangelogCommit {
        oid: "a".repeat(40),
        summary: "Add users (#3)".into(),
        body: String::new(),
        author: "Ada".into(),
        pull: Some(3),
    };
    let facts = changelog_facts(&a, &b, "v1", "v2", &[&Notes], vec![commit.clone()]).unwrap();
    assert_eq!(
        facts.markdown,
        changelog_with(&a, &b, "v1", "v2", &[&Notes]).unwrap()
    );
    assert_eq!(facts.sections, ["## Service metadata\n\n- one change"]);
    assert_eq!(facts.commits, [commit]);
    assert!(!facts.collection.changes.is_empty(), "{facts:?}");
    assert_eq!((facts.from.as_str(), facts.to.as_str()), ("v1", "v2"));
}

#[test]
fn pull_request_numbers_are_read_from_github_and_gitlab_messages() {
    for (message, want) in [
        (
            "Merge pull request #12 from acme/notes\n\nOrders get a note",
            Some(12),
        ),
        ("Add region filter (#7)", Some(7)),
        (
            "Merge branch 'x' into 'main'\n\nSee merge request group/repo!31",
            Some(31),
        ),
        (
            "Merge branch 'x' into 'main'\n\nSee merge request g/sub/repo!9\n\nMore",
            Some(9),
        ),
        ("Fix typo", None),
        ("Mention #12 in passing", None),
        ("Revert (#x)", None),
        ("Merge pull request #", None),
        ("", None),
    ] {
        assert_eq!(pull_number(message), want, "{message:?}");
    }
}

#[test]
fn a_commit_is_split_into_summary_and_body() {
    let c = ChangelogCommit::from_message(
        "abc",
        "Merge pull request #12 from a/b\n\nOrders get a note\n",
        "Ada",
    );
    assert_eq!(
        c,
        ChangelogCommit {
            oid: "abc".into(),
            summary: "Merge pull request #12 from a/b".into(),
            body: "Orders get a note".into(),
            author: "Ada".into(),
            pull: Some(12),
        }
    );
    assert_eq!(ChangelogCommit::from_message("a", "Only", "x").body, "");
}
