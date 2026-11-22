// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Embeds the JavaScript of the sandbox (`js/runtime`, `js/shims`, `js/modules`, each in
//! file-name order) so every target, wasm32 and iOS included, carries it without a file
//! system. Writes `$OUT_DIR/js_files.rs` with `RUNTIME`, `SHIMS` and `MODULES`
//! (`&[(file name, code)]`).
//!
//! With the `test-cases` feature it also embeds the case files under `tests/cases`
//! (`$OUT_DIR/case_files.rs`, `CASE_FILES: &[(path below tests/cases, JSON)]`), so the same
//! cases run natively and in the browser.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=engine/rt-script-engine.wasm");

    let mut code = String::new();
    for (name, dir) in [
        ("RUNTIME", "js/runtime"),
        ("SHIMS", "js/shims"),
        ("MODULES", "js/modules"),
    ] {
        let dir = root.join(dir);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "js") && path.is_file() {
                    files.push(path);
                }
            }
        }
        files.sort();
        writeln!(code, "pub static {name}: &[(&str, &str)] = &[").unwrap();
        for path in files {
            let file = path.file_name().unwrap().to_str().expect("UTF-8 file name");
            let abs = path.to_str().expect("UTF-8 path");
            writeln!(code, "    ({file:?}, include_str!({abs:?})),").unwrap();
        }
        code.push_str("];\n");
    }
    std::fs::write(out.join("js_files.rs"), code).unwrap();

    let mut cases = String::from("pub static CASE_FILES: &[(&str, &str)] = &[\n");
    if std::env::var_os("CARGO_FEATURE_TEST_CASES").is_some() {
        let dir = root.join("tests/cases");
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        walk(&dir, &mut files);
        files.sort();
        for path in files {
            let key = path
                .strip_prefix(&dir)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str().expect("UTF-8 path"))
                .collect::<Vec<_>>()
                .join("/");
            let abs = path.to_str().expect("UTF-8 path");
            writeln!(cases, "    ({key:?}, include_str!({abs:?})),").unwrap();
        }
    }
    cases.push_str("];\n");
    std::fs::write(out.join("case_files.rs"), cases).unwrap();
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}
