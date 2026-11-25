// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Consumer contracts (wave 19, strand F): two local consumers and one cloned consumer run
//! against a built provider with a deliberate change. The breaking consumer and request are
//! named, writing requests need `allow_writes` and never run against production, clones are
//! shallow and limited, and no secret or personal value reaches the report.

mod consumers_support;

use std::sync::atomic::Ordering;

use consumers_support::*;
use rt_app::consumers::{self, fetch};

/// `file://` URL of a local path that `git` accepts: `file:///C:/x` on Windows, `file:///x` elsewhere.
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

fn clone_entry(repo: &std::path::Path) -> String {
    format!(
        "      - name: mobile\n        source: {}\n        environment: ci\n",
        file_url(repo)
    )
}

#[test]
fn the_breaking_consumer_and_request_are_named() {
    let tmp = Temp::new("break");
    let repo = mobile_repo(&tmp.0);
    let root = provider_collection(&tmp.0, &format!("{LOCAL_TWO}{}", clone_entry(&repo)));
    let provider = provider();
    provider.changed.store(true, Ordering::SeqCst);
    let report = verify(&root, &tmp.0.join("data"), &request(&provider.base));

    assert_eq!(report.consumers.len(), 3);
    assert!(report.broken());
    let billing = &report.consumers[0];
    assert_eq!(billing.name, "billing");
    assert_eq!(billing.failures.len(), 1, "{billing:?}");
    let f = &billing.failures[0];
    assert_eq!(f.request, "GET /orders/{orderId}");
    assert_eq!(f.request_path, "get-order.yml");
    assert!(f.assertion.contains("res.body.items[0].price"), "{f:?}");
    assert!(f.assertion.contains("isNumber"), "{f:?}");
    assert!(f.difference.contains("got string"), "{f:?}");
    assert_eq!(billing.requests, 1);
    assert_eq!(
        billing.skipped, 1,
        "the POST is left out without allow_writes"
    );

    assert!(
        report.consumers[1].failures.is_empty(),
        "{:?}",
        report.consumers[1]
    );
    assert_eq!(report.consumers[1].commit, None);
    let mobile = &report.consumers[2];
    assert!(
        mobile.failures.is_empty() && mobile.error.is_none(),
        "{mobile:?}"
    );
    assert_eq!(mobile.commit.as_deref().map(str::len), Some(40));

    let text = consumers::text(&report);
    assert!(text.contains("Consumer billing breaks"), "{text}");
    assert!(text.contains("GET /orders/{orderId}"), "{text}");
    assert!(text.contains("Consumer portal is fine"), "{text}");
    assert!(text.contains("1 of 3 consumers broken"), "{text}");
    let github = consumers::github(&report);
    assert!(
        github.starts_with("::error title=Consumer billing breaks%3A GET /orders/{orderId}::"),
        "{github}"
    );
    assert_eq!(github.lines().count(), 1, "{github}");
}

#[test]
fn an_unchanged_provider_keeps_every_consumer_green() {
    let tmp = Temp::new("green");
    let root = provider_collection(&tmp.0, LOCAL_TWO);
    let provider = provider();
    let report = verify(&root, &tmp.0.join("data"), &request(&provider.base));
    assert!(!report.broken(), "{report:?}");
    assert_eq!(report.consumers[0].requests, 1);
}

#[test]
fn only_names_the_consumers_to_run_and_a_typo_is_an_error() {
    let tmp = Temp::new("only");
    let root = provider_collection(&tmp.0, LOCAL_TWO);
    let provider = provider();
    let mut req = request(&provider.base);
    req.only = vec!["portal".into()];
    let report = verify(&root, &tmp.0.join("data"), &req);
    assert_eq!(report.consumers.len(), 1);
    assert_eq!(report.consumers[0].name, "portal");
    let files = rt_format::FileSet::from_dir(&root).unwrap();
    let listed = consumers::list(&files).unwrap();
    let err = consumers::select(&listed, &["portl".into()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("portl") && err.contains("billing"), "{err}");
}

#[test]
fn writing_requests_run_only_with_allow_writes() {
    let tmp = Temp::new("writes");
    let root = provider_collection(&tmp.0, LOCAL_TWO);
    let provider = provider();
    let mut req = request(&provider.base);
    req.only = vec!["billing".into()];
    verify(&root, &tmp.0.join("data"), &req);
    assert!(
        !provider
            .hits
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.starts_with("POST")),
        "{:?}",
        provider.hits.lock().unwrap()
    );
    req.allow_writes = true;
    let report = verify(&root, &tmp.0.join("data"), &req);
    assert_eq!(
        provider
            .hits
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.starts_with("POST"))
            .count(),
        1
    );
    assert_eq!(report.consumers[0].requests, 2);
    assert_eq!(report.consumers[0].skipped, 0);
    assert!(!report.broken(), "{report:?}");
}

