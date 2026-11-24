// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests on the command line (wave 17, strand B): `rt snapshot update`, and `rt run`
//! and `rt send` comparing (exit code 4 on a difference, never writing).

#[path = "support/snapshot_server.rs"]
mod server;

use std::path::{Path, PathBuf};
use std::process::Command;

const SECRET: &str = "RT-CANARY-cli-snapshot-1";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-snapshot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

const GET: &str = "info:\n  name: Get item\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/item\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n";

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n",
    );
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
        ),
    );
    write(
        &root.join("items/folder.yml"),
        "info:\n  name: items\n  type: folder\n  seq: 1\n",
    );
    write(&root.join("items/get.yml"), GET);
    write(
        &root.join("items/plain.yml"),
        "info:\n  name: Plain\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/item\"\n",
    );
}

fn rt(args: &[&str], data: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .env("RT_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn var() -> String {
    format!("token={SECRET}")
}

fn tree(root: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push((
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    std::fs::read_to_string(&p).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn update_writes_the_file_adds_the_assertion_and_keeps_secrets_out() {
    let server = server::spawn();
    let dir = tmp("update");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let get = root.join("items/get.yml");

    let (code, out, err) = rt(
        &[
            "snapshot",
            "update",
            get.to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &var(),
        ],
        &data,
    );
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("items/get.snap.json"), "{out}");
    let snap = read(&root.join("items/get.snap.json"));
    assert!(!snap.contains(SECRET), "{snap}");
    assert!(snap.contains("Bearer ***"), "{snap}");
    assert!(
        !snap.contains("sid=zzz") && !snap.contains("date"),
        "{snap}"
    );
    assert!(snap.contains("\"name\": \"tea\""), "{snap}");
    // The request file got exactly the assertion.
    assert_eq!(
        read(&get),
        format!(
            "{GET}\nruntime:\n  assertions:\n    - expression: res\n      operator: matchesSnapshot\n      value: get.snap.json\n"
        )
    );
    // The other request is untouched.
    assert!(!root.join("items/plain.snap.json").exists());

    // Updating again keeps the ignored paths of the file and does not touch the request.
    let edited = snap.replace("\"ignore\": []", "\"ignore\": [\n    \"$.stamp\"\n  ]");
    assert_ne!(edited, snap);
    write(&root.join("items/get.snap.json"), &edited);
    let request_before = read(&get);
    *server.name.lock().unwrap() = "coffee".into();
    let (code, _, err) = rt(
        &[
            "snapshot",
            "update",
            "--all",
            root.to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &var(),
        ],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let again = read(&root.join("items/get.snap.json"));
    assert!(
        again.contains("coffee") && again.contains("$.stamp"),
        "{again}"
    );
    assert_eq!(read(&get), request_before);
    assert!(!root.join("items/plain.snap.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_needs_a_target() {
    let dir = tmp("usage");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, 1);
    let (code, _, err) = rt(&["snapshot", "update"], &data);
    assert_eq!(code, 2, "{err}");
    // --all with no snapshot anywhere.
    let (code, _, err) = rt(
        &["snapshot", "update", "--all", root.to_str().unwrap()],
        &data,
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("no request with a snapshot assertion"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_and_send_compare_exit_4_on_a_difference_and_never_write() {
    let server = server::spawn();
    let dir = tmp("run");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let get = root.join("items/get.yml");
    let run = |extra: &[&str]| {
        let mut args = vec![
            "run",
            root.to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            "token=RT-CANARY-cli-snapshot-1",
        ];
        args.extend_from_slice(extra);
        rt(&args, &data)
    };

    // No file yet: the assertion fails with the hint, and nothing is written.
    write(
        &get,
        &format!(
            "{GET}\nruntime:\n  assertions:\n    - expression: res\n      operator: matchesSnapshot\n      value: get.snap.json\n"
        ),
    );
    let before = tree(&root);
    let (code, _, err) = run(&[]);
    assert_eq!(code, 4, "{err}");
    assert!(
        err.contains("no snapshot yet; run `rt snapshot update`"),
        "{err}"
    );
    assert_eq!(tree(&root), before);

    // Freeze, then compare. The counter in the body changes with every request.
    let (code, _, err) = rt(
        &[
            "snapshot",
            "update",
            get.to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &var(),
        ],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let frozen = tree(&root);
    let (code, _, err) = run(&[]);
    assert_eq!(code, 4, "the volatile stamp differs: {err}");
    assert!(err.contains("body.stamp"), "{err}");
    assert!(!err.contains(SECRET), "{err}");

    // Ignore it by hand in the file: green, nothing written.
    let snap = root.join("items/get.snap.json");
    write(
        &snap,
        &read(&snap).replace("\"ignore\": []", "\"ignore\": [\n    \"$.stamp\"\n  ]"),
    );
    let before = tree(&root);
    let (code, _, err) = run(&[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(tree(&root), before);
    assert_ne!(frozen, before);

    // A real difference fails again, in `rt send` as well (JSON report, masked).
    *server.name.lock().unwrap() = "coffee".into();
    let (code, _, err) = run(&[]);
    assert_eq!(code, 4, "{err}");
    assert!(err.contains("body.name"), "{err}");
    let (code, out, _) = rt(
        &[
            "send",
            get.to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &var(),
        ],
        &data,
    );
    assert_eq!(code, 4, "{out}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["assertions"][0]["operator"], "matchesSnapshot");
    assert_eq!(report["assertions"][0]["passed"], false);
    assert!(!report["assertions"].to_string().contains(SECRET), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
