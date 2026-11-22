// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The example collections in `examples/` (wave 6, launch): every one is valid OpenCollection
//! that `rt-format` reads without issues, writes back byte for byte (CLAUDE.md, "Format"), has
//! the requests its README promises (read, create, update, `$batch`), and keeps secrets out of
//! the files (secret variables are declared without a value).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rt_core::model::{Auth, Body, Collection, Folder, Item, RequestEntry, Variable};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn requests(items: &[Item], out: &mut Vec<RequestEntry>) {
    for item in items {
        match item {
            Item::Folder(folder) => requests(&folder.items, out),
            Item::Request(entry) => out.push(entry.clone()),
        }
    }
}

/// Reads one example, checks it reads cleanly and round-trips byte-identically, and returns
/// the model.
fn read(name: &str) -> Collection {
    let root = examples().join(name);
    let files = FileSet::from_dir(&root).unwrap();
    assert!(
        files.files.contains_key(COLLECTION_FILE),
        "{name}: no {COLLECTION_FILE}"
    );
    let doc = read_collection(&files).unwrap_or_else(|e| panic!("{name}: read failed: {e}"));
    assert!(
        doc.issues().is_empty(),
        "{name}: issues while reading: {:?}",
        doc.issues()
    );

    let written = doc.write();
    assert_eq!(
        written.files.keys().collect::<Vec<_>>(),
        files.files.keys().collect::<Vec<_>>(),
        "{name}: written file list differs"
    );
    for (path, original) in &files.files {
        assert!(
            &written.files[path] == original,
            "{name}/{path}: not byte-identical after read and write\n--- original\n{}\n--- written\n{}",
            String::from_utf8_lossy(original),
            String::from_utf8_lossy(&written.files[path])
        );
    }
    doc.model().clone()
}

fn by_path<'a>(all: &'a [RequestEntry], path: &str) -> &'a RequestEntry {
    all.iter()
        .find(|r| r.path == path)
        .unwrap_or_else(|| panic!("no request {path}; have {:?}", paths(all)))
}

fn paths(all: &[RequestEntry]) -> Vec<&str> {
    all.iter().map(|r| r.path.as_str()).collect()
}

/// The request at `path` has this method, its URL contains `url_part`, and it has a body
/// whenever it writes.
fn expect(all: &[RequestEntry], path: &str, method: &str, url_part: &str) {
    let r = by_path(all, path);
    assert_eq!(r.spec.method, method, "{path}: method");
    assert!(
        r.spec.url.contains(url_part),
        "{path}: URL {} does not contain {url_part}",
        r.spec.url
    );
    if matches!(method, "POST" | "PUT" | "PATCH" | "MERGE") {
        assert!(!matches!(r.spec.body, Body::None), "{path}: no body");
    }
}

/// Every `$batch` request names its multipart boundary or is a JSON batch.
fn expect_batch(all: &[RequestEntry], path: &str) {
    expect(all, path, "POST", "/$batch");
    let r = by_path(all, path);
    let content_type = r
        .spec
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.as_str())
        .unwrap_or_default();
    let Body::Raw { data, .. } = &r.spec.body else {
        panic!("{path}: a batch needs a raw body");
    };
    if let Some(boundary) = content_type.strip_prefix("multipart/mixed; boundary=") {
        assert!(
            data.contains(&format!("--{boundary}--")),
            "{path}: body does not close boundary {boundary}"
        );
        assert!(data.contains("changeset"), "{path}: no changeset");
    } else {
        assert_eq!(content_type, "application/json", "{path}: content type");
        assert!(
            data.contains("\"atomicityGroup\""),
            "{path}: no atomicity group"
        );
    }
}

/// The environment `Local` exists, and every secret variable in any environment has no value.
fn expect_local_env(model: &Collection, vars: &[(&str, &str)], secrets: &[&str]) {
    let local = model.environment("Local").expect("environment Local");
    for (name, value) in vars {
        let var = local
            .variables
            .iter()
            .find(|v| v.name == *name)
            .unwrap_or_else(|| panic!("Local: no variable {name}"));
        assert_eq!(var.value.as_deref(), Some(*value), "Local: {name}");
        assert!(!var.secret, "Local: {name} must not be secret");
    }
    for name in secrets {
        let var = local
            .variables
            .iter()
            .find(|v| v.name == *name)
            .unwrap_or_else(|| panic!("Local: no secret {name}"));
        assert!(var.secret, "Local: {name} must be secret");
    }
    for env in &model.environments {
        for var in &env.variables {
            if var.secret {
                assert_eq!(
                    var.value, None,
                    "{}: secret {} has a value",
                    env.name, var.name
                );
            }
        }
    }
}

/// The service roots are those of rt-odata-testserver (`plugins/odata/testdata/SOURCES.md`).
fn expect_var(vars: &[Variable], name: &str, value: &str) {
    let var = vars
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("no variable {name}"));
    assert_eq!(var.value.as_deref(), Some(value), "{name}");
}

