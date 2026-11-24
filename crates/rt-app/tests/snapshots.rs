// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests (wave 17, strand B): freeze a response, compare, ignore a path, accept a
//! new response, and the assertion in a real send. Native only (sends to `rt-testserver`).

#![cfg(not(target_arch = "wasm32"))]

use rt_app::snapshots::{self, FreezeOptions, SnapshotState};
use rt_app::{AppError, ResponseStore, SendInput, Workspace};
use rt_core::model::{Auth, Environment, KeyValue, Variable};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use rt_format::{COLLECTION_FILE, FileSet};

const SECRET: &str = "RT-CANARY-snapshot-secret-1";

fn files(port: u16) -> FileSet {
    let list = [
        (COLLECTION_FILE, "opencollection: \"1.0.0\"\ninfo:\n  name: snaps\n".to_owned()),
        (
            "users/get.yml",
            format!("info:\n  name: Get user\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}/echo/users?n={{{{n}}}}\"\n  headers:\n    - name: Authorization\n      value: Bearer {{{{token}}}}\n  auth: inherit\n"),
        ),
        (
            "environments/Local.yml",
            "name: Local\nvariables:\n  - name: n\n    value: \"1\"\n  - secret: true\n    name: token\n".to_owned(),
        ),
    ];
    FileSet {
        files: list
            .into_iter()
            .map(|(p, t)| (p.to_owned(), t.into_bytes()))
            .collect(),
    }
}

fn secrets() -> Vec<String> {
    vec![SECRET.to_owned()]
}

fn input(n: &str) -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        vars: vec![("n".into(), n.into())],
        secrets: vec![("token".into(), SECRET.into())],
    }
}

async fn send(
    ws: &Workspace,
    store: &mut ResponseStore,
    n: &str,
) -> (u64, rt_core::send::SendReport) {
    let snapshot = ws.snapshot("users/get.yml", input(n)).unwrap();
    let transport = NativeTransport::new().unwrap();
    let report = snapshot.send(&transport, &SendOptions::default()).await;
    assert_eq!(report.error, None, "{report:?}");
    let id = store.insert(report.clone()).response_id.unwrap();
    (id, report)
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, f)
}

#[test]
fn freeze_check_ignore_accept_and_send() {
    run(async {
        let port = rt_testserver::spawn_local().await.port();
        let mut ws = Workspace::open(files(port)).unwrap();
        let _ = (
            Auth::default as fn() -> Auth,
            Environment::default as fn() -> Environment,
            Variable::new("a", "b"),
            KeyValue::new("a", "b"),
        );
        let mut store = ResponseStore::new();
        assert!(snapshots::list(&ws).is_empty());

        let (first, _) = send(&ws, &mut store, "1").await;
        assert_eq!(
            snapshots::check(&ws, &store, "users/get.yml", first, &secrets())
                .unwrap()
                .state,
            SnapshotState::None
        );

        // Freeze: the file and the assertion are staged, unsaved.
        let frozen = snapshots::freeze(
            &mut ws,
            &store,
            "users/get.yml",
            first,
            &FreezeOptions::default(),
            &secrets(),
        )
        .unwrap();
        assert!(frozen.assertion_added);
        assert_eq!(frozen.snapshot, "users/get.snap.json");
        let mut dirty = ws.dirty();
        dirty.sort();
        assert_eq!(dirty, ["users/get.snap.json", "users/get.yml"]);
        let spec = ws.request("users/get.yml").unwrap();
        assert_eq!(spec.assertions.len(), 1);
        assert_eq!(spec.assertions[0].operator, "matchesSnapshot");
        assert_eq!(spec.assertions[0].expression, "res");
        assert_eq!(spec.assertions[0].value, "get.snap.json");
        let text =
            String::from_utf8(ws.companion_file("users/get.snap.json").unwrap().to_vec()).unwrap();
        assert!(!text.contains(SECRET), "{text}");
        assert!(text.contains("***"), "the masked token is there: {text}");
        assert!(!text.contains("\"date\""));

        // Freezing again changes nothing about the assertion.
        let again = snapshots::freeze(
            &mut ws,
            &store,
            "users/get.yml",
            first,
            &FreezeOptions::default(),
            &secrets(),
        )
        .unwrap();
        assert!(!again.assertion_added);
        assert_eq!(ws.request("users/get.yml").unwrap().assertions.len(), 1);

        let entries = snapshots::list(&ws);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].exists);

        // The same response matches; another one differs at exactly the changed place.
        assert_eq!(
            snapshots::check(&ws, &store, "users/get.yml", first, &secrets())
                .unwrap()
                .state,
            SnapshotState::Matches
        );
        let (second, report) = send(&ws, &mut store, "2").await;
        // The assertion of the real send sees the staged file: the send carries it.
        assert_eq!(report.assertions.len(), 1);
        assert!(!report.assertions[0].passed, "{:?}", report.assertions);
        let check = snapshots::check(&ws, &store, "users/get.yml", second, &secrets()).unwrap();
        assert_eq!(check.state, SnapshotState::Differs);
        let paths: Vec<&str> = check.differences.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["body.query"], "{:?}", check.differences);

        // Ignoring the path makes it match, in the app and in the send.
        let snap = snapshots::set_ignore(
            &mut ws,
            "users/get.yml",
            vec!["$.query".into(), "$.query".into()],
        )
        .unwrap();
        assert_eq!(snap.ignore, ["$.query"]);
        assert_eq!(
            snapshots::check(&ws, &store, "users/get.yml", second, &secrets())
                .unwrap()
                .state,
            SnapshotState::Matches
        );
        let (_, report) = send(&ws, &mut store, "3").await;
        assert!(report.assertions[0].passed, "{:?}", report.assertions);
        // A bad path is refused and nothing changes.
        assert!(snapshots::set_ignore(&mut ws, "users/get.yml", vec!["nonsense".into()]).is_err());
        assert_eq!(
            snapshots::read(&ws, "users/get.yml")
                .unwrap()
                .unwrap()
                .ignore,
            ["$.query"]
        );

        // Accepting a new response keeps the ignored paths.
        snapshots::freeze(
            &mut ws,
            &store,
            "users/get.yml",
            second,
            &FreezeOptions::default(),
            &secrets(),
        )
        .unwrap();
        assert_eq!(
            snapshots::read(&ws, "users/get.yml")
                .unwrap()
                .unwrap()
                .ignore,
            ["$.query"]
        );

        // Save: no secret in any written file; the request file changed by one assertion.
        let changes = ws.save().unwrap();
        let mut names: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        names.sort();
        assert_eq!(names, ["users/get.snap.json", "users/get.yml"]);
        for c in &changes {
            assert!(!c.text.contains(SECRET), "{}", c.path);
        }
        let request = &changes
            .iter()
            .find(|c| c.path == "users/get.yml")
            .unwrap()
            .text;
        assert!(request.contains("operator: matchesSnapshot"), "{request}");
        assert!(request.contains("value: get.snap.json"), "{request}");
    });
}

