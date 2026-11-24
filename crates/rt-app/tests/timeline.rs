// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand D: the request history built from versions, blame per field over five
//! commits, the results records (limit, order, results file name) and reading an old version.

use std::collections::BTreeMap;

use rt_app::git::{GitCommit, GitFileChange, GitFileCommit};
use rt_app::timeline::{
    self, RECORDS_PER_REQUEST, RecordSource, RecordedTest, TestRecord, Version, results_file,
};
use rt_format::diff::DiffValue;

fn request(method: &str, url: &str, headers: &str) -> String {
    format!(
        "info:\n  name: Users\n  type: http\n  seq: 1\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n{headers}  auth: inherit\n"
    )
}

fn commit(n: u32) -> GitCommit {
    GitCommit {
        oid: format!("{n:040x}"),
        summary: format!("commit {n}"),
        author: "Ada".into(),
        time: 1_700_000_000 + i64::from(n),
    }
}

fn version(n: u32, change: GitFileChange, text: Option<String>) -> Version {
    Version {
        commit: GitFileCommit {
            commit: commit(n),
            path: "users.yml".into(),
            change,
            from: None,
        },
        text,
    }
}

/// Five commits, newest first.
fn history() -> Vec<Version> {
    use GitFileChange::*;
    let h1 = "  headers:\n    - name: X-A\n      value: one\n";
    let h2 = "  headers:\n    - name: X-A\n      value: one\n    - name: X-B\n      value: two\n";
    let h3 = "  headers:\n    - name: X-A\n      value: three\n    - name: X-B\n      value: two\n";
    vec![
        version(5, Modified, Some(request("POST", "{{base}}/b", h3))),
        version(4, Modified, Some(request("GET", "{{base}}/b", h3))),
        version(3, Modified, Some(request("GET", "{{base}}/b", h2))),
        version(2, Modified, Some(request("GET", "{{base}}/b", h1))),
        version(1, Added, Some(request("GET", "{{base}}/a", h1))),
    ]
}

fn blamed(blame: &[timeline::FieldBlame], path: &[&str]) -> u32 {
    let b = blame
        .iter()
        .find(|b| b.field == path)
        .unwrap_or_else(|| panic!("no blame for {path:?}: {blame:?}"));
    u32::from_str_radix(&b.commit.oid, 16).unwrap()
}

#[test]
fn blame_names_the_last_commit_per_field() {
    let blame = timeline::blame(&history());
    assert_eq!(blamed(&blame, &["method"]), 5);
    assert_eq!(blamed(&blame, &["url"]), 2);
    assert_eq!(blamed(&blame, &["name"]), 1);
    assert_eq!(blamed(&blame, &["headers", "X-A"]), 4);
    assert_eq!(blamed(&blame, &["headers", "X-B"]), 3);
    let url = blame.iter().find(|b| b.field == ["url"]).unwrap();
    assert_eq!(
        url.value,
        Some(DiffValue::Text {
            text: "{{base}}/b".into()
        })
    );
    assert_eq!(url.commit.summary, "commit 2");
}

#[test]
fn blame_ignores_removed_fields_and_deleted_requests() {
    let mut h = history();
    // The newest version drops X-B: it has no blame, the others stay.
    h.insert(
        0,
        version(
            6,
            GitFileChange::Modified,
            Some(request(
                "POST",
                "{{base}}/b",
                "  headers:\n    - name: X-A\n      value: three\n",
            )),
        ),
    );
    let blame = timeline::blame(&h);
    assert!(blame.iter().all(|b| b.field != ["headers", "X-B"]));
    assert_eq!(blamed(&blame, &["headers", "X-A"]), 4);
    // A request deleted at HEAD has nothing to blame.
    h.insert(0, version(7, GitFileChange::Deleted, None));
    assert!(timeline::blame(&h).is_empty());
}