fn folder<'a>(model: &'a Collection, path: &str) -> &'a Folder {
    model
        .items
        .iter()
        .find_map(|item| match item {
            Item::Folder(f) if f.path == path => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no folder {path}"))
}

fn expect_basic_auth_from_variables(model: &Collection) {
    assert_eq!(
        model.defaults.auth,
        Auth::Basic {
            username: "{{user}}".into(),
            password: "{{password}}".into(),
        }
    );
}

/// Requests that inherit their auth from the collection, i.e. all of them.
fn expect_inherited_auth(all: &[RequestEntry]) {
    for r in all {
        assert_eq!(r.spec.auth, Auth::Inherit, "{}: auth must inherit", r.path);
    }
}

#[test]
fn s4hana_sales() {
    let model = read("s4hana-sales");
    let mut all = Vec::new();
    requests(&model.items, &mut all);
    expect_var(
        &model.defaults.vars,
        "service",
        "/sap/opu/odata/sap/ZRT_SALES_SRV",
    );
    expect(
        &all,
        "list-sales-orders.yml",
        "GET",
        "{{service}}/SalesOrderSet?",
    );
    expect(
        &all,
        "read-sales-order.yml",
        "GET",
        "/SalesOrderSet('{{salesOrderId}}')",
    );
    expect(&all, "create-sales-order.yml", "POST", "/SalesOrderSet");
    expect(
        &all,
        "update-sales-order.yml",
        "MERGE",
        "/SalesOrderSet('{{salesOrderId}}')",
    );
    expect(
        &all,
        "open-order-count.yml",
        "GET",
        "/GetOpenOrderCount?CustomerID=",
    );
    expect_batch(&all, "batch.yml");
    assert_eq!(all.len(), 6, "{:?}", paths(&all));
    expect_basic_auth_from_variables(&model);
    expect_inherited_auth(&all);
    expect_local_env(
        &model,
        &[
            ("baseUrl", "http://127.0.0.1:47830"),
            ("sapClient", "100"),
            ("sapLanguage", "EN"),
            ("user", "RT_USER"),
        ],
        &["password"],
    );
}

#[test]
fn cap_bookshop() {
    let model = read("cap-bookshop");
    let mut all = Vec::new();
    requests(&model.items, &mut all);
    // OData V4 (draft-enabled Books).
    expect_var(
        &folder(&model, "v4").defaults.vars,
        "service",
        "/odata/v4/admin",
    );
    expect(&all, "v4/list-books.yml", "GET", "{{service}}/Books?");
    expect(
        &all,
        "v4/read-book.yml",
        "GET",
        "/Books(ID={{bookId}},IsActiveEntity=true)",
    );
    expect(&all, "v4/create-book-draft.yml", "POST", "/Books");
    expect(
        &all,
        "v4/edit-book.yml",
        "POST",
        "IsActiveEntity=true)/AdminService.draftEdit",
    );
    expect(
        &all,
        "v4/update-book-draft.yml",
        "PATCH",
        "/Books(ID={{bookId}},IsActiveEntity=false)",
    );
    expect(
        &all,
        "v4/activate-book.yml",
        "POST",
        "IsActiveEntity=false)/AdminService.draftActivate",
    );
    expect(
        &all,
        "v4/discard-book-draft.yml",
        "DELETE",
        "/Books(ID={{bookId}},IsActiveEntity=false)",
    );
    expect_batch(&all, "v4/batch.yml");
    // OData V2 (the same service through the V2 adapter, no drafts).
    expect_var(
        &folder(&model, "v2").defaults.vars,
        "service",
        "/odata/v2/admin",
    );
    expect(&all, "v2/list-books.yml", "GET", "{{service}}/Books?");
    expect(&all, "v2/read-book.yml", "GET", "/Books({{bookId}})");
    expect(&all, "v2/create-book.yml", "POST", "{{service}}/Books");
    expect(&all, "v2/update-book.yml", "MERGE", "/Books({{bookId}})");
    expect_batch(&all, "v2/batch.yml");
    assert_eq!(all.len(), 13, "{:?}", paths(&all));
    // CAP's bookshop runs without authentication locally.
    assert_eq!(model.defaults.auth, Auth::None);
    expect_local_env(&model, &[("baseUrl", "http://127.0.0.1:47830")], &[]);
}

#[test]
fn rap_salesorder() {
    let model = read("rap-salesorder");
    let mut all = Vec::new();
    requests(&model.items, &mut all);
    let key_active = "SalesOrder(SalesOrderUUID={{salesOrderUUID}},IsActiveEntity=true)";
    let key_draft = "SalesOrder(SalesOrderUUID={{salesOrderUUID}},IsActiveEntity=false)";
    expect_var(
        &model.defaults.vars,
        "service",
        "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001",
    );
    expect(
        &all,
        "list-sales-orders.yml",
        "GET",
        "{{service}}/SalesOrder?",
    );
    expect(&all, "read-sales-order.yml", "GET", key_active);
    expect(&all, "create-sales-order-draft.yml", "POST", "/SalesOrder");
    expect(
        &all,
        "edit-sales-order.yml",
        "POST",
        &format!("{key_active}/com.example.zrt_salesorder.Edit"),
    );
    expect(&all, "update-sales-order-draft.yml", "PATCH", key_draft);
    expect(
        &all,
        "prepare-sales-order.yml",
        "POST",
        &format!("{key_draft}/com.example.zrt_salesorder.Prepare"),
    );
    expect(
        &all,
        "activate-sales-order.yml",
        "POST",
        &format!("{key_draft}/com.example.zrt_salesorder.Activate"),
    );
    expect(
        &all,
        "discard-sales-order-draft.yml",
        "POST",
        &format!("{key_draft}/com.example.zrt_salesorder.Discard"),
    );
    expect_batch(&all, "batch.yml");
    assert_eq!(all.len(), 9, "{:?}", paths(&all));
    expect_basic_auth_from_variables(&model);
    expect_inherited_auth(&all);
    expect_local_env(
        &model,
        &[
            ("baseUrl", "http://127.0.0.1:47830"),
            ("sapClient", "100"),
            ("sapLanguage", "EN"),
            ("user", "RT_USER"),
        ],
        &["password"],
    );
}

/// Every folder in `examples/` is one of the collections tested above, so a new example cannot
/// skip the checks.
#[test]
fn every_example_is_tested() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(examples())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    let tested: BTreeSet<String> = ["cap-bookshop", "rap-salesorder", "s4hana-sales"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(on_disk, tested);
}
