// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Installed plugins on disk: install, update, enable, remove, storage.

mod support;

use std::path::PathBuf;

use rt_plugin::api::Permissions;
use rt_plugin::host::PluginError;
use rt_plugin::host::dir::PluginDir;
use rt_plugin::host::services::PluginStorage;
use serde_json::json;
use support::{Body, block_on, manifest, plugin};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-plugin-{name}-{}", std::process::id()));
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

#[test]
fn install_update_disable_remove() {
    let data = temp("lifecycle");
    let dir = PluginDir::in_data_dir(&data);
    assert_eq!(dir.path(), data.join("plugins"));
    assert!(dir.load().0.is_empty(), "nothing installed yet");

    let other = plugin(&manifest("other", json!({}), json!({})), Body::Trap);
    dir.install(&other).unwrap();
    let demo = dir.install(&fixture("demo.wasm")).unwrap();
    assert_eq!(
        demo.info.granted, demo.info.manifest.permissions,
        "install grants"
    );
    assert!(demo.info.enabled);
    assert!(data.join("plugins/demo.wasm").is_file());

    dir.set_enabled("demo", false).unwrap();
    // An update keeps the order and the enabled state, and grants the new permissions.
    let v2 = dir.install(&fixture("demo-v2.wasm")).unwrap();
    assert_eq!(v2.info.manifest.version, "2.0.0");
    assert!(!v2.info.enabled);
    let (loaded, failed) = dir.load();
    assert!(failed.is_empty(), "{failed:?}");
    let ids: Vec<_> = loaded
        .iter()
        .map(|p| (p.id().to_owned(), p.info.enabled))
        .collect();
    assert_eq!(
        ids,
        [("other".to_owned(), true), ("demo".to_owned(), false)]
    );
    assert_eq!(loaded[1].info.granted.network, vec!["{request}".to_owned()]);

    // Storage is per plugin and goes with it.
    let storage = dir.storage();
    block_on(storage.set("demo", "k", Some(json!({"a": 1}))));
    assert_eq!(block_on(storage.get("demo", "k")), Some(json!({"a": 1})));
    assert_eq!(block_on(storage.get("other", "k")), None);

    dir.remove("demo").unwrap();
    assert!(!data.join("plugins/demo.wasm").exists());
    assert_eq!(block_on(storage.get("demo", "k")), None);
    assert!(matches!(dir.remove("demo"), Err(PluginError::NotFound(_))));
    assert!(matches!(
        dir.set_enabled("demo", true),
        Err(PluginError::NotFound(_))
    ));
    assert_eq!(dir.load().0.len(), 1);

    // Invalid modules are refused before anything is written.
    assert!(dir.install(b"junk").is_err());
    assert_eq!(dir.load().0.len(), 1);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn damaged_or_ungranted_plugins_are_reported_or_get_no_rights() {
    let data = temp("damaged");
    let dir = PluginDir::in_data_dir(&data);
    dir.install(&fixture("demo.wasm")).unwrap();
    // Someone swaps in the version that wants more: it loads without any permission.
    std::fs::write(data.join("plugins/demo.wasm"), fixture("demo-v2.wasm")).unwrap();
    let (loaded, _) = dir.load();
    assert_eq!(loaded[0].info.granted, Permissions::default());
    assert!(!loaded[0].info.fully_granted());
    std::fs::write(data.join("plugins/demo.wasm"), b"junk").unwrap();
    let (loaded, failed) = dir.load();
    assert!(loaded.is_empty());
    assert_eq!(failed[0].0, "demo");
    let _ = std::fs::remove_dir_all(&data);
}
