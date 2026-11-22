// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests of the semantic merge (wave 12, gate item 2), over the Bruno corpus:
//! different fields merge cleanly with both changes in and everything else byte-identical;
//! the same field gives one `FieldConflict` that `resolve` decides; new, deleted and renamed
//! requests; plus environments, secrets, folder scripts and files outside the model.

use std::path::Path;

use rt_core::model::{Item, KeyValue, RequestEntry, Script, ScriptPhase, Variable};
use rt_format::diff::{ChangeTarget, DiffValue};
use rt_format::merge::{ConflictChoice, Side, merge, merge_file, merge_marked, resolve};
use rt_format::{FileSet, read_collection};

fn corpus() -> Vec<(String, FileSet)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno");
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("opencollection.yml").exists())
        .collect();
    dirs.sort_by_key(|e| e.file_name());
    dirs.into_iter()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name, FileSet::from_dir(&e.path()).unwrap())
        })
        .collect()
}

fn requests(items: &[Item], out: &mut Vec<RequestEntry>) {
    for i in items {
        match i {
            Item::Request(r) => out.push(r.clone()),
            Item::Folder(f) => requests(&f.items, out),
        }
    }
}

fn all_requests(files: &FileSet) -> Vec<RequestEntry> {
    let mut out = Vec::new();
    requests(&read_collection(files).unwrap().model().items, &mut out);
    out
}

/// `files` with the request at `path` edited by `edit`, or `None` if the writer refuses.
fn edited(
    files: &FileSet,
    path: &str,
    edit: impl Fn(&mut rt_core::model::RequestSpec),
) -> Option<FileSet> {
    let mut doc = read_collection(files).ok()?;
    let mut spec = doc.model().request(path)?.request.spec.clone();
    edit(&mut spec);
    doc.set_request(path, &spec).ok()?;
    let mut out = files.clone();
    out.files.extend(doc.write().files);
    Some(out)
}

fn spec(files: &FileSet, path: &str) -> rt_core::model::RequestSpec {
    read_collection(files)
        .unwrap()
        .model()
        .request(path)
        .unwrap()
        .request
        .spec
        .clone()
}

fn assert_rest_equal(merged: &FileSet, base: &FileSet, except: &[&str]) {
    for (path, bytes) in &base.files {
        if except.contains(&path.as_str()) {
            continue;
        }
        assert_eq!(merged.files.get(path), Some(bytes), "{path} is untouched");
    }
}

/// Requests of the corpus the writer can edit, one per collection.
fn editable() -> Vec<(String, FileSet, String)> {
    let mut out = Vec::new();
    for (name, files) in corpus() {
        let Ok(doc) = read_collection(&files) else {
            continue;
        };
        let _ = doc;
        for r in all_requests(&files) {
            if r.spec.protocol.is_http()
                && edited(&files, &r.path, |s| s.url.push_str("/x")).is_some()
            {
                out.push((name, files, r.path));
                break;
            }
        }
    }
    assert!(
        out.len() >= 50,
        "most corpus collections have an editable request, got {}",
        out.len()
    );
    out
}

#[test]
fn different_fields_merge_cleanly_and_everything_else_stays() {
    for (name, base, path) in editable() {
        let ours = edited(&base, &path, |s| s.url.push_str("/ours")).unwrap();
        let theirs = edited(&base, &path, |s| {
            s.headers.push(KeyValue::new("X-Merge-Test", "theirs"))
        })
        .unwrap();
        let r = merge(&base, &ours, &theirs).unwrap();
        assert!(
            r.is_clean(),
            "{name}: {:?} {:?}",
            r.conflicts,
            r.text_conflicts
        );
        let got = spec(&r.files, &path);
        let want = spec(&ours, &path);
        assert_eq!(got.url, want.url, "{name}: ours' URL");
        assert!(
            got.headers
                .iter()
                .any(|h| h.name == "X-Merge-Test" && h.value == "theirs"),
            "{name}: theirs' header"
        );
        // Both edits one after the other give exactly the merged file.
        let both = edited(&ours, &path, |s| {
            s.headers.push(KeyValue::new("X-Merge-Test", "theirs"))
        })
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&r.files.files[&path]),
            String::from_utf8_lossy(&both.files[&path]),
            "{name}: merged file"
        );
        assert_rest_equal(&r.files, &base, &[&path]);
        assert_eq!(
            r.files.files.len(),
            base.files.len(),
            "{name}: no file more or less"
        );
    }
}

