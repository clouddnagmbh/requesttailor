// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 7 gate (1): a Postman collection with pre-request and test scripts runs unchanged in
//! CI after import. `rt import` turns the export into a collection folder, `rt run` runs it
//! against `rt-testserver` and writes a JUnit report: exit 0, every test passed, the report
//! parses and lists the tests.
//!
//! Needs the importer shims and the fixture (strand B), the sandbox (strand A) and the
//! runner (strand D). Run: `cargo test -p rt-cli --test gate_w7 -- --include-ignored`.

#[path = "support/xml.rs"]
mod xml;

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/import/postman/scripts-gate.postman_collection.json")
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

#[test]
#[ignore = "wave 7: needs strands A, B, D"]
fn imported_postman_collection_runs_green_in_ci() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let addr = server.block_on(rt_testserver::spawn_local());
    let tmp = std::env::temp_dir().join(format!("rt-gate-w7-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let data = tmp.join("data");
    let collection = tmp.join("collection");
    let junit = tmp.join("junit.xml");

    let (code, out, err) = rt(
        &[
            "import",
            fixture().to_str().unwrap(),
            "--out",
            collection.to_str().unwrap(),
        ],
        &data,
    );
    assert_eq!(code, 0, "import failed\n{out}\n{err}");

    // `baseUrl` is the fixture's variable for the server; a runtime variable wins over the
    // collection's and an environment's value.
    let base = format!("baseUrl=http://{addr}");
    let report = format!("junit={}", junit.display());
    let (code, out, err) = rt(
        &[
            "run",
            collection.to_str().unwrap(),
            "--var",
            &base,
            "--no-prompt",
            "--reporter",
            &report,
        ],
        &data,
    );
    assert_eq!(code, 0, "rt run failed\n{out}\n{err}");

    let doc = std::fs::read_to_string(&junit).unwrap();
    let root = xml::parse(&doc).unwrap_or_else(|e| panic!("{e}\n{doc}"));
    assert_eq!(root.name, "testsuites");
    assert_eq!(root.attr("failures"), "0", "{doc}");
    assert_eq!(root.attr("errors"), "0", "{doc}");
    let cases = root.all("testcase");
    let tests: Vec<_> = cases
        .iter()
        .filter(|c| c.attr("name") != "request")
        .collect();
    assert!(
        !tests.is_empty(),
        "the report lists no tests of the scripts\n{doc}"
    );
    for c in &cases {
        assert!(
            c.child("failure").is_none() && c.child("error").is_none(),
            "{} failed\n{doc}",
            c.attr("name")
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
