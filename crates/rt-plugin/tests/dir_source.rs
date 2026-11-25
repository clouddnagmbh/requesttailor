// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Where an installed plugin came from (wave 18, strand I): the catalogue source and the
//! SHA-256 of the module are kept in `plugins.json`, additively.

use std::path::PathBuf;

use rt_plugin::host::dir::{Origin, PluginDir};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-plugin-src-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn origin(sha: &str) -> Origin {
    Origin {
        source: "https://example.test/plugins.json".into(),
        sha256: sha.into(),
    }
}

#[test]
fn the_origin_is_recorded_read_back_and_dropped_by_a_file_install() {
    let data = temp("origin");
    let dir = PluginDir::in_data_dir(&data);
    dir.install_from(&fixture("demo.wasm"), Some(origin("aa")))
        .unwrap();
    let records = dir.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "demo");
    assert_eq!(records[0].version, "1.0.0");
    assert_eq!(records[0].origin, Some(origin("aa")));

    // A new instance reads the same state.
    let again = PluginDir::in_data_dir(&data).records();
    assert_eq!(again[0].origin, Some(origin("aa")));

    // An update from a file says nothing about a source any more.
    dir.install(&fixture("demo-v2.wasm")).unwrap();
    let records = dir.records();
    assert_eq!(records[0].version, "2.0.0");
    assert_eq!(records[0].origin, None);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn an_older_state_file_without_the_fields_still_reads() {
    let data = temp("old-state");
    let dir = PluginDir::in_data_dir(&data);
    dir.install(&fixture("demo.wasm")).unwrap();
    let state = std::fs::read_to_string(data.join("plugins/plugins.json")).unwrap();
    assert!(!state.contains("sha256"), "no origin, no field: {state}");
    assert_eq!(dir.records()[0].origin, None);
    assert_eq!(dir.load().0.len(), 1);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn the_installed_module_is_the_downloaded_one_byte_for_byte() {
    let data = temp("bytes");
    let dir = PluginDir::in_data_dir(&data);
    let wasm = fixture("demo.wasm");
    dir.install_from(&wasm, Some(origin("bb"))).unwrap();
    assert_eq!(dir.module_bytes("demo"), Some(wasm));
    assert_eq!(dir.module_bytes("missing"), None);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn small_files_next_to_the_plugins_stay_in_the_folder() {
    let data = temp("aux");
    let dir = PluginDir::in_data_dir(&data);
    assert_eq!(dir.read_aux("sources.json"), None);
    dir.write_aux("sources.json", b"[]").unwrap();
    assert_eq!(dir.read_aux("sources.json"), Some(b"[]".to_vec()));
    assert!(dir.write_aux("../escape.json", b"x").is_err());
    assert!(dir.write_aux("a/b.json", b"x").is_err());
    assert_eq!(dir.read_aux("../plugins/sources.json"), None);
    let _ = std::fs::remove_dir_all(&data);
}