#[test]
fn the_same_field_conflicts_once_and_resolves_to_either_side() {
    for (name, base, path) in editable() {
        let ours = edited(&base, &path, |s| s.url.push_str("/ours")).unwrap();
        let theirs = edited(&base, &path, |s| s.url.push_str("/theirs")).unwrap();
        let r = merge(&base, &ours, &theirs).unwrap();
        assert!(
            r.text_conflicts.is_empty(),
            "{name}: {:?}",
            r.text_conflicts
        );
        assert_eq!(r.conflicts.len(), 1, "{name}: {:?}", r.conflicts);
        let c = &r.conflicts[0];
        assert_eq!(c.field, vec!["url".to_string()], "{name}");
        assert_eq!(c.file, path);
        assert_eq!(c.target, ChangeTarget::Request { path: path.clone() });
        let text = |files: &FileSet| DiffValue::Text {
            text: spec(files, &path).url,
        };
        assert_eq!(c.base, Some(text(&base)), "{name}");
        assert_eq!(c.ours, Some(text(&ours)), "{name}");
        assert_eq!(c.theirs, Some(text(&theirs)), "{name}");
        assert_eq!(
            r.files.files[&path], ours.files[&path],
            "{name}: ours until resolved"
        );
        assert!(resolve(&base, &ours, &theirs, &[]).is_err());
        for (side, want) in [(Side::Ours, &ours), (Side::Theirs, &theirs)] {
            let choice = ConflictChoice::Field {
                target: c.target.clone(),
                field: c.field.clone(),
                side,
            };
            let files = resolve(&base, &ours, &theirs, &[choice]).unwrap();
            assert_eq!(
                String::from_utf8_lossy(&files.files[&path]),
                String::from_utf8_lossy(&want.files[&path]),
                "{name}: resolved with {side:?}"
            );
            assert_rest_equal(&files, &base, &[&path]);
        }
    }
}

#[test]
fn new_and_deleted_requests_merge() {
    let mut checked = 0;
    for (name, base, _) in editable() {
        let all = all_requests(&base);
        if all.len() < 2 {
            continue;
        }
        let mut doc = read_collection(&base).unwrap();
        let new = rt_core::model::RequestSpec {
            name: "Added by ours".into(),
            method: "GET".into(),
            url: "https://example.test/added".into(),
            ..Default::default()
        };
        let added = doc.add_request("", &new).unwrap();
        let ours = doc.write();
        let gone = &all[1].path;
        let mut theirs = base.clone();
        theirs.files.remove(gone);
        let r = merge(&base, &ours, &theirs).unwrap();
        assert!(r.is_clean(), "{name}");
        assert_eq!(r.files.files.get(&added), ours.files.get(&added), "{name}");
        assert!(
            !r.files.files.contains_key(gone),
            "{name}: theirs deleted it"
        );
        assert_rest_equal(&r.files, &base, &[gone, &added]);
        checked += 1;
    }
    assert!(checked >= 20, "{checked}");
}

#[test]
fn a_rename_on_one_side_takes_the_other_sides_changes() {
    for (name, base, path) in editable() {
        // Ours renames the file (and the request); theirs changes the URL in the old file.
        let renamed = match path.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/renamed-by-ours.yml"),
            None => "renamed-by-ours.yml".to_string(),
        };
        let mut ours = edited(&base, &path, |s| s.name = "Renamed by ours".into()).unwrap();
        let bytes = ours.files.remove(&path).unwrap();
        ours.files.insert(renamed.clone(), bytes);
        let theirs = edited(&base, &path, |s| s.url.push_str("/theirs")).unwrap();
        let r = merge(&base, &ours, &theirs).unwrap();
        assert!(
            r.is_clean(),
            "{name}: {:?} {:?}",
            r.conflicts,
            r.text_conflicts
        );
        assert!(
            !r.files.files.contains_key(&path),
            "{name}: the old path is gone"
        );
        let got = spec(&r.files, &renamed);
        assert_eq!(got.name, "Renamed by ours", "{name}");
        assert_eq!(got.url, spec(&theirs, &path).url, "{name}");
        assert_rest_equal(&r.files, &base, &[&path]);
    }
}

