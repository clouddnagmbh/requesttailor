// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand A: the semantic diff on the model.

use rt_format::diff::{ChangeKind, ChangeTarget, DiffValue, FieldChange, diff};
use rt_format::{CollectionDoc, FileSet, read_collection};

const HEAD: &str = "opencollection: 1.0.0\n\ninfo:\n  name: demo\n";

fn set(files: &[(&str, &str)]) -> FileSet {
    let mut s = FileSet::default();
    s.files
        .insert("opencollection.yml".into(), HEAD.as_bytes().to_vec());
    for (p, t) in files {
        s.files.insert((*p).into(), t.as_bytes().to_vec());
    }
    s
}

fn req(name: &str, url: &str, extra: &str) -> String {
    format!(
        "info:\n  name: {name}\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {url}\n{extra}  auth: inherit\n"
    )
}

fn field<'a>(c: &'a rt_format::diff::ModelChange, path: &[&str]) -> &'a FieldChange {
    c.fields
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no field {path:?} in {:?}", c.fields))
}

fn text(v: &Option<DiffValue>) -> &str {
    match v {
        Some(DiffValue::Text { text }) => text,
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn nothing_changed() {
    let a = set(&[("a.yml", &req("A", "http://x/a", ""))]);
    assert!(diff(&a, &a).unwrap().is_empty());
}

#[test]
fn url_change_has_field_line_and_is_covered() {
    let old = set(&[("a.yml", &req("A", "http://x/a", ""))]);
    let new = set(&[("a.yml", &req("A", "http://x/b", ""))]);
    let d = diff(&old, &new).unwrap();
    assert_eq!(d.changes.len(), 1);
    let c = &d.changes[0];
    assert_eq!(
        c.target,
        ChangeTarget::Request {
            path: "a.yml".into()
        }
    );
    assert_eq!(c.change, ChangeKind::Modified);
    let f = field(c, &["url"]);
    assert_eq!(text(&f.old), "http://x/a");
    assert_eq!(text(&f.new), "http://x/b");
    assert_eq!(f.line, Some(8));
    assert!(d.files[0].covered);
}

#[test]
fn reordered_headers_are_no_change_and_header_edits_are_named() {
    let h1 = "  headers:\n    - name: A\n      value: 1\n    - name: B\n      value: 2\n";
    let h2 = "  headers:\n    - name: B\n      value: 3\n    - name: A\n      value: 1\n    - name: C\n      value: 4\n      disabled: true\n";
    let old = set(&[("a.yml", &req("A", "u", h1))]);
    let new = set(&[("a.yml", &req("A", "u", h2))]);
    let d = diff(&old, &new).unwrap();
    let c = &d.changes[0];
    let paths: Vec<_> = c.fields.iter().map(|f| f.path.join("/")).collect();
    assert_eq!(
        paths,
        ["headers/B", "headers/C", "headers/C/@enabled"],
        "{paths:?}"
    );
    assert_eq!(text(&field(c, &["headers", "B"]).new), "3");
    assert!(field(c, &["headers", "C"]).old.is_none());
    assert_eq!(field(c, &["headers", "C"]).line, Some(14));
    // An unchanged request next to it does not show up.
    let same = diff(&old, &old).unwrap();
    assert!(same.is_empty());
}

#[test]
fn added_removed_and_moved_requests() {
    let a = req("Alpha", "http://x/a", "");
    let b = req("Bravo", "http://x/b", "");
    let old = set(&[("a.yml", &a), ("b.yml", &b)]);
    // a.yml moved into a folder (renamed), b.yml gone, c.yml new.
    let new = set(&[
        ("dir/alpha.yml", &a),
        ("c.yml", &req("Charlie", "http://x/c", "")),
    ]);
    let d = diff(&old, &new).unwrap();
    let kinds: Vec<_> = d
        .changes
        .iter()
        .map(|c| (c.file.as_str(), c.change.clone()))
        .collect();
    assert!(kinds.contains(&(
        "dir/alpha.yml",
        ChangeKind::Moved {
            from: "a.yml".into()
        }
    )));
    assert!(kinds.contains(&("c.yml", ChangeKind::Added)));
    assert!(kinds.contains(&("b.yml", ChangeKind::Removed)));
    assert_eq!(d.changes.len(), 4, "the new folder counts: {:?}", d.changes);
    assert!(d.files.iter().all(|f| f.covered), "{:?}", d.files);
    let moved = d
        .changes
        .iter()
        .find(|c| matches!(c.change, ChangeKind::Moved { .. }))
        .unwrap();
    assert!(moved.fields.is_empty());
}

#[test]
fn moved_with_changes_keeps_the_fields() {
    let old = set(&[("a.yml", &req("Alpha", "http://x/a", ""))]);
    let new = set(&[("z.yml", &req("Alpha", "http://x/other", ""))]);
    let d = diff(&old, &new).unwrap();
    assert_eq!(d.changes.len(), 1);
    assert!(matches!(d.changes[0].change, ChangeKind::Moved { .. }));
    assert_eq!(text(&field(&d.changes[0], &["url"]).new), "http://x/other");
}

#[test]
fn comment_only_change_is_not_covered() {
    let old = set(&[("a.yml", &req("A", "u", ""))]);
    let new = set(&[("a.yml", &format!("# hello\n{}", req("A", "u", "")))]);
    let d = diff(&old, &new).unwrap();
    assert!(d.changes.is_empty());
    assert_eq!(d.files.len(), 1);
    assert!(!d.files[0].covered);
}

#[test]
fn environments_hide_secret_values() {
    let env = |extra: &str| format!("name: Dev\nvariables:\n  - name: host\n    value: a\n{extra}");
    let old = set(&[(
        "environments/Dev.yml",
        &env("  - name: token\n    value: plain-old\n"),
    )]);
    let new = set(&[(
        "environments/Dev.yml",
        &env("  - name: token\n    secret: true\n  - name: other\n    value: x\n"),
    )]);
    let d = diff(&old, &new).unwrap();
    let c = &d.changes[0];
    assert_eq!(c.target, ChangeTarget::Environment { name: "Dev".into() });
    assert_eq!(c.file, "environments/Dev.yml");
    let t = field(c, &["variables", "token"]);
    assert_eq!(t.old, Some(DiffValue::Secret));
    assert_eq!(t.new, Some(DiffValue::Secret));
    assert!(
        !serde_json::to_string(&d.changes)
            .unwrap()
            .contains("plain-old")
    );
    assert_eq!(text(&field(c, &["variables", "other"]).new), "x");
}

#[test]
fn folder_and_collection_changes() {
    let old = set(&[("users/folder.yml", "info:\n  name: Users\n  seq: 1\n")]);
    let mut new = set(&[("users/folder.yml", "info:\n  name: People\n  seq: 1\n")]);
    new.files.insert(
        "opencollection.yml".into(),
        b"opencollection: 1.0.0\n\ninfo:\n  name: renamed\n".to_vec(),
    );
    let d = diff(&old, &new).unwrap();
    assert_eq!(d.changes.len(), 2);
    assert_eq!(d.changes[0].target, ChangeTarget::Collection);
    assert_eq!(text(&field(&d.changes[0], &["name"]).new), "renamed");
    assert_eq!(
        d.changes[1].target,
        ChangeTarget::Folder {
            path: "users".into()
        }
    );
    assert_eq!(field(&d.changes[1], &["name"]).line, Some(2));
}

#[test]
fn a_new_collection_is_all_added() {
    let new = set(&[("a.yml", &req("A", "u", ""))]);
    let d = diff(&FileSet::default(), &new).unwrap();
    assert!(d.changes.iter().all(|c| c.change == ChangeKind::Added));
    assert_eq!(d.changes.len(), 2);
    let back = diff(&new, &FileSet::default()).unwrap();
    assert!(back.changes.iter().all(|c| c.change == ChangeKind::Removed));
}

#[test]
fn unreadable_side_is_an_error() {
    let mut bad = FileSet::default();
    bad.files.insert("x.yml".into(), b"a: 1".to_vec());
    assert!(diff(&bad, &set(&[])).is_err());
}

#[test]
fn the_diff_follows_the_written_edit() {
    // Whatever the writer changes in a real collection shows up as fields, and the writer's
    // output is "covered" (it is exactly what the model change says).
    let old = set(&[(
        "a.yml",
        &req("A", "u", "  headers:\n    - name: A\n      value: 1\n"),
    )]);
    let mut doc: CollectionDoc = read_collection(&old).unwrap();
    let mut spec = doc.model().request("a.yml").unwrap().request.spec.clone();
    spec.method = "POST".into();
    spec.headers.push(rt_core::model::KeyValue::new("B", "2"));
    doc.set_request("a.yml", &spec).unwrap();
    let d = diff(&old, &doc.write()).unwrap();
    let c = &d.changes[0];
    assert_eq!(text(&field(c, &["method"]).new), "POST");
    assert_eq!(text(&field(c, &["headers", "B"]).new), "2");
    assert!(d.files.iter().all(|f| f.covered));
}

#[test]
fn a_value_written_for_a_secret_is_reported_without_the_value() {
    let env =
        |value: &str| format!("name: Dev\nvariables:\n  - name: token\n    secret: true\n{value}");
    let old = set(&[("environments/Dev.yml", &env(""))]);
    let new = set(&[("environments/Dev.yml", &env("    value: hunter2-secret\n"))]);
    let d = diff(&old, &new).unwrap();
    let c = &d.changes[0];
    assert_eq!(c.target, ChangeTarget::Environment { name: "Dev".into() });
    let f = field(c, &["variables", "token", "@file-value"]);
    assert_eq!(f.old, None);
    assert_eq!(f.new, Some(DiffValue::Secret));
    assert!(!serde_json::to_string(&d).unwrap().contains("hunter2"));
    assert!(d.files[0].covered);
}
