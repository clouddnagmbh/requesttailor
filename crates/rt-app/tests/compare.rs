// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs at a revision and the comparison of two runs (wave 12, strand C): the requests come
//! from the revision's tree, the environments from today, answers are compared per request.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use rt_app::bisect::{BisectVerdict, test_revision};
use rt_app::compare::{
    CompareRequest, RevisionRunRequest, compare_revisions, run_revision, snapshot_at,
};
use rt_app::git::{GitCommit, GitTree};
use rt_app::runner::RunSnapshot;
use rt_app::timeline::{RecordSource, TestRecord, records_of_steps};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// `/v1/*` answers like the old API, `/v2/*` like the new one, `/same` the same both times,
/// `/text` plain text that differs by the `v` query, `/boom` fails.
struct Wire;

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let url = request.url.clone();
        let (status, headers, body): (u16, Vec<(&str, &str)>, &str) = if url.contains("/v1/") {
            (
                200,
                vec![("X-Version", "1"), ("Date", "Mon")],
                r#"{"id":1,"tags":["a"],"owner":{"name":"ada"},"old":true}"#,
            )
        } else if url.contains("/v2/") {
            (
                404,
                vec![("X-Version", "2"), ("Date", "Tue"), ("X-New", "yes")],
                r#"{"id":2,"tags":["a","b"],"owner":{"name":"ada"},"fresh":1}"#,
            )
        } else if url.contains("/text?v=1") {
            (200, vec![], "hello")
        } else if url.contains("/text?v=2") {
            (200, vec![], "world")
        } else if url.contains("/boom") {
            return Err(TransportError::Connect("refused".into()));
        } else {
            (200, vec![("Date", "Wed")], r#"{"same":true}"#)
        };
        let body: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers: headers
                    .into_iter()
                    .map(|(a, b)| (a.to_owned(), b.to_owned()))
                    .collect(),
                url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body,
        })
    }
}

fn request(name: &str, seq: u32, path: &str, assert: bool) -> (String, String) {
    let assertions = if assert {
        "\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n"
    } else {
        ""
    };
    (
        format!("{name}.yml"),
        format!(
            "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}{path}\"\n{assertions}"
        ),
    )
}

fn tree(oid: &str, requests: &[(String, String)]) -> GitTree {
    let mut files = std::collections::BTreeMap::new();
    files.insert(
        COLLECTION_FILE.to_owned(),
        "opencollection: \"1.0.0\"\ninfo:\n  name: c\n".to_owned(),
    );
    // The revision's own environment is ignored: today's wins.
    files.insert(
        "environments/Local.yml".to_owned(),
        "name: Local\nvariables:\n  - name: base\n    value: http://stale.invalid\n".to_owned(),
    );
    for (p, t) in requests {
        files.insert(p.clone(), t.clone());
    }
    GitTree {
        oid: oid.to_owned(),
        files,
    }
}

fn commit(oid: &str) -> GitCommit {
    GitCommit {
        oid: oid.to_owned(),
        summary: format!("commit {oid}"),
        author: "t".into(),
        time: 0,
    }
}

fn today() -> RunSnapshot {
    let files = FileSet {
        files: [
            (
                COLLECTION_FILE.to_owned(),
                b"opencollection: \"1.0.0\"\ninfo:\n  name: now\n".to_vec(),
            ),
            (
                "environments/Local.yml".to_owned(),
                b"name: Local\nvariables:\n  - name: base\n    value: http://api.test\n".to_vec(),
            ),
        ]
        .into_iter()
        .collect(),
    };
    RunSnapshot {
        collection: read_collection(&files).unwrap().model().clone(),
        environment: Some("Local".into()),
        vars: Vec::new(),
        files: FileSet::default(),
    }
}

fn never() -> bool {
    false
}