fn set(files: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: files
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: Merge\n";

fn request(url: &str, headers: &str) -> String {
    format!(
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {url}\n{headers}\ndocs: |-\n  Some docs\n"
    )
}

#[test]
fn the_same_header_changed_differently_conflicts_by_name() {
    let h = |v: &str| {
        format!("  headers:\n    - name: X-A\n      value: {v}\n    - name: X-B\n      value: b\n")
    };
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://a", &h("1"))),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://a", &h("2"))),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://a", &h("3"))),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert_eq!(r.conflicts.len(), 1, "{r:?}");
    assert_eq!(r.conflicts[0].field, vec!["headers", "X-A"]);
    assert_eq!(
        r.conflicts[0].theirs,
        Some(DiffValue::Text { text: "3".into() })
    );
}

#[test]
fn changes_outside_the_model_are_kept_from_either_side() {
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://a", "")),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "get.yml",
            &request("https://a", "").replace("Some docs", "Better docs"),
        ),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://b", "")),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert!(r.is_clean());
    let text = String::from_utf8(r.files.files["get.yml"].clone()).unwrap();
    assert_eq!(
        text,
        request("https://b", "").replace("Some docs", "Better docs")
    );
    // The same the other way round.
    let r = merge(&base, &theirs, &ours).unwrap();
    assert!(r.is_clean());
    assert_eq!(
        String::from_utf8(r.files.files["get.yml"].clone()).unwrap(),
        text
    );
}

#[test]
fn environments_merge_per_variable_and_secrets_stay_hidden() {
    let env = |vars: &str| format!("name: Local\nvariables:\n{vars}");
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "environments/Local.yml",
            &env("  - name: a\n    value: \"1\"\n  - name: b\n    value: \"1\"\n"),
        ),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "environments/Local.yml",
            &env("  - name: a\n    value: \"2\"\n  - name: b\n    value: \"1\"\n"),
        ),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "environments/Local.yml",
            &env("  - name: a\n    value: \"1\"\n  - name: b\n    value: \"3\"\n"),
        ),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert!(r.is_clean(), "{:?}", r.conflicts);
    let doc = read_collection(&r.files).unwrap();
    let vars = &doc.model().environment("Local").unwrap().variables;
    assert_eq!(
        vars.iter()
            .map(|v| v.value.clone().unwrap())
            .collect::<Vec<_>>(),
        ["2", "3"]
    );

    // A variable one side made secret and the other changed: a conflict without values.
    let marker = "plain-marker-value-4711";
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "environments/Local.yml",
            &env(&format!(
                "  - name: a\n    value: {marker}\n  - name: b\n    value: \"1\"\n"
            )),
        ),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        (
            "environments/Local.yml",
            &env("  - name: a\n    secret: true\n  - name: b\n    value: \"1\"\n"),
        ),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert_eq!(r.conflicts.len(), 1, "{:?}", r.conflicts);
    let c = &r.conflicts[0];
    assert_eq!(
        c.target,
        ChangeTarget::Environment {
            name: "Local".into()
        }
    );
    assert_eq!(c.field, vec!["variables", "a"]);
    assert_eq!(c.theirs, Some(DiffValue::Secret));
    assert!(!format!("{:?}", r.conflicts).contains(marker));
    let _ = Variable::new("unused", "");
}

#[test]
fn folder_scripts_merge_per_phase() {
    let folder = |pre: &str, tests: &str| {
        format!(
            "info:\n  name: Users\n  type: folder\nrequest:\n  scripts:\n    - type: before-request\n      code: {pre}\n    - type: tests\n      code: {tests}\n"
        )
    };
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("a()", "t()")),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("b()", "t()")),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("a()", "u()")),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert!(r.is_clean(), "{:?} {:?}", r.conflicts, r.text_conflicts);
    let doc = read_collection(&r.files).unwrap();
    let Item::Folder(f) = &doc.model().items[0] else {
        panic!("a folder")
    };
    assert_eq!(
        f.defaults.scripts,
        vec![
            Script {
                phase: ScriptPhase::BeforeRequest,
                code: "b()".into()
            },
            Script {
                phase: ScriptPhase::Tests,
                code: "u()".into()
            },
        ]
    );
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("c()", "t()")),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(
        r.conflicts[0].target,
        ChangeTarget::Folder {
            path: "users".into()
        }
    );
    assert_eq!(
        r.conflicts[0].field,
        vec!["defaults", "scripts", "before-request"]
    );
}

