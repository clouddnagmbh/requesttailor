// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Every case under `tests/cases` through the sandbox, natively. The browser runs the same
//! files through the WASM core (`crates/rt-wasm/tests/scripts.rs`).
//!
//! Run: `cargo test -p rt-script --features test-cases --test cases -- --nocapture`

#![cfg(feature = "test-cases")]

use rt_script::cases;

#[test]
fn case_files_parse() {
    let (cases, problems) = cases::all();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(cases.len() >= 20, "{} cases", cases.len());
    let mut names: Vec<&str> = cases.iter().map(|(_, c)| c.name.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "case names must be unique");
}

#[test]
fn core_cases() {
    rt_script::block_on(cases::run_all("core/", |line| println!("{line}")));
}

#[test]
fn shim_and_module_cases() {
    rt_script::block_on(cases::run_all("shims/", |line| println!("{line}")));
    rt_script::block_on(cases::run_all("modules/", |line| println!("{line}")));
}
