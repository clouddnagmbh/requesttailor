// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Embeds the conformance case files and the Bruno corpus for the browser tests, which have
//! no file system. Writes `$OUT_DIR/testdata_files.rs`:
//! `static TESTDATA: &[(&str, &[u8])]`, keys relative to `testdata/`, `/`-separated.
//! Only the wasm32 test binary includes it; for other targets the list is empty.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const DIRS: &[&str] = &["conformance", "collections/bruno"];

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("testdata_files.rs");
    // Read at run time, not with `env!`: a build-script binary reused from another checkout
    // (shared target dir) would otherwise point at that checkout.
    let testdata = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"))
        .join("../../testdata");
    let mut entries = Vec::new();
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        for dir in DIRS {
            let dir = testdata.join(dir);
            println!("cargo:rerun-if-changed={}", dir.display());
            walk(&dir, &mut entries);
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    entries.sort();
    let mut code = String::from("pub static TESTDATA: &[(&str, &[u8])] = &[\n");
    for path in entries {
        let key = path
            .strip_prefix(&testdata)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_str().expect("UTF-8 path"))
            .collect::<Vec<_>>()
            .join("/");
        let abs = path.to_str().expect("UTF-8 path");
        writeln!(code, "    ({key:?}, include_bytes!({abs:?})),").unwrap();
    }
    code.push_str("];\n");
    std::fs::write(out, code).unwrap();
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}
