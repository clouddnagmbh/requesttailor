// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt import` through the built binary: written collection, text and JSON report, exit codes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/import")
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt(args: &[&std::ffi::OsStr]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .output()
        .expect("start rt");
    Output {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// A fresh, empty scratch folder per test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-cli-import-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_tree(root: &Path) -> rt_format::FileSet {
    fn walk(root: &Path, dir: &Path, files: &mut rt_format::FileSet) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, files);
            } else {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.files.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut files = rt_format::FileSet::default();
    walk(root, root, &mut files);
    files
}

#[test]
fn imports_a_postman_collection_with_environment_and_globals() {
    let bundle = corpus().join("postman/synthetic-v21-features.bundle");
    let out = scratch("postman").join("Features");
    let o = rt(&[
        "import".as_ref(),
        bundle.join("features.postman_collection.json").as_os_str(),
        bundle.join("staging.postman_environment.json").as_os_str(),
        bundle.join("workspace.postman_globals.json").as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    assert!(
        o.stdout
            .starts_with("Imported Postman \"Synthetic Features\""),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("Loss report:"), "{}", o.stdout);
    assert!(o.stdout.contains("secret value of"), "{}", o.stdout);
    // The written folder is the collection the snapshot describes, and it reads back.
    let files = read_tree(&out);
    let doc = rt_format::read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    let expected =
        read_tree(&corpus().join("postman/synthetic-v21-features.bundle.expected/collection"));
    assert_eq!(files.files, expected.files);
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn json_report_and_explicit_format() {
    let out = scratch("json").join("c");
    let o = rt(&[
        "import".as_ref(),
        corpus().join("curl/synthetic-lossless.sh").as_os_str(),
        "-f".as_ref(),
        "curl".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["collectionName"], "synthetic-lossless");
    assert_eq!(v["report"]["format"], "curl");
    assert_eq!(v["report"]["requests"], 1);
    assert_eq!(v["report"]["losses"].as_array().unwrap().len(), 0);
    assert!(out.join("opencollection.yml").is_file());
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn lossless_imports_say_so() {
    let out = scratch("lossless").join("c");
    let o = rt(&[
        "import".as_ref(),
        corpus().join("har/synthetic-lossless.har").as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    assert!(o.stdout.contains("Nothing was lost"), "{}", o.stdout);
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn errors_exit_with_usage_and_write_nothing() {
    let dir = scratch("errors");
    // Invalid file.
    let out = dir.join("a");
    let o = rt(&[
        "import".as_ref(),
        corpus()
            .join("invalid/synthetic-truncated-postman.json")
            .as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
    ]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("is not valid JSON"), "{}", o.stderr);
    assert!(!out.exists(), "nothing written for a failed import");
    // Out folder in use.
    std::fs::create_dir_all(dir.join("used")).unwrap();
    std::fs::write(dir.join("used/keep.txt"), "x").unwrap();
    let o = rt(&[
        "import".as_ref(),
        corpus().join("curl/synthetic-lossless.sh").as_os_str(),
        "--out".as_ref(),
        dir.join("used").as_os_str(),
    ]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("not empty"), "{}", o.stderr);
    assert_eq!(std::fs::read_dir(dir.join("used")).unwrap().count(), 1);
    // Missing arguments and files.
    assert_eq!(rt(&["import".as_ref(), "x.json".as_ref()]).code, 2);
    let o = rt(&[
        "import".as_ref(),
        dir.join("missing.json").as_os_str(),
        "--out".as_ref(),
        dir.join("b").as_os_str(),
    ]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("cannot read"), "{}", o.stderr);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn help_mentions_import() {
    let o = rt(&["--help".as_ref()]);
    assert_eq!(o.code, 0);
    assert!(
        o.stdout.contains("rt import <file>... --out <dir>"),
        "{}",
        o.stdout
    );
    let o = rt(&["import".as_ref(), "--help".as_ref()]);
    assert_eq!(o.code, 0);
}
