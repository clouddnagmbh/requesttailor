// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request history (wave 4, strand E): recording, filtering, pruning, loading, and that no
//! secret value ends up in an entry, also for sends that fail.

use rt_app::history::{HISTORY_LIMIT, scrub_error};
use rt_app::{History, HistoryEntry};
use rt_core::send::{SendError, SendReport};
use rt_core::transport::TransportError;
use serde_json::json;

/// A report as the core produces it; `status` `None` means the transport failed.
fn report(path: &str, url: &str, status: Option<u16>) -> SendReport {
    let response = status.map(|status| {
        json!({
            "status": status, "version": "HTTP/1.1", "headers": [["x-secret-header", "v"]],
            "url": url, "redirects": [],
            "body": { "encoding": "utf8", "data": "response body" },
            "size": 13, "truncated": false,
            "timing": { "dns": null, "connect": 1.0, "tls": null, "ttfb": 5.0 },
            "total": 12.5
        })
    });
    let error = match status {
        Some(_) => serde_json::Value::Null,
        None => {
            json!({ "kind": "transport", "message": { "kind": "connect", "message": "refused" } })
        }
    };
    serde_json::from_value(json!({
        "version": 1,
        "request_path": path,
        "environment": "Local",
        "request": { "method": "GET", "url": url, "headers": [["authorization", "Bearer ***"]], "body_size": 4 },
        "response": response,
        "variables": [],
        "unresolved": [],
        "error": error,
    }))
    .unwrap()
}

fn record(h: &mut History, collection: &str, name: &str, status: Option<u16>) -> HistoryEntry {
    let path = format!("{}.yml", name.to_lowercase().replace(' ', "-"));
    let url = format!("http://api.test/{}", name.to_lowercase().replace(' ', "/"));
    let at = 1_000 + h.entries().len() as i64;
    let entry = h.record(collection, name, "GET", at, &report(&path, &url, status));
    h.prune();
    entry
}

