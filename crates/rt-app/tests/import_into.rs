// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importing into the open collection as a new folder (since wave 13): structure, carried
//! defaults, environments, companion files, nothing on disk before save, and unchanged files
//! byte-identical.

use std::path::Path;

use rt_app::Workspace;
use rt_app::import::{ExternalImport, ImportedFile, Imports, add_to_files, add_to_workspace};
use rt_core::model::{Auth, Item};
use rt_format::import::{ImportFormat, LossEffect, SourceFile};
use rt_format::{FileSet, read_collection};

const COLLECTION: &str = r#"{"info": {"name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
  "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]},
  "variable": [{"key": "page", "value": "1"}],
  "item": [
    {"name": "Orders", "item": [
      {"name": "List orders", "request": {"method": "GET", "url": "{{base}}/orders?page={{page}}"}},
      {"name": "Open order", "request": {"method": "GET", "url": "{{base}}/orders/1", "auth": {"type": "noauth"}}}
    ]},
    {"name": "Ping", "request": {"method": "GET", "url": "{{base}}/ping"}}
  ]}"#;
const PROD: &str = r#"{"name": "Prod", "_postman_variable_scope": "environment",
  "values": [{"key": "base", "value": "https://other.example.com"}, {"key": "token", "value": "not-real", "type": "secret"}]}"#;
const STAGING: &str = r#"{"name": "Staging", "_postman_variable_scope": "environment",
  "values": [{"key": "base", "value": "https://staging.example.com"}]}"#;

fn base() -> FileSet {
    FileSet::from_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/collections/bruno/api-spec-create-from-collection"),
    )
    .unwrap()
}

fn imported() -> FileSet {
    let files = [
        SourceFile {
            name: "shop.postman_collection.json",
            data: COLLECTION.as_bytes(),
        },
        SourceFile {
            name: "prod.postman_environment.json",
            data: PROD.as_bytes(),
        },
        SourceFile {
            name: "staging.postman_environment.json",
            data: STAGING.as_bytes(),
        },
    ];
    let mut import = rt_format::import::import_files(Some(ImportFormat::Postman), &files).unwrap();
    // A companion file of the import (a recording, a snapshot) and a nested one.
    import
        .files
        .files
        .insert("recording.odtrace".into(), b"{\"format\":\"x\"}".to_vec());
    import
        .files
        .files
        .insert("snapshots/metadata.xml".into(), b"<edmx/>".to_vec());
    import.files
}

/// The base with a Prod environment that already has `base`.
fn base_with_prod() -> Workspace {
    let mut ws = Workspace::open(base()).unwrap();
    ws.update_environment(rt_core::model::Environment {
        name: "Prod".into(),
        variables: vec![rt_core::model::Variable::new(
            "base",
            "https://shop.example.com",
        )],
        client_certificates: Vec::new(),
        external_secrets: None,
    })
    .unwrap();
    ws.save().unwrap();
    ws
}

