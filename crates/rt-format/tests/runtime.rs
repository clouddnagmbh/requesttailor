// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 7 interface: scripts, assertions and actions are read from the Bruno corpus, at
//! collection, folder and request level, and the files still write back byte-identically.

use std::path::PathBuf;

use rt_core::model::{Collection, Item, RequestEntry, ScriptPhase, VarScope};
use rt_format::{FileSet, read_collection};

fn read(name: &str) -> Collection {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/collections/bruno")
        .join(name);
    let files = FileSet::from_dir(&root).unwrap();
    let doc = read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    assert_eq!(doc.write().files, files.files, "{name}: not byte-identical");
    doc.model().clone()
}

fn requests(items: &[Item], out: &mut Vec<RequestEntry>) {
    for item in items {
        match item {
            Item::Folder(f) => requests(&f.items, out),
            Item::Request(r) => out.push(r.clone()),
        }
    }
}

#[test]
fn scripts_at_every_level() {
    let c = read("response-timeline-headers-yml");
    assert!(
        c.defaults
            .scripts
            .iter()
            .any(|s| s.phase == ScriptPhase::BeforeRequest && s.code.contains("bru.setVar"))
    );
    let Some(Item::Folder(folder)) = c.items.iter().find(|i| matches!(i, Item::Folder(_))) else {
        panic!("no folder");
    };
    assert!(
        folder.defaults.scripts[0]
            .code
            .contains("folder-1-set-header-1")
    );
}

#[test]
fn assertions_with_descriptions() {
    let c = read("request-description-yml");
    let mut all = Vec::new();
    requests(&c.items, &mut all);
    let a: Vec<_> = all.iter().flat_map(|r| &r.spec.assertions).collect();
    assert_eq!(a[0].expression, "res.status");
    assert_eq!(a[0].operator, "eq");
    assert_eq!(a[0].value, "200");
    assert_eq!(a[0].description.as_deref(), Some("Single-line assert desc"));
    assert!(a.iter().all(|a| a.enabled));
}

#[test]
fn test_scripts_and_set_variable_actions() {
    let c = read("collection-collection-run-skipped-run");
    let mut all = Vec::new();
    requests(&c.items, &mut all);
    assert!(
        all.iter()
            .flat_map(|r| &r.spec.scripts)
            .any(|s| s.phase == ScriptPhase::Tests && s.code.contains("test("))
    );
    let mut all = Vec::new();
    requests(
        &read("scripting-quickjs-safe-mode-quickjs-safe-mode").items,
        &mut all,
    );
    let action = all.iter().flat_map(|r| &r.spec.actions).next().unwrap();
    assert_eq!(action.variable, "drainBorn");
    assert_eq!(action.method, "jsonq");
    assert_eq!(action.scope, VarScope::Runtime);
}