fn names(entries: &[HistoryEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn records_what_was_sent_without_bodies_or_header_values() {
    let mut h = History::new();
    let e = record(&mut h, "c1", "List users", Some(200));
    assert_eq!(e.id, 1);
    assert_eq!(e.at, 1_000);
    assert_eq!(e.collection, "c1");
    assert_eq!(e.request_path, "list-users.yml");
    assert_eq!(e.name, "List users");
    assert_eq!(e.method, "GET");
    assert_eq!(e.environment.as_deref(), Some("Local"));
    assert_eq!(e.url.as_deref(), Some("http://api.test/list/users"));
    assert_eq!(e.status, Some(200));
    assert_eq!(e.error, None);
    assert_eq!(e.duration_ms, Some(12.5));
    assert_eq!(e.size, Some(13));
    assert_eq!(h.entries(), std::slice::from_ref(&e));

    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("response body"));
    assert!(!json.contains("x-secret-header"));
    assert!(!json.contains("authorization"));
}

#[test]
fn records_failed_sends() {
    let mut h = History::new();
    let e = record(&mut h, "c1", "Down", None);
    assert_eq!(e.status, None);
    assert_eq!(e.duration_ms, None);
    assert_eq!(e.size, None);
    assert_eq!(
        e.error,
        Some(SendError::Transport(TransportError::Connect(
            "refused".into()
        )))
    );
    // A send that never got prepared has no URL.
    let mut r = report("bad.yml", "x", None);
    r.request = None;
    r.error = Some(SendError::Prepare("environment \"Nope\" not found".into()));
    let e = h.record("c1", "Bad", "GET", 5, &r);
    assert_eq!(e.url, None);
    assert_eq!(e.id, 2);
}

#[test]
fn lists_newest_first_per_collection_with_limit() {
    let mut h = History::new();
    record(&mut h, "c1", "One", Some(200));
    record(&mut h, "c2", "Other", Some(200));
    record(&mut h, "c1", "Two", Some(200));
    record(&mut h, "c1", "Three", Some(200));
    assert_eq!(names(&h.list("c1", "", 10)), ["Three", "Two", "One"]);
    assert_eq!(names(&h.list("c1", "", 2)), ["Three", "Two"]);
    assert_eq!(names(&h.list("c2", "", 10)), ["Other"]);
    assert!(h.list("nope", "", 10).is_empty());
}

#[test]
fn filters_by_name_method_url_status_environment_and_error() {
    let mut h = History::new();
    record(&mut h, "c", "List users", Some(200));
    record(&mut h, "c", "Create user", Some(201));
    record(&mut h, "c", "Missing", Some(404));
    record(&mut h, "c", "Broken", Some(500));
    record(&mut h, "c", "Down", None);
    let mut r = report("post.yml", "http://other.test/orders", Some(302));
    r.environment = Some("Staging".into());
    h.record("c", "Orders", "POST", 9, &r);

    let q = |query: &str| names(&h.list("c", query, 100)).join(",");
    assert_eq!(q("USER"), "Create user,List users");
    assert_eq!(q("post"), "Orders");
    assert_eq!(q("other.test"), "Orders");
    assert_eq!(q("list/users"), "List users");
    assert_eq!(q("500"), "Broken");
    assert_eq!(q("4xx"), "Missing");
    assert_eq!(q("2xx"), "Create user,List users");
    assert_eq!(q("20"), "Create user,List users");
    assert_eq!(q("3XX"), "Orders");
    assert_eq!(q("staging"), "Orders");
    assert_eq!(q("error"), "Down");
    assert_eq!(q("connect"), "Down");
    // Every word must match.
    assert_eq!(q("user 2xx"), "Create user,List users");
    assert_eq!(q("user 201"), "Create user");
    assert_eq!(q("user 5xx"), "");
    assert_eq!(q("  "), "Orders,Down,Broken,Missing,Create user,List users");
}

#[test]
fn clear_forgets_only_that_collection() {
    let mut h = History::new();
    record(&mut h, "c1", "One", Some(200));
    record(&mut h, "c2", "Other", Some(200));
    h.clear("c1");
    assert!(h.list("c1", "", 10).is_empty());
    assert_eq!(names(&h.list("c2", "", 10)), ["Other"]);
    // Ids keep increasing after a clear.
    assert_eq!(record(&mut h, "c1", "Again", Some(200)).id, 3);
}

#[test]
fn prunes_the_oldest_beyond_the_limit() {
    let mut h = History::new();
    for i in 0..HISTORY_LIMIT {
        h.record(
            "c",
            &format!("R{i}"),
            "GET",
            i as i64,
            &report("r.yml", "u", Some(200)),
        );
        assert!(h.prune().is_empty());
    }
    h.record("c", "Newest", "GET", 0, &report("r.yml", "u", Some(200)));
    let dropped = h.prune();
    assert_eq!(names(&dropped), ["R0"]);
    assert_eq!(h.entries().len(), HISTORY_LIMIT);
    assert_eq!(h.entries()[0].name, "R1");
    assert_eq!(h.list("c", "", 1)[0].name, "Newest");
}

#[test]
fn load_sorts_dedups_prunes_and_continues_ids() {
    let mut h = History::new();
    let a = record(&mut h, "c", "A", Some(200));
    let b = record(&mut h, "c", "B", Some(200));
    let mut b2 = b.clone();
    b2.name = "B again".into();
    let mut loaded = History::load(vec![b.clone(), a.clone(), b2]);
    assert_eq!(names(loaded.entries()), ["A", "B again"]);
    assert_eq!(record(&mut loaded, "c", "C", Some(200)).id, 3);

    let many: Vec<HistoryEntry> = (1..=HISTORY_LIMIT as u64 + 10)
        .map(|id| HistoryEntry { id, ..a.clone() })
        .collect();
    let loaded = History::load(many);
    assert_eq!(loaded.entries().len(), HISTORY_LIMIT);
    assert_eq!(loaded.entries()[0].id, 11);
}

#[test]
fn error_messages_that_quote_the_request_are_scrubbed() {
    const SECRET: &str = "s3cr3t";
    let cases = [
        SendError::Transport(TransportError::InvalidUrl(format!(
            "ftp://h/{SECRET}: only http:// and https:// URLs are supported"
        ))),
        SendError::Transport(TransportError::InvalidUrl(format!(
            "http://h/{SECRET}: Failed to parse URL from http://h/{SECRET}"
        ))),
        SendError::Transport(TransportError::Other(format!(
            "invalid value for header Authorization: \"Bearer {SECRET}\\n\""
        ))),
        SendError::Transport(TransportError::Other(format!(
            "error sending request for url (http://h/?key={SECRET})"
        ))),
        SendError::Transport(TransportError::Blocked(format!(
            "Failed to fetch https://h/{SECRET}"
        ))),
        SendError::Prepare(format!(
            "invalid URL after variable substitution: http://{SECRET}"
        )),
    ];
    for error in &cases {
        let scrubbed = scrub_error(error);
        let text = serde_json::to_string(&scrubbed).unwrap();
        assert!(!text.contains(SECRET), "{error:?} → {text}");
    }
    assert_eq!(
        scrub_error(&cases[0]),
        SendError::Transport(TransportError::InvalidUrl(
            "only http:// and https:// URLs are supported".into()
        ))
    );
    assert_eq!(
        scrub_error(&cases[2]),
        SendError::Transport(TransportError::Other(
            "invalid value for header Authorization".into()
        ))
    );
    // Messages without request data stay as they are.
    let plain = SendError::Transport(TransportError::Connect("127.0.0.1:9: refused".into()));
    assert_eq!(scrub_error(&plain), plain);
    let mut h = History::new();
    let mut r = report("x.yml", "http://h/***", None);
    r.error = Some(cases[2].clone());
    let e = h.record("c", "X", "GET", 0, &r);
    assert_eq!(e.error, Some(scrub_error(&cases[2])));
}

// ---------------------------------------------------------------------------------------------
// End to end: real sends with a secret, recorded; no form of the value is in any entry.

#[cfg(not(target_arch = "wasm32"))]
mod secrets {
    use base64::Engine as _;
    use rt_app::{History, SendInput, Workspace};
    use rt_core::model::{ApiKeyPlacement, Auth, KeyValue, RequestSpec};
    use rt_core::send::SendOptions;
    use rt_core::transport::native::NativeTransport;
    use rt_format::{COLLECTION_FILE, FileSet};

    const SECRET: &str = "s3cr3tHist42";
    /// A secret that is not a valid header value: the transport's error quotes it.
    const BAD_SECRET: &str = "s3cr3tHist42\nbroken";

    fn forms() -> Vec<String> {
        let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
        vec![
            SECRET.to_owned(),
            "s3cr3t".to_owned(),
            b64(&format!("ada:{SECRET}")),
            b64(SECRET),
        ]
    }

    fn assert_clean(what: &str, text: &str) {
        for form in forms() {
            assert!(!text.contains(&form), "{what} contains {form:?}: {text}");
        }
    }

    #[test]
    fn no_secret_value_in_history_entries() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&runtime, async {
            let port = rt_testserver::spawn_local().await.port();
            let files = FileSet {
                files: [
                    (COLLECTION_FILE, "info:\n  name: Secrets\n".to_owned()),
                    (
                        "environments/Local.yml",
                        format!(
                            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n  - secret: true\n    name: bad\n"
                        ),
                    ),
                ]
                .into_iter()
                .map(|(p, t)| (p.to_owned(), t.into_bytes()))
                .collect(),
            };
            let mut ws = Workspace::open(files).unwrap();
            let get = |url: &str| RequestSpec {
                method: "GET".into(),
                url: url.into(),
                ..RequestSpec::default()
            };
            let cases: Vec<(&str, RequestSpec, Option<u16>)> = vec![
                // Query: the echo answers 200 with the value in its JSON.
                ("Query", get("{{base}}/echo/q?key={{token}}&x=1"), Some(200)),
                // Path and Bearer auth: 200 only if the server got the value.
                (
                    "Path and bearer",
                    RequestSpec {
                        auth: Auth::Bearer {
                            token: "{{token}}".into(),
                        },
                        ..get("{{base}}/bearer/{{token}}")
                    },
                    Some(200),
                ),
                (
                    "Header",
                    RequestSpec {
                        headers: vec![KeyValue::new("X-Token", "pre-{{token}}-post")],
                        ..get("{{base}}/echo/h")
                    },
                    Some(200),
                ),
                (
                    "Basic",
                    RequestSpec {
                        auth: Auth::Basic {
                            username: "ada".into(),
                            password: "{{token}}".into(),
                        },
                        ..get("{{base}}/basic-auth/ada/{{token}}")
                    },
                    Some(200),
                ),
                (
                    "API key in query",
                    RequestSpec {
                        auth: Auth::ApiKey {
                            key: "api_key".into(),
                            value: "{{token}}".into(),
                            placement: ApiKeyPlacement::Query,
                        },
                        ..get("{{base}}/echo/k")
                    },
                    Some(200),
                ),
                // Failures whose transport errors quote the URL or the header value.
                ("Bad scheme", get("ftp://example.test/{{token}}"), None),
                (
                    "Bad header",
                    RequestSpec {
                        headers: vec![KeyValue::new("X-Bad", "{{bad}}")],
                        ..get("{{base}}/echo/b")
                    },
                    None,
                ),
            ];
            let transport = NativeTransport::new().unwrap();
            let mut history = History::new();
            for (name, spec, status) in cases {
                let path = ws.create_request("", name).unwrap();
                ws.update_request(&path, RequestSpec { name: name.into(), ..spec })
                    .unwrap();
                let input = SendInput {
                    environment: Some("Local".into()),
                    vars: Vec::new(),
                    secrets: vec![
                        ("token".into(), SECRET.into()),
                        ("bad".into(), BAD_SECRET.into()),
                    ],
                };
                let report = ws
                    .snapshot(&path, input)
                    .unwrap()
                    .send(&transport, &SendOptions::default())
                    .await;
                let status_got = report.response.as_ref().map(|r| r.status);
                assert_eq!(status_got, status, "{name}: {report:?}");
                let entry = history.record("c", name, "GET", 0, &report);
                assert_clean(name, &serde_json::to_string(&entry).unwrap());
                if status.is_none() {
                    assert!(entry.error.is_some(), "{name}");
                }
                assert!(entry.url.is_some(), "{name}");
            }
            let all = serde_json::to_string(history.entries()).unwrap();
            assert_clean("history", &all);
            assert!(all.contains("***"), "{all}");
            assert_clean("list", &serde_json::to_string(&history.list("c", "", 100)).unwrap());
            // Searching the history for the value finds nothing.
            assert!(history.list("c", SECRET, 100).is_empty());
        });
    }
}
