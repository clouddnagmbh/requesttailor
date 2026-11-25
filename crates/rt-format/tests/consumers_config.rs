// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.consumers` (wave 19, strand F): read; `opencollection.yml` stays
//! byte for byte through reading, writing and every other writer of the file.

use rt_core::model::{AgentAccess, AgentPolicy};
use rt_format::consumers_config::{MAX_CONSUMERS, configured};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const BASE: &str = "opencollection: 1.0.0\ninfo:\n  name: orders\nextensions:\n  requesttailor:\n    consumers:\n      - name: billing\n        source: ../billing # next to this repo\n        environment: ci\n        vars: { baseUrl: \"{{provider.baseUrl}}\", tenant: \"a\" }\n      - name: portal\n        source: https://git.example.test/team/portal.git\n        ref: v2\n        path: api-tests\n        environment: ci\n    http3: true\n";

fn set(yaml: &str) -> FileSet {
    FileSet {
        files: [(COLLECTION_FILE.to_owned(), yaml.as_bytes().to_vec())].into(),
    }
}

fn text(s: &FileSet) -> String {
    String::from_utf8(s.files[COLLECTION_FILE].clone()).unwrap()
}

#[test]
fn reads_every_field_of_every_entry() {
    let list = configured(&set(BASE)).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "billing");
    assert_eq!(list[0].source, "../billing");
    assert_eq!(list[0].environment, "ci");
    assert_eq!(list[0].git_ref, None);
    assert_eq!(
        list[0].vars,
        vec![
            ("baseUrl".to_owned(), "{{provider.baseUrl}}".to_owned()),
            ("tenant".to_owned(), "a".to_owned())
        ]
    );
    assert_eq!(list[1].git_ref.as_deref(), Some("v2"));
    assert_eq!(list[1].path.as_deref(), Some("api-tests"));
    assert!(list[1].vars.is_empty());
}

#[test]
fn no_list_is_an_empty_list() {
    assert!(
        configured(&set("opencollection: 1.0.0\ninfo:\n  name: a\n"))
            .unwrap()
            .is_empty()
    );
    assert!(configured(&FileSet::default()).unwrap().is_empty());
}

#[test]
fn an_unusable_entry_is_an_error_that_names_it() {
    for (yaml, needle) in [
        ("      - name: a\n        environment: ci\n", "source"),
        ("      - name: a\n        source: x\n", "environment"),
        ("      - source: x\n        environment: ci\n", "name"),
        ("      - just text\n", "consumers[0]"),
        (
            "      - {name: a, source: x, environment: ci}\n      - {name: a, source: y, environment: ci}\n",
            "twice",
        ),
    ] {
        let doc = format!(
            "opencollection: 1.0.0\ninfo:\n  name: a\nextensions:\n  requesttailor:\n    consumers:\n{yaml}"
        );
        let err = configured(&set(&doc)).unwrap_err().to_string();
        assert!(err.contains(needle), "{needle}: {err}");
    }
}

#[test]
fn too_many_entries_are_refused() {
    let mut doc = String::from(
        "opencollection: 1.0.0\ninfo:\n  name: a\nextensions:\n  requesttailor:\n    consumers:\n",
    );
    for i in 0..=MAX_CONSUMERS {
        doc.push_str(&format!(
            "      - {{name: c{i}, source: x, environment: ci}}\n"
        ));
    }
    assert!(
        configured(&set(&doc))
            .unwrap_err()
            .to_string()
            .contains("at most")
    );
}

#[test]
fn reading_and_writing_keep_the_file_byte_for_byte() {
    let doc = read_collection(&set(BASE)).unwrap();
    assert_eq!(text(&doc.write()), BASE);
}

#[test]
fn other_writers_leave_the_consumers_block_alone() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    doc.set_agent_policy(&AgentPolicy {
        access: AgentAccess::Write,
        environments: vec![],
    })
    .unwrap();
    doc.set_contract_spec("api/openapi.json").unwrap();
    let written = doc.write();
    assert!(text(&written).contains("        source: ../billing # next to this repo\n"));
    assert_eq!(configured(&written).unwrap().len(), 2);
}