#[test]
fn writing_requests_never_run_against_a_production_environment() {
    let tmp = Temp::new("prod");
    let root = provider_collection(&tmp.0, LOCAL_TWO);
    let env = tmp.0.join("billing/environments");
    std::fs::rename(env.join("ci.yml"), env.join("prod.yml")).unwrap();
    let text = std::fs::read_to_string(env.join("prod.yml"))
        .unwrap()
        .replace("name: ci", "name: prod");
    std::fs::write(env.join("prod.yml"), text).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        std::fs::read_to_string(root.join("opencollection.yml"))
            .unwrap()
            .replacen("environment: ci", "environment: prod", 1),
    )
    .unwrap();
    let provider = provider();
    let mut req = request(&provider.base);
    req.only = vec!["billing".into()];
    req.allow_writes = true;
    let report = verify(&root, &tmp.0.join("data"), &req);
    assert!(
        !provider
            .hits
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.starts_with("POST")),
        "a write reached the provider"
    );
    assert_eq!(report.consumers[0].skipped, 1);
    assert!(!report.broken(), "reads still run: {report:?}");
}

#[test]
fn a_clone_is_shallow_at_the_named_ref_and_reused_until_fetch() {
    let tmp = Temp::new("clone");
    let repo = mobile_repo(&tmp.0);
    let first = git(&repo, &["rev-parse", "HEAD~1"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let root = provider_collection(&tmp.0, &clone_entry(&repo));
    let provider = provider();
    let data = tmp.0.join("data");
    let report = verify(&root, &data, &request(&provider.base));
    assert_eq!(report.consumers[0].commit.as_deref(), Some(head.as_str()));

    let dir = std::fs::read_dir(data.join("consumers"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "1", "shallow");
    assert!(dir.join(".git/shallow").is_file());

    // A new commit upstream is not seen without --fetch, and is seen with it.
    std::fs::write(repo.join("more.md"), "more\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "third"]);
    let newest = git(&repo, &["rev-parse", "HEAD"]);
    let again = verify(&root, &data, &request(&provider.base));
    assert_eq!(again.consumers[0].commit.as_deref(), Some(head.as_str()));
    let mut req = request(&provider.base);
    req.fetch = true;
    let fresh = verify(&root, &data, &req);
    assert_eq!(fresh.consumers[0].commit.as_deref(), Some(newest.as_str()));

    // A ref names a commit exactly.
    let pinned = provider_collection(
        &tmp.0,
        &format!("{}        ref: {first}\n", clone_entry(&repo)),
    );
    let at = verify(&pinned, &data, &request(&provider.base));
    assert_eq!(
        at.consumers[0].commit.as_deref(),
        Some(first.as_str()),
        "{at:?}"
    );
}

#[test]
fn a_clone_over_the_size_limit_is_refused_and_removed() {
    let tmp = Temp::new("limit");
    let repo = mobile_repo(&tmp.0);
    let consumer = consumers::ConsumerRef {
        name: "mobile".into(),
        source: file_url(&repo),
        git_ref: None,
        path: None,
        environment: "ci".into(),
        vars: Vec::new(),
    };
    let data = tmp.0.join("data");
    let err = fetch::fetch_with_limit(&consumer, &tmp.0, Some(&data), false, None, 10).unwrap_err();
    assert!(err.contains("larger than"), "{err}");
    let left = std::fs::read_dir(data.join("consumers"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "nothing half-cloned stays");
}

#[test]
fn a_git_token_goes_to_git_in_a_header_and_never_into_the_error() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let s = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            s.lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase());
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let tmp = Temp::new("token");
    let consumer = consumers::ConsumerRef {
        name: "private".into(),
        source: format!("http://127.0.0.1:{port}/team/private.git"),
        git_ref: None,
        path: None,
        environment: "ci".into(),
        vars: Vec::new(),
    };
    let err = fetch::fetch(
        &consumer,
        &tmp.0,
        Some(&tmp.0.join("data")),
        false,
        Some(SECRET),
    )
    .unwrap_err();
    assert!(!err.contains(SECRET), "{err}");
    let seen = seen.lock().unwrap().clone();
    use base64::Engine as _;
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("git:{SECRET}"))
        .to_ascii_lowercase();
    assert!(
        seen.contains(&format!("authorization: basic {basic}")),
        "{seen}"
    );
}

#[test]
fn neither_secret_nor_personal_value_reaches_any_output() {
    let tmp = Temp::new("canary");
    let root = provider_collection(&tmp.0, LOCAL_TWO);
    let provider = provider();
    provider.changed.store(true, Ordering::SeqCst);
    let report = verify(&root, &tmp.0.join("data"), &request(&provider.base));
    assert!(report.broken());
    let json = serde_json::to_string(&report).unwrap();
    for out in [json, consumers::text(&report), consumers::github(&report)] {
        assert!(!out.contains(SECRET), "{out}");
        assert!(!out.contains(PERSONAL), "{out}");
        assert!(!out.contains("canary"), "{out}");
    }
}

#[test]
fn a_consumer_that_cannot_be_found_or_run_is_reported_not_hidden() {
    let tmp = Temp::new("missing");
    let root = provider_collection(
        &tmp.0,
        "      - name: ghost\n        source: ../nowhere\n        environment: ci\n      - name: billing\n        source: ../billing\n        environment: staging\n",
    );
    let provider = provider();
    let report = verify(&root, &tmp.0.join("data"), &request(&provider.base));
    assert!(report.broken());
    assert!(
        report.consumers[0]
            .error
            .as_deref()
            .unwrap()
            .contains("ghost")
    );
    let e = report.consumers[1].error.as_deref().unwrap();
    assert!(e.contains("staging") && e.contains("ci"), "{e}");
    assert!(consumers::text(&report).contains("could not be verified"));
}