#[test]
fn files_outside_the_model_merge_line_by_line() {
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("README.md", "a\nb\nc\n"),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        ("README.md", "A\nb\nc\n"),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("README.md", "a\nb\nC\n"),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert!(r.is_clean());
    assert_eq!(r.files.files["README.md"], b"A\nb\nC\n");
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("README.md", "X\nb\nc\n"),
    ]);
    let (r, marked) = merge_marked(&base, &ours, &theirs, &[]).unwrap();
    assert_eq!(r.text_conflicts, vec!["README.md".to_string()]);
    assert_eq!(
        r.files.files["README.md"], b"A\nb\nc\n",
        "ours until resolved"
    );
    assert!(String::from_utf8_lossy(&marked["README.md"]).contains("<<<<<<< ours"));
}

#[test]
fn deleted_on_one_side_and_changed_on_the_other_is_a_file_decision() {
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://a", "")),
    ]);
    let ours = set(&[("opencollection.yml", COLLECTION)]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", &request("https://b", "")),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert_eq!(r.text_conflicts, vec!["get.yml".to_string()]);
    assert!(!r.files.files.contains_key("get.yml"));
    let keep = ConflictChoice::File {
        path: "get.yml".into(),
        side: Side::Theirs,
    };
    let files = resolve(&base, &ours, &theirs, &[keep]).unwrap();
    assert_eq!(files.files["get.yml"], theirs.files["get.yml"]);
}

#[test]
fn one_file_on_its_own_gets_comments_above_conflicted_fields() {
    let base = request("https://a", "");
    let ours = request("https://ours", "");
    let theirs = request("https://theirs", "");
    let out = merge_file(
        Some("get.yml"),
        Some(base.as_bytes()),
        Some(ours.as_bytes()),
        Some(theirs.as_bytes()),
    );
    assert_eq!(out.conflicts.len(), 1);
    let text = String::from_utf8(out.bytes.unwrap()).unwrap();
    assert!(
        text.contains("  # rt merge-driver: conflict in url (ours kept below). Theirs: https://theirs\n  url: https://ours\n"),
        "{text}"
    );
    // Still a valid request file that reads as ours.
    let files = set(&[("opencollection.yml", COLLECTION), ("get.yml", &text)]);
    assert_eq!(spec(&files, "get.yml").url, "https://ours");

    let theirs = request("https://a", "  headers:\n    - name: X\n      value: y\n");
    let out = merge_file(
        Some("get.yml"),
        Some(base.as_bytes()),
        Some(ours.as_bytes()),
        Some(theirs.as_bytes()),
    );
    assert!(out.conflicts.is_empty() && !out.text_conflict);
    let files = set(&[
        ("opencollection.yml", COLLECTION),
        ("get.yml", std::str::from_utf8(&out.bytes.unwrap()).unwrap()),
    ]);
    let s = spec(&files, "get.yml");
    assert_eq!((s.url.as_str(), s.headers.len()), ("https://ours", 1));
}

#[test]
fn folder_defaults_and_scripts_from_different_sides_merge() {
    let folder = |header: &str, pre: &str| {
        format!(
            "info:\n  name: Users\n  type: folder\nrequest:\n  headers:\n    - name: X-Team\n      value: {header}\n  scripts:\n    - type: before-request\n      code: {pre}\n"
        )
    };
    let base = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("a", "a()")),
    ]);
    let ours = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("b", "a()")),
    ]);
    let theirs = set(&[
        ("opencollection.yml", COLLECTION),
        ("users/folder.yml", &folder("a", "b()")),
    ]);
    let r = merge(&base, &ours, &theirs).unwrap();
    assert!(r.is_clean(), "{:?} {:?}", r.conflicts, r.text_conflicts);
    assert_eq!(
        String::from_utf8(r.files.files["users/folder.yml"].clone()).unwrap(),
        folder("b", "b()")
    );
}