#[test]
fn an_import_becomes_a_folder_of_the_open_collection() {
    let mut ws = base_with_prod();
    let before = ws.files().clone();
    let result = add_to_workspace(&mut ws, &imported(), "Shop").unwrap();
    assert_eq!(result.name, "Shop");
    assert_eq!((result.requests, result.folders), (3, 1));
    assert_eq!(result.environments_added, vec!["Staging".to_owned()]);
    assert_eq!(result.environments_extended, vec!["Prod".to_owned()]);
    assert_eq!(
        result.files,
        vec![
            format!("{}/recording.odtrace", result.folder),
            format!("{}/snapshots/metadata.xml", result.folder)
        ]
    );
    assert!(
        result
            .losses
            .iter()
            .any(|l| l.effect == LossEffect::Approximated && l.detail.contains("auth"))
    );
    assert!(result.losses.iter().any(|l| l.detail.contains("variable")));

    // Nothing on disk yet: everything is dirty in memory.
    assert_eq!(ws.files(), &before);
    assert!(!ws.dirty().is_empty());

    let model = ws.collection().clone();
    let Some(Item::Folder(top)) = model
        .items
        .iter()
        .find(|i| matches!(i, Item::Folder(f) if f.path == result.folder))
    else {
        panic!("no folder {}", result.folder);
    };
    assert_eq!(top.name, "Shop");
    let names: Vec<&str> = top
        .items
        .iter()
        .map(|i| match i {
            Item::Folder(f) => f.name.as_str(),
            Item::Request(r) => r.spec.name.as_str(),
        })
        .collect();
    assert_eq!(names, vec!["Orders", "Ping"]);
    let Item::Folder(orders) = &top.items[0] else {
        panic!()
    };
    let list = &orders
        .items
        .iter()
        .find_map(|i| match i {
            Item::Request(r) if r.spec.name == "List orders" => Some(r),
            _ => None,
        })
        .unwrap()
        .spec;
    assert!(
        matches!(list.auth, Auth::Bearer { .. }),
        "inherited auth is carried: {:?}",
        list.auth
    );
    assert!(list.vars.iter().any(|v| v.name == "page"));
    let open = orders
        .items
        .iter()
        .find_map(|i| match i {
            Item::Request(r) if r.spec.name == "Open order" => Some(r),
            _ => None,
        })
        .unwrap();
    assert!(
        !matches!(open.spec.auth, Auth::Bearer { .. }),
        "own auth wins"
    );

    let prod = model.environment("Prod").unwrap();
    assert_eq!(
        prod.variables
            .iter()
            .find(|v| v.name == "base")
            .unwrap()
            .value
            .as_deref(),
        Some("https://shop.example.com"),
        "existing values stay"
    );
    assert!(prod.variables.iter().any(|v| v.name == "token" && v.secret));

    // A second import of the same name gets " 2".
    let again = add_to_workspace(&mut ws, &imported(), "Shop").unwrap();
    assert_eq!(again.name, "Shop 2");
    assert_ne!(again.folder, result.folder);

    // Saving writes only new and changed files; no secret value anywhere.
    let changes = ws.save().unwrap();
    let all: String = changes.iter().map(|c| c.text.as_str()).collect();
    assert!(!all.contains("not-real"));
    assert!(changes.iter().all(|c| !c.deleted));
}

#[test]
fn files_level_import_keeps_unchanged_files_byte_identical() {
    let base = base();
    let (files, result) = add_to_files(&base, &imported(), "Shop").unwrap();
    for (path, bytes) in &base.files {
        if path.starts_with("environments/") {
            continue;
        }
        assert_eq!(files.files.get(path), Some(bytes), "{path} changed");
    }
    assert!(
        files
            .files
            .contains_key(&format!("{}/recording.odtrace", result.folder))
    );
    let doc = read_collection(&files).unwrap();
    assert!(doc.model().environment("Staging").is_some());
    // Reading and writing again gives the same files (round trip).
    assert_eq!(
        doc.write()
            .files
            .iter()
            .filter(|(p, _)| p.ends_with(".yml"))
            .count(),
        files
            .files
            .iter()
            .filter(|(p, _)| p.ends_with(".yml"))
            .count()
    );
}

#[test]
fn external_imports_are_checked_before_they_are_kept() {
    let mut imports = Imports::new();
    let report = rt_format::import::LossReport {
        format: ImportFormat::Plugin,
        requests: 0,
        folders: 0,
        environments: 0,
        losses: Vec::new(),
    };
    let file = |path: &str, text: &str| ImportedFile {
        path: path.into(),
        text: text.into(),
    };
    let good = ExternalImport {
        collection_name: "Trace".into(),
        files: vec![
            file(
                "opencollection.yml",
                "opencollection: 1.0.0\ninfo:\n  name: Trace\n",
            ),
            file("trace.odtrace", "{}"),
        ],
        report: report.clone(),
    };
    let plan = imports.adopt(good).unwrap();
    assert_eq!((plan.collection_name.as_str(), plan.files), ("Trace", 2));
    for bad in [
        vec![file("../x.yml", "a: 1\n")],
        vec![file("/abs.yml", "a: 1\n")],
        vec![file(".git/config", "x")],
        vec![file("only.yml", "a: 1\n")],
    ] {
        let e = imports.adopt(ExternalImport {
            collection_name: "x".into(),
            files: bad.clone(),
            report: report.clone(),
        });
        assert!(e.is_err(), "{bad:?}");
    }
}

