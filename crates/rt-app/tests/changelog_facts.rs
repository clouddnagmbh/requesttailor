// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 16 interface: the facts a changelog writer builds on are the deterministic changelog
//! and its parts, nothing else.

use rt_app::AppError;
use rt_app::contract::{ChangelogCommit, ChangelogSection, Side, changelog_facts, changelog_with};
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