#[test]
fn a_missing_file_fails_with_the_hint_and_the_check_says_missing() {
    run(async {
        let port = rt_testserver::spawn_local().await.port();
        let mut ws = Workspace::open(files(port)).unwrap();
        let mut spec = ws.request("users/get.yml").unwrap();
        spec.assertions.push(rt_core::model::Assertion {
            expression: "res".into(),
            operator: "matchesSnapshot".into(),
            value: "get.snap.json".into(),
            description: None,
            enabled: true,
        });
        ws.update_request("users/get.yml", spec).unwrap();
        let mut store = ResponseStore::new();
        let (id, report) = send(&ws, &mut store, "1").await;
        assert!(!report.assertions[0].passed);
        assert_eq!(
            report.assertions[0].error.as_deref(),
            Some("no snapshot yet; run `rt snapshot update`")
        );
        let check = snapshots::check(&ws, &store, "users/get.yml", id, &secrets()).unwrap();
        assert_eq!(check.state, SnapshotState::Missing);
        assert_eq!(check.snapshot, "users/get.snap.json");
        assert!(!snapshots::list(&ws)[0].exists);
    });
}

#[test]
fn a_snapshot_with_a_credential_is_not_written() {
    run(async {
        let port = rt_testserver::spawn_local().await.port();
        let mut ws = Workspace::open(files(port)).unwrap();
        let mut spec = ws.request("users/get.yml").unwrap();
        spec.headers = vec![rt_core::model::KeyValue::new(
            "Authorization",
            "Bearer hardcoded1234567890abcdefghij",
        )];
        ws.update_request("users/get.yml", spec).unwrap();
        let mut store = ResponseStore::new();
        let (id, _) = send(&ws, &mut store, "1").await;
        let err = snapshots::freeze(
            &mut ws,
            &store,
            "users/get.yml",
            id,
            &FreezeOptions::default(),
            &secrets(),
        )
        .unwrap_err();
        assert!(matches!(err, AppError::SecretsFound(_)), "{err:?}");
        assert!(ws.companion_file("users/get.snap.json").is_none());
        assert_eq!(ws.request("users/get.yml").unwrap().assertions.len(), 0);
    });
}

#[test]
fn the_file_name_must_stay_beside_the_request() {
    run(async {
        let port = rt_testserver::spawn_local().await.port();
        let mut ws = Workspace::open(files(port)).unwrap();
        let mut store = ResponseStore::new();
        let (id, _) = send(&ws, &mut store, "1").await;
        for bad in ["../x.snap.json", "/x.snap.json", "x.json"] {
            let err = snapshots::freeze(
                &mut ws,
                &store,
                "users/get.yml",
                id,
                &FreezeOptions {
                    file: Some(bad.into()),
                    ..FreezeOptions::default()
                },
                &secrets(),
            );
            assert!(err.is_err(), "{bad}");
        }
        let ok = snapshots::freeze(
            &mut ws,
            &store,
            "users/get.yml",
            id,
            &FreezeOptions {
                file: Some("custom.snap.json".into()),
                ..FreezeOptions::default()
            },
            &secrets(),
        )
        .unwrap();
        assert_eq!(ok.snapshot, "users/custom.snap.json");
    });
}

#[test]
fn the_body_limit_constants_agree() {
    assert_eq!(
        rt_core::snapshot::BODY_LIMIT,
        rt_app::compare::SNAPSHOT_BODY_LIMIT
    );
}
