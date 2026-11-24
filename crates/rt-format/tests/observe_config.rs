// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.observe` (wave 17): read; `opencollection.yml` stays byte for byte
//! through reading, writing and every other writer of the file.

use rt_core::model::{AgentAccess, AgentPolicy};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const BASE: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\nextensions:\n  requesttailor:\n    observe:\n      enabled: false # the team decided\n    http3: true\n";

fn set(yaml: &str) -> FileSet {
    FileSet {
        files: [(COLLECTION_FILE.to_owned(), yaml.as_bytes().to_vec())].into(),
    }
}

fn text(s: &FileSet) -> String {
    String::from_utf8(s.files[COLLECTION_FILE].clone()).unwrap()
}

#[test]
fn only_an_explicit_false_turns_delivery_off() {
    let enabled = |y: &str| {
        read_collection(&set(y))
            .unwrap()
            .model()
            .config
            .observe
            .enabled
    };
    assert!(!enabled(BASE));
    assert!(enabled("opencollection: 1.0.0\ninfo:\n  name: a\n"));
    assert!(enabled(&BASE.replace("false", "true")));
    assert!(enabled(&BASE.replace("false", "maybe")));
    assert!(enabled(&BASE.replace("enabled: false", "other: 1")));
}

#[test]
fn reading_and_writing_keep_the_file_byte_for_byte() {
    let doc = read_collection(&set(BASE)).unwrap();
    assert_eq!(text(&doc.write()), BASE);
}

#[test]
fn other_writers_leave_the_observe_block_alone() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    doc.set_agent_policy(&AgentPolicy {
        access: AgentAccess::Write,
        environments: vec![],
    })
    .unwrap();
    doc.set_contract_spec("api/openapi.json").unwrap();
    let written = text(&doc.write());
    assert!(
        written.contains("    observe:\n      enabled: false # the team decided\n"),
        "{written}"
    );
    let back = read_collection(&doc.write()).unwrap();
    assert!(!back.model().config.observe.enabled);
    assert_eq!(back.model().config.agents.access, AgentAccess::Write);
}