/// An OpenAPI import that kept its spec (OpenAPI import wizard).
fn openapi_import(keep_spec: bool) -> FileSet {
    let spec = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/import/openapi/oai-petstore-3.0.yaml"),
    )
    .unwrap();
    let options = rt_format::import::OpenApiOptions {
        keep_spec,
        ..Default::default()
    };
    rt_format::import::import_openapi(
        &[SourceFile {
            name: "petstore.yaml",
            data: &spec,
        }],
        &options,
    )
    .unwrap()
    .files
}

#[test]
fn a_kept_spec_becomes_the_contract_spec_of_the_open_collection() {
    let base = base();
    assert_eq!(rt_format::contract::configured_spec(&base), None);
    let mut ws = Workspace::open(base.clone()).unwrap();
    let result = add_to_workspace(&mut ws, &openapi_import(true), "Petstore").unwrap();
    assert_eq!(result.folder, "Petstore");
    assert_eq!(
        result.contract_spec.as_deref(),
        Some("Petstore/openapi.yaml")
    );
    assert!(result.files.contains(&"Petstore/openapi.yaml".to_owned()));
    // Unsaved like any edit: opencollection.yml and the spec are dirty.
    let dirty = ws.dirty();
    assert!(
        dirty.contains(&rt_format::COLLECTION_FILE.to_owned()),
        "{dirty:?}"
    );
    assert!(
        dirty.contains(&"Petstore/openapi.yaml".to_owned()),
        "{dirty:?}"
    );
    let changes = ws.save().unwrap();
    let mut files = base.clone();
    for c in changes {
        files.files.insert(c.path, c.text.into_bytes());
    }
    assert_eq!(
        rt_format::contract::configured_spec(&files).as_deref(),
        Some("Petstore/openapi.yaml")
    );
    let doc = read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    assert_eq!(
        doc.write().files[rt_format::COLLECTION_FILE],
        files.files[rt_format::COLLECTION_FILE]
    );
    // Only the extension was added to opencollection.yml.
    let before = String::from_utf8(base.files[rt_format::COLLECTION_FILE].clone()).unwrap();
    let after = String::from_utf8(files.files[rt_format::COLLECTION_FILE].clone()).unwrap();
    assert!(after.starts_with(before.trim_end()), "{after}");
    assert!(
        after.ends_with("contract:\n      spec: Petstore/openapi.yaml\n"),
        "{after}"
    );

    // A collection that names a spec keeps it; the spec file still comes along.
    let mut ws = Workspace::open(files).unwrap();
    let second = add_to_workspace(&mut ws, &openapi_import(true), "Petstore").unwrap();
    assert_eq!(second.folder, "Petstore 2");
    assert_eq!(second.contract_spec, None);
    assert!(second.files.contains(&"Petstore 2/openapi.yaml".to_owned()));
    assert!(!ws.dirty().contains(&rt_format::COLLECTION_FILE.to_owned()));

    // An import without a kept spec names none.
    let mut ws = Workspace::open(base).unwrap();
    let plain = add_to_workspace(&mut ws, &openapi_import(false), "Petstore").unwrap();
    assert_eq!((plain.contract_spec, plain.files.len()), (None, 0));
    assert!(!ws.dirty().contains(&rt_format::COLLECTION_FILE.to_owned()));
}

#[test]
fn an_import_without_requests_adds_no_folder_and_no_carry_over_note() {
    // Environments only (a list of destinations, say), with auth on the collection.
    const EMPTY: &str = r#"{"info": {"name": "Destinations", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
      "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]},
      "item": []}"#;
    let files = [
        SourceFile {
            name: "dest.postman_collection.json",
            data: EMPTY.as_bytes(),
        },
        SourceFile {
            name: "staging.postman_environment.json",
            data: STAGING.as_bytes(),
        },
    ];
    let import = rt_format::import::import_files(Some(ImportFormat::Postman), &files).unwrap();
    let mut workspace = base_with_prod();
    let before = workspace.collection().items.len();
    let result = add_to_workspace(&mut workspace, &import.files, "Destinations").unwrap();
    assert_eq!(
        workspace.collection().items.len(),
        before,
        "no empty folder"
    );
    assert!(result.folder.is_empty());
    assert_eq!(result.requests, 0);
    assert_eq!(result.environments_added, ["Staging"]);
    assert!(
        result
            .losses
            .iter()
            .all(|l| !l.detail.contains("inherited")),
        "{:?}",
        result.losses
    );
}