#[test]
fn a_revision_runs_with_todays_environment() {
    let t = tree(
        &"a".repeat(40),
        &[
            request("one", 1, "/v1/x", true),
            request("two", 2, "/same", false),
        ],
    );
    let run = block_on(run_revision(
        &today(),
        commit("a1"),
        &t,
        &RevisionRunRequest {
            rev: "a1".into(),
            folder: None,
            paths: None,
        },
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |_| {},
        &never,
    ))
    .unwrap();
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.summary.passed, 2);
    // Today's base, not the revision's stale one.
    assert_eq!(
        run.steps[0].report.request.as_ref().unwrap().url,
        "http://api.test/v1/x"
    );
    // `paths` selects and orders; paths that do not exist at the revision are left out.
    let run = block_on(run_revision(
        &today(),
        commit("a1"),
        &t,
        &RevisionRunRequest {
            rev: "a1".into(),
            folder: None,
            paths: Some(vec!["two.yml".into(), "gone.yml".into(), "one.yml".into()]),
        },
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |_| {},
        &never,
    ))
    .unwrap();
    let paths: Vec<_> = run.steps.iter().map(|s| s.request_path.as_str()).collect();
    assert_eq!(paths, ["two.yml", "one.yml"]);
}

#[test]
fn an_unreadable_revision_is_an_error() {
    let empty = GitTree {
        oid: "b".repeat(40),
        files: Default::default(),
    };
    assert!(snapshot_at(&today(), &empty).is_err());
}

#[test]
fn compares_status_headers_body_tests_and_errors() {
    let from = tree(
        &"a".repeat(40),
        &[
            request("api", 1, "/v1/x", true),
            request("same", 2, "/same", false),
            request("text", 3, "/text?v=1", false),
            request("boom", 4, "/boom", false),
            request("gone", 5, "/same", false),
        ],
    );
    let to = tree(
        &"b".repeat(40),
        &[
            request("api", 1, "/v2/x", true),
            request("same", 2, "/same", false),
            request("text", 3, "/text?v=2", false),
            request("boom", 4, "/v1/x", false),
            request("added", 6, "/same", false),
        ],
    );
    let mut runs: Vec<(String, Vec<TestRecord>)> = Vec::new();
    let cmp = block_on(compare_revisions(
        &today(),
        (commit("a"), &from),
        (commit("b"), &to),
        &CompareRequest {
            from: "a".into(),
            to: "b".into(),
            folder: None,
            paths: None,
        },
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |c, run| {
            let records = records_of_steps("", 7, RecordSource::Compare, &run.steps);
            runs.push((c.oid.clone(), records));
        },
        &never,
    ))
    .unwrap();
    // Each side's run goes to the time travel under its own commit.
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].0.as_str(), runs[1].0.as_str()), ("a", "b"));
    let api_before = runs[0].1.iter().find(|r| r.request == "api.yml").unwrap();
    assert!(api_before.passed && api_before.source == RecordSource::Compare);
    assert_eq!(api_before.status, Some(200));
    let api_after = runs[1].1.iter().find(|r| r.request == "api.yml").unwrap();
    assert!(!api_after.passed);
    assert_eq!(api_after.tests[0].name, "res.status eq 200");
    assert!(!api_after.tests[0].passed);
    let by = |p: &str| cmp.requests.iter().find(|r| r.path == p).unwrap();

    let api = by("api.yml");
    assert!(!api.same);
    let status = api.status.as_ref().unwrap();
    assert_eq!((status.old, status.new), (Some(200), Some(404)));
    // Date is volatile; X-Version changed; X-New appeared.
    let names: Vec<_> = api.headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["X-New", "X-Version"]);
    assert_eq!(api.headers[0].old, None);
    assert_eq!(api.headers[0].new.as_deref(), Some("yes"));
    // JSON Pointer changes, sorted by key; unchanged `/owner` is not listed.
    let pointers: Vec<_> = api.body.iter().map(|b| b.pointer.as_str()).collect();
    assert_eq!(pointers, ["/fresh", "/id", "/old", "/tags/1"]);
    let id = api.body.iter().find(|b| b.pointer == "/id").unwrap();
    assert_eq!(
        (id.old.clone(), id.new.clone()),
        (Some(1.into()), Some(2.into()))
    );
    let old = api.body.iter().find(|b| b.pointer == "/old").unwrap();
    assert_eq!(old.new, None);
    assert!(!api.body_text);
    // The assertion `res.status eq 200` passed before and fails now.
    assert_eq!(api.tests.len(), 1);
    assert_eq!(api.tests[0].name, "res.status eq 200");
    assert_eq!(
        (api.tests[0].old, api.tests[0].new),
        (Some(true), Some(false))
    );

    assert!(by("same.yml").same, "{:?}", by("same.yml"));

    let text = by("text.yml");
    assert!(text.body_text && text.body.is_empty() && text.status.is_none());

    // An error on one side only.
    let boom = by("boom.yml");
    assert_eq!(
        boom.error.as_ref().unwrap().old.as_deref(),
        Some("connection failed: refused")
    );
    assert_eq!(boom.error.as_ref().unwrap().new, None);
    assert_eq!(boom.status.as_ref().unwrap().old, None);

    // Requests on one side only.
    let gone = by("gone.yml");
    assert!(!gone.same);
    assert_eq!(gone.status.as_ref().unwrap().new, None);
    let added = by("added.yml");
    assert_eq!(added.status.as_ref().unwrap().old, None);
    assert_eq!(added.status.as_ref().unwrap().new, Some(200));

    assert_eq!(cmp.from.oid, "a");
    assert_eq!(cmp.to.oid, "b");
    assert_eq!(cmp.from_summary.requests, 5);
    assert_eq!(cmp.to_summary.requests, 5);
}