#[test]
fn credentials_in_values_stay_masked() {
    let secret = "  headers:\n    - name: Authorization\n      value: Bearer eyJhbGciOiJIUzI1NiJ9.abcdefghijklmnop.qrstuvwxyz0123456789\n";
    let h = vec![version(
        1,
        GitFileChange::Added,
        Some(request("GET", "{{base}}/x", secret)),
    )];
    let t = timeline::build("users.yml", "", &h, &BTreeMap::new());
    let f = t.entries[0]
        .fields
        .iter()
        .find(|f| f.path == ["headers", "Authorization"])
        .expect("the header is listed");
    assert_eq!(f.new, Some(DiffValue::Secret));
    assert!(!serde_json::to_string(&t).unwrap().contains("eyJhbGci"));
    let blame = timeline::blame(&h);
    let b = blame
        .iter()
        .find(|b| b.field == ["headers", "Authorization"])
        .unwrap();
    assert_eq!(b.value, Some(DiffValue::Secret));
}

fn record(request: &str, time: i64, passed: bool) -> TestRecord {
    TestRecord {
        collection: String::new(),
        request: request.into(),
        environment: Some("Local".into()),
        time,
        source: RecordSource::Runner,
        passed,
        status: Some(200),
        tests: vec![RecordedTest {
            name: "status is 200".into(),
            passed,
        }],
        duration_ms: None,
    }
}

#[test]
fn timeline_lists_fields_per_commit_and_records_per_request() {
    let mut records = BTreeMap::new();
    records.insert(
        commit(5).oid,
        vec![
            record("users.yml", 10, true),
            record("users.yml", 30, false),
            record("other.yml", 20, true),
        ],
    );
    let t = timeline::build("users.yml", "", &history(), &records);
    assert_eq!(t.path, "users.yml");
    assert_eq!(t.entries.len(), 5);
    let newest = &t.entries[0];
    let paths: Vec<_> = newest.fields.iter().map(|f| f.path.join("/")).collect();
    assert_eq!(paths, ["method"]);
    // Newest first, only this request.
    assert_eq!(
        newest.tests.iter().map(|r| r.time).collect::<Vec<_>>(),
        [30, 10]
    );
    assert!(t.entries[1].tests.is_empty());
    // The first version lists everything it set.
    let first: Vec<_> = t.entries[4]
        .fields
        .iter()
        .map(|f| f.path.join("/"))
        .collect();
    assert!(first.contains(&"url".to_string()) && first.contains(&"headers/X-A".to_string()));
    // Another collection's records do not show.
    let other = timeline::build("users.yml", "api", &history(), &records);
    assert!(other.entries[0].tests.is_empty());
}

#[test]
fn records_keep_twenty_per_request_and_commit() {
    let mut text = String::new();
    for i in 0..25 {
        text = timeline::merge_records(&text, &[record("a.yml", i, true)]);
    }
    text = timeline::merge_records(&text, &[record("b.yml", 99, false)]);
    let all = timeline::parse_records(&text);
    let a: Vec<i64> = all
        .iter()
        .filter(|r| r.request == "a.yml")
        .map(|r| r.time)
        .collect();
    assert_eq!(a.len(), RECORDS_PER_REQUEST);
    assert_eq!(a.first(), Some(&5));
    assert_eq!(a.last(), Some(&24));
    assert_eq!(all.iter().filter(|r| r.request == "b.yml").count(), 1);
    // Damaged lines are skipped, not fatal.
    assert_eq!(timeline::parse_records("not json\n").len(), 0);
    assert_eq!(
        timeline::parse_records(&format!("garbage\n{text}")).len(),
        all.len()
    );
}

#[test]
fn results_files_are_fanned_out_by_two_hex_digits() {
    assert_eq!(results_file("ab12cd"), "test-results/ab/ab12cd.jsonl");
}

#[test]
fn an_old_version_reads_as_a_request() {
    let spec = timeline::spec_of(&request("PUT", "{{base}}/x", "")).unwrap();
    assert_eq!(spec.method, "PUT");
    assert_eq!(spec.url, "{{base}}/x");
    assert!(timeline::spec_of("not: a request\n").is_err());
}
