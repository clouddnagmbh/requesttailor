// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand C: `rt send --all-pages [--max-pages N]` against `rt-testserver`'s paged
//! lists: the rule from the request's companion or the collection, merging, the default limit of
//! twenty pages with its note, `--max-pages`, the hard maximum, no rule, the send guard per page
//! and a canary secret that must not reach the output.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rt_core::guard::{GuardHandle, GuardVerdict, SendCheck, SendGuard};

const SECRET: &str = "RT-CANARY-paging-cli-61d0";

fn serve() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let addr = rt_testserver::spawn(([127, 0, 0, 1], 0).into())
                .await
                .unwrap();
            tx.send(addr.port()).unwrap();
            std::future::pending::<()>().await;
        });
    });
    rx.recv().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-paging-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A collection whose `list.yml` reads `/pages/<kind>/<pages>`.
fn collection(root: &Path, port: u16, kind: &str, pages: usize) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Paging\n",
    );
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
        ),
    );
    write(
        &root.join("list.yml"),
        &format!(
            "info:\n  name: List\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/pages/{kind}/{pages}?size=2\"\n  headers:\n    - name: Authorization\n      value: Bearer {{{{token}}}}\n"
        ),
    );
    write(
        &root.join("write.yml"),
        &format!(
            "info:\n  name: Write\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{{{base}}}}/pages/{kind}/{pages}?size=2\"\n"
        ),
    );
}

const LINK_RULE: &str = "{\n  \"version\": 1,\n  \"rule\": {\n    \"kind\": \"link-header\"\n  },\n  \"merge\": \"$.items\"\n}\n";

fn rt(args: &[String]) -> (i32, String, String) {
    let secret = format!("token={SECRET}");
    let mut full: Vec<String> = args.to_vec();
    full.extend(["--env", "Local", "--var", &secret, "--no-plugins"].map(String::from));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run(full, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn send(file: &Path, extra: &[&str]) -> (i32, String, String) {
    let mut args = vec!["send".to_owned(), file.to_str().unwrap().to_owned()];
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    rt(&args)
}

fn items(out: &str) -> Vec<u64> {
    let report: serde_json::Value = serde_json::from_str(out).unwrap();
    let body = report["response"]["body"]["data"].as_str().unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_u64().unwrap())
        .collect()
}

fn notes(out: &str) -> String {
    let report: serde_json::Value = serde_json::from_str(out).unwrap();
    report["notes"].to_string()
}

#[test]
fn all_pages_follows_the_companion_rule_and_merges() {
    let port = serve();
    let dir = tmp("all");
    collection(&dir, port, "link", 7);
    write(&dir.join("list.paging.json"), LINK_RULE);
    let (code, out, err) = send(&dir.join("list.yml"), &["--all-pages"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(items(&out), (1..=14).collect::<Vec<_>>());
    assert!(notes(&out).contains("7 pages"), "{}", notes(&out));
    for text in [&out, &err] {
        assert!(!text.contains(SECRET), "the canary reached the output");
    }
    // Without the flag the request is sent once, as before.
    let (code, out, _) = send(&dir.join("list.yml"), &[]);
    assert_eq!(code, 0);
    assert_eq!(items(&out), vec![1, 2]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_default_limit_is_twenty_pages_and_max_pages_changes_it() {
    let port = serve();
    let dir = tmp("limit");
    collection(&dir, port, "link", 60);
    write(&dir.join("list.paging.json"), LINK_RULE);
    let (code, out, _) = send(&dir.join("list.yml"), &["--all-pages"]);
    assert_eq!(code, 0);
    assert_eq!(items(&out).len(), 40);
    assert!(
        notes(&out).contains("stopped after 20 pages: limit reached; more pages exist"),
        "{}",
        notes(&out)
    );
    let (_, out, _) = send(&dir.join("list.yml"), &["--all-pages", "--max-pages", "5"]);
    assert_eq!(items(&out).len(), 10);
    assert!(notes(&out).contains("stopped after 5 pages"));
    let (_, out, _) = send(
        &dir.join("list.yml"),
        &["--all-pages", "--max-pages", "5000"],
    );
    assert_eq!(items(&out).len(), 120, "all 60 pages, the cap is 1000");
    assert!(notes(&out).contains("cut to 1000"), "{}", notes(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_collection_default_applies_without_a_companion() {
    let port = serve();
    let dir = tmp("default");
    collection(&dir, port, "cursor", 4);
    write(
        &dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Paging\nextensions:\n  requesttailor:\n    paging:\n      rule: cursor\n      next: $.meta.cursor\n      param:\n        place: query\n        name: cursor\n      merge: $.items\n",
    );
    let (code, out, err) = send(&dir.join("list.yml"), &["--all-pages"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(items(&out), (1..=8).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_rule_and_writes_are_refused_with_a_reason() {
    let port = serve();
    let dir = tmp("refuse");
    collection(&dir, port, "link", 3);
    let (code, _, err) = send(&dir.join("list.yml"), &["--all-pages"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("no paging rule"), "{err}");
    assert!(err.contains("list.paging.json"), "{err}");
    write(&dir.join("write.paging.json"), LINK_RULE);
    let (code, _, err) = send(&dir.join("write.yml"), &["--all-pages"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("writes"), "{err}");
    let (code, _, err) = send(&dir.join("list.yml"), &["--max-pages", "3"]);
    assert_eq!(code, 2, "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

struct DenyFrom(usize, AtomicUsize);

impl SendGuard for DenyFrom {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        if self.1.fetch_add(1, Ordering::SeqCst) + 1 >= self.0 {
            GuardVerdict::Refuse {
                rule: "cli.no-pages".into(),
                reason: "enough".into(),
            }
        } else {
            GuardVerdict::Allow
        }
    }
}

#[test]
fn the_send_guard_answers_for_every_page() {
    let port = serve();
    let dir = tmp("guard");
    collection(&dir, port, "link", 10);
    write(&dir.join("list.paging.json"), LINK_RULE);
    let guard = Arc::new(DenyFrom(3, AtomicUsize::new(0)));
    let secret = format!("token={SECRET}");
    let file = dir.join("list.yml");
    let args = [
        "send",
        file.to_str().unwrap(),
        "--all-pages",
        "--env",
        "Local",
        "--var",
        &secret,
        "--no-plugins",
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run_distribution(
        args.iter().map(|s| (*s).to_owned()),
        &mut out,
        &mut err,
        &rt_cli::Distribution {
            guard: GuardHandle::new(guard.clone()),
            ..Default::default()
        },
    );
    let out = String::from_utf8_lossy(&out).into_owned();
    assert_eq!(code, 0, "{out}");
    assert_eq!(guard.1.load(Ordering::SeqCst), 3);
    assert_eq!(items(&out), vec![1, 2, 3, 4], "what was loaded stays");
    assert!(notes(&out).contains("cli.no-pages"), "{}", notes(&out));
    let _ = std::fs::remove_dir_all(&dir);
}