#[test]
fn a_folder_limits_the_comparison() {
    let mut from = tree(&"a".repeat(40), &[request("top", 1, "/same", false)]);
    from.files.insert(
        "api/folder.yml".into(),
        "info:\n  name: api\n  type: folder\n  seq: 2\n".into(),
    );
    from.files
        .insert("api/in.yml".into(), request("in", 1, "/same", false).1);
    let cmp = block_on(compare_revisions(
        &today(),
        (commit("a"), &from),
        (commit("a"), &from),
        &CompareRequest {
            from: "a".into(),
            to: "a".into(),
            folder: Some("api".into()),
            paths: None,
        },
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |_, _| {},
        &never,
    ))
    .unwrap();
    assert_eq!(cmp.requests.len(), 1);
    assert_eq!(cmp.requests[0].path, "api/in.yml");
    assert!(cmp.requests[0].same);
}

/// Bisect hands each tested commit's run to the time travel (`RecordSource::Bisect`); a
/// skipped commit ran nothing and records nothing.
#[test]
fn a_bisect_test_hands_its_run_to_the_time_travel() {
    let at = tree(
        &"c".repeat(40),
        &[
            request("api", 1, "/v2/x", true),
            request("same", 2, "/same", false),
        ],
    );
    let mut records = Vec::new();
    let (verdict, failed) = block_on(test_revision(
        &today(),
        Ok(at),
        None,
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |run| records.extend(records_of_steps("", 9, RecordSource::Bisect, &run.steps)),
        &never,
    ));
    assert_eq!(verdict, BisectVerdict::Bad, "{failed:?}");
    assert_eq!(records.len(), 2);
    let api = records.iter().find(|r| r.request == "api.yml").unwrap();
    assert_eq!(
        (api.passed, api.status, api.source, api.time),
        (false, Some(404), RecordSource::Bisect, 9)
    );
    assert!(
        records
            .iter()
            .find(|r| r.request == "same.yml")
            .unwrap()
            .passed
    );

    let mut ran = false;
    let (verdict, _) = block_on(test_revision(
        &today(),
        Err("not readable".into()),
        None,
        &Wire,
        &SendOptions::default(),
        &Services::none(),
        &mut |_| ran = true,
        &never,
    ));
    assert_eq!(verdict, BisectVerdict::Skip);
    assert!(!ran);
}
