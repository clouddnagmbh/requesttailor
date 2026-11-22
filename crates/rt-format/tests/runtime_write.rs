// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance: writing scripts, assertions and actions (wave 7, strand B).
//!
//! - Request files: `runtime.scripts`, `runtime.assertions` and `runtime.actions` can be
//!   added, changed and removed; the result reads back as the edited request and nothing else
//!   in the file changes. Checked for every request of the Bruno corpus.
//! - Collection and folder files: `request.scripts` likewise (`set_folder_scripts`).
//! - Script entries of other phases (gRPC) and actions other than `set-variable` stay in the
//!   file untouched, in their place.
//! - CRLF files stay CRLF; script code of any shape reads back exactly.

use std::path::{Path, PathBuf};

use rt_core::model::{
    Assertion, Auth, Folder, Item, RequestSpec, Script, ScriptPhase, VarAction, VarScope,
};
use rt_format::{COLLECTION_FILE, CollectionDoc, FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn corpus(name: &str) -> FileSet {
    FileSet::from_dir(&corpus_dir().join(name)).unwrap()
}

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

fn text(set: &FileSet, path: &str) -> String {
    String::from_utf8(set.files[path].clone()).unwrap()
}

/// Every collection root of the corpus (nested collections are their own roots).
fn all_roots() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if dir.join(COLLECTION_FILE).is_file() {
            out.push(dir.to_path_buf());
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries.into_iter().filter(|p| p.is_dir()) {
            walk(&path, out);
        }
    }
    let mut out = Vec::new();
    walk(&corpus_dir(), &mut out);
    assert!(out.len() > 80, "corpus not found");
    out
}

fn walk_items<'a>(items: &'a [Item], reqs: &mut Vec<String>, folders: &mut Vec<&'a Folder>) {
    for item in items {
        match item {
            Item::Folder(f) => {
                folders.push(f);
                walk_items(&f.items, reqs, folders);
            }
            Item::Request(r) => reqs.push(r.path.clone()),
        }
    }
}

fn requests(doc: &CollectionDoc) -> Vec<String> {
    let mut reqs = Vec::new();
    walk_items(&doc.model().items, &mut reqs, &mut Vec::new());
    reqs
}

fn folders(doc: &CollectionDoc) -> Vec<(String, Vec<Script>)> {
    let mut list = Vec::new();
    walk_items(&doc.model().items, &mut Vec::new(), &mut list);
    list.iter()
        .map(|f| (f.path.clone(), f.defaults.scripts.clone()))
        .collect()
}

fn folder_scripts(doc: &CollectionDoc, path: &str) -> Vec<Script> {
    if path.is_empty() {
        return doc.model().defaults.scripts.clone();
    }
    folders(doc)
        .into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, s)| s)
        .unwrap()
}

fn spec(doc: &CollectionDoc, path: &str) -> RequestSpec {
    doc.model().request(path).unwrap().request.spec.clone()
}

/// Paths whose bytes differ between two file sets (including added and removed files).
fn changed(a: &FileSet, b: &FileSet) -> Vec<String> {
    let mut out: Vec<String> = a
        .files
        .iter()
        .filter(|(p, bytes)| b.files.get(*p) != Some(bytes))
        .map(|(p, _)| p.clone())
        .collect();
    out.extend(
        b.files
            .keys()
            .filter(|p| !a.files.contains_key(*p))
            .cloned(),
    );
    out.sort();
    out
}

fn script(phase: ScriptPhase, code: &str) -> Script {
    Script {
        phase,
        code: code.into(),
    }
}

fn assertion(expression: &str, operator: &str, value: &str) -> Assertion {
    Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.into(),
        description: None,
        enabled: true,
    }
}

fn action(variable: &str, expression: &str) -> VarAction {
    VarAction {
        phase: ScriptPhase::AfterResponse,
        expression: expression.into(),
        method: "jsonq".into(),
        variable: variable.into(),
        scope: VarScope::Runtime,
        enabled: true,
    }
}

/// Adds, changes and removes a script, an assertion and an action.
fn edited(s: &RequestSpec) -> RequestSpec {
    let mut e = s.clone();
    match e.scripts.first_mut() {
        Some(first) => first
            .code
            .push_str("\n// edited: it's \"quoted\"\n  indented"),
        None => e.scripts.push(script(
            ScriptPhase::BeforeRequest,
            "rt.vars.set('a', 1);\nconsole.log(\"x: y\");",
        )),
    }
    if e.scripts.len() > 1 {
        e.scripts.remove(1);
    }
    e.scripts.push(script(
        ScriptPhase::Tests,
        "test('ok', () => expect(1).to.equal(1));",
    ));
    match e.assertions.first_mut() {
        Some(first) => {
            first.value = "201".into();
            first.enabled = !first.enabled;
            first.description = Some("edited".into());
        }
        None => e.assertions.push(assertion("res.status", "eq", "200")),
    }
    if e.assertions.len() > 2 {
        e.assertions.remove(1);
    }
    e.assertions.push(Assertion {
        description: Some("multi\nline".into()),
        enabled: false,
        ..assertion("res.body", "isJson", "")
    });
    match e.actions.first_mut() {
        Some(first) => {
            first.expression = "res.body.id".into();
            first.scope = VarScope::Environment;
        }
        None => e.actions.push(action("token", "res.body.token")),
    }
    e.actions.push(VarAction {
        method: "header".into(),
        scope: VarScope::Global,
        enabled: false,
        ..action("etag", "etag")
    });
    e
}

#[test]
fn every_corpus_request_can_get_scripts_assertions_and_actions_and_lose_them_again() {
    for root in all_roots() {
        let input = FileSet::from_dir(&root).unwrap();
        let base = read_collection(&input).unwrap();
        let original = base.write();
        for path in requests(&base) {
            let ctx = format!("{}: {path}", root.display());
            let mut doc = base.clone();
            let before = spec(&doc, &path);
            // Wave 8: WebSocket and gRPC items have no assertions or actions; their scripts
            // are covered by their own tests (`tests/websocket.rs`).
            if !before.protocol.is_http() {
                continue;
            }
            let after = edited(&before);
            doc.set_request(&path, &after)
                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            let written = doc.write();
            assert_eq!(changed(&original, &written), vec![path.clone()], "{ctx}");
            let reread = read_collection(&written).unwrap();
            assert_eq!(spec(&reread, &path), after, "{ctx}");
            assert_eq!(spec(&doc, &path), after, "{ctx}: model view");
            // Back to the original request.
            doc.set_request(&path, &before)
                .unwrap_or_else(|e| panic!("{ctx}: back: {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(spec(&reread, &path), before, "{ctx}: back");
            // And without any of them.
            let mut bare = before.clone();
            bare.scripts.clear();
            bare.assertions.clear();
            bare.actions.clear();
            doc.set_request(&path, &bare)
                .unwrap_or_else(|e| panic!("{ctx}: bare: {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(spec(&reread, &path), bare, "{ctx}: bare");
        }
    }
}

#[test]
fn every_corpus_collection_and_folder_can_change_its_scripts() {
    for root in all_roots() {
        let input = FileSet::from_dir(&root).unwrap();
        let base = read_collection(&input).unwrap();
        let original = base.write();
        let mut targets = vec![String::new()];
        targets.extend(folders(&base).into_iter().map(|(p, _)| p));
        for folder in targets {
            let ctx = format!("{}: folder {folder:?}", root.display());
            let mut doc = base.clone();
            let before = folder_scripts(&doc, &folder);
            let mut after = before.clone();
            after.push(script(
                ScriptPhase::AfterResponse,
                "bru.setVar('x', res.status);",
            ));
            if let Some(first) = after.first_mut() {
                first.code.push_str("\n// edited");
            }
            doc.set_folder_scripts(&folder, &after)
                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            let written = doc.write();
            let file = if folder.is_empty() {
                COLLECTION_FILE.to_owned()
            } else {
                format!("{folder}/folder.yml")
            };
            assert_eq!(changed(&original, &written), vec![file.clone()], "{ctx}");
            let reread = read_collection(&written).unwrap();
            assert_eq!(reread.issues(), base.issues(), "{ctx}");
            assert_eq!(folder_scripts(&reread, &folder), after, "{ctx}");
            assert_eq!(folder_scripts(&doc, &folder), after, "{ctx}: model view");
            doc.set_folder_scripts(&folder, &before)
                .unwrap_or_else(|e| panic!("{ctx}: back: {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(folder_scripts(&reread, &folder), before, "{ctx}: back");
        }
    }
}

#[test]
fn unchanged_scripts_change_no_byte() {
    for root in all_roots() {
        let input = FileSet::from_dir(&root).unwrap();
        let mut doc = read_collection(&input).unwrap();
        let original = doc.write();
        for path in requests(&doc) {
            let s = spec(&doc, &path);
            doc.set_request(&path, &s).unwrap();
        }
        for (folder, scripts) in folders(&doc) {
            doc.set_folder_scripts(&folder, &scripts).unwrap();
        }
        let scripts = doc.model().defaults.scripts.clone();
        doc.set_folder_scripts("", &scripts).unwrap();
        assert!(
            changed(&original, &doc.write()).is_empty(),
            "{}",
            root.display()
        );
    }
}

#[test]
fn editing_one_script_rewrites_only_its_code() {
    let input = corpus("scripting-quickjs-safe-mode-quickjs-safe-mode");
    let mut doc = read_collection(&input).unwrap();
    let path = "03-recovered.yml";
    let mut s = spec(&doc, path);
    s.scripts[0].code = "test(\"recovered\", function() {\n  expect(1).to.equal(1);\n});".into();
    doc.set_request(path, &s).unwrap();
    let before = text(&input, path);
    let after = text(&doc.write(), path);
    let old_block = "      code: |-\n        test(\"scripts still run after the trap: fresh engine module serves\", function() {\n          bru.setVar('recovered', true);\n          expect(bru.getVar('recovered')).to.equal(true);\n        });\n";
    let new_block = "      code: |-\n        test(\"recovered\", function() {\n          expect(1).to.equal(1);\n        });\n";
    assert!(before.contains(old_block), "{before}");
    assert_eq!(after, before.replace(old_block, new_block));
}

#[test]
fn editing_an_assertion_keeps_the_others_and_their_descriptions() {
    let input = corpus("request-description-yml");
    let mut doc = read_collection(&input).unwrap();
    let path = requests(&doc)
        .into_iter()
        .find(|p| !spec(&doc, p).assertions.is_empty())
        .unwrap();
    let mut s = spec(&doc, &path);
    let before = text(&input, &path);
    s.assertions[0].value = "201".into();
    doc.set_request(&path, &s).unwrap();
    let after = text(&doc.write(), &path);
    assert_eq!(
        after,
        before.replacen(
            "      operator: eq\n      value: \"200\"\n",
            "      operator: eq\n      value: \"201\"\n",
            1
        )
    );
    // Removing the first one leaves the second as it was.
    s.assertions.remove(0);
    doc.set_request(&path, &s).unwrap();
    let after = text(&doc.write(), &path);
    assert!(!after.contains("Single-line assert desc"));
    assert!(after.contains("    - expression: res.body.ok\n      operator: eq\n      value: \"true\"\n      description: |-\n"));
}

const GRPC_AND_OTHER: &str = "info:\n  name: mixed\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://x\n\nruntime:\n  scripts:\n    - type: before-request\n      code: one()\n    - type: grpc:before-call-start\n      code: grpc()\n    - type: tests\n      code: two()\n  actions:\n    - type: future-action\n      foo: bar\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.body.id\n        method: jsonq\n      variable:\n        name: id\n        scope: runtime\n";

fn mixed() -> CollectionDoc {
    read_collection(&files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: c\n",
        ),
        ("mixed.yml", GRPC_AND_OTHER),
    ]))
    .unwrap()
}

#[test]
fn grpc_scripts_and_other_actions_stay_in_place() {
    let mut doc = mixed();
    let s = spec(&doc, "mixed.yml");
    assert_eq!(s.scripts.len(), 2);
    assert_eq!(s.actions.len(), 1);

    // Change the script after the gRPC one, add one at the end.
    let mut e = s.clone();
    e.scripts[1].code = "three()".into();
    e.scripts.push(script(ScriptPhase::AfterResponse, "four()"));
    doc.set_request("mixed.yml", &e).unwrap();
    assert_eq!(
        text(&doc.write(), "mixed.yml"),
        GRPC_AND_OTHER
            .replace("code: two()", "code: three()")
            .replace(
                "      code: three()\n",
                "      code: three()\n    - type: after-response\n      code: four()\n"
            )
    );

    // Remove every modelled entry: the others stay.
    let mut bare = s.clone();
    bare.scripts.clear();
    bare.actions.clear();
    doc.set_request("mixed.yml", &bare).unwrap();
    let out = text(&doc.write(), "mixed.yml");
    assert_eq!(
        out,
        "info:\n  name: mixed\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://x\n\nruntime:\n  scripts:\n    - type: grpc:before-call-start\n      code: grpc()\n  actions:\n    - type: future-action\n      foo: bar\n"
    );
    assert_eq!(
        spec(&read_collection(&doc.write()).unwrap(), "mixed.yml"),
        bare
    );

    // Adding them back puts them after the kept entries.
    doc.set_request("mixed.yml", &s).unwrap();
    let out = text(&doc.write(), "mixed.yml");
    assert!(out.contains("    - type: grpc:before-call-start\n      code: grpc()\n    - type: before-request\n      code: one()\n    - type: tests\n      code: two()\n"), "{out}");
    assert!(
        out.contains("    - type: future-action\n      foo: bar\n    - type: set-variable\n"),
        "{out}"
    );
    assert_eq!(
        spec(&read_collection(&doc.write()).unwrap(), "mixed.yml"),
        s
    );
}

#[test]
fn editing_an_action_edits_its_fields_in_place() {
    let mut doc = mixed();
    let mut s = spec(&doc, "mixed.yml");
    s.actions[0].expression = "res.body.token".into();
    s.actions[0].scope = VarScope::Collection;
    s.actions[0].enabled = false;
    doc.set_request("mixed.yml", &s).unwrap();
    assert_eq!(
        text(&doc.write(), "mixed.yml"),
        GRPC_AND_OTHER
            .replace("expression: res.body.id", "expression: res.body.token")
            .replace(
                "scope: runtime\n",
                "scope: collection\n      disabled: true\n"
            )
    );
}

fn fresh() -> FileSet {
    files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: fresh\n",
        ),
        (
            "users/folder.yml",
            "info:\n  name: users\n  type: folder\n  seq: 3\n",
        ),
        (
            "users/get.yml",
            "info:\n  name: get\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: https://example.com\n",
        ),
    ])
}

#[test]
fn new_requests_with_scripts_assertions_and_actions_are_laid_out_like_bruno() {
    let mut doc = read_collection(&fresh()).unwrap();
    let spec_in = RequestSpec {
        name: "Login".into(),
        method: "POST".into(),
        url: "https://example.com/login".into(),
        scripts: vec![
            script(ScriptPhase::BeforeRequest, "bru.setVar('ts', Date.now());"),
            script(
                ScriptPhase::Tests,
                "test(\"ok\", function() {\n  expect(res.getStatus()).to.equal(200);\n});",
            ),
        ],
        assertions: vec![
            assertion("res.status", "eq", "200"),
            Assertion {
                description: Some("has a token".into()),
                enabled: false,
                ..assertion("res.body.token", "isDefined", "")
            },
        ],
        actions: vec![action("token", "res.body.token")],
        seq: Some(1.0),
        auth: Auth::None,
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &spec_in).unwrap();
    let out = text(&doc.write(), &path);
    assert_eq!(
        out,
        "info:\n  name: Login\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: https://example.com/login\n\nruntime:\n  scripts:\n    - type: before-request\n      code: bru.setVar('ts', Date.now());\n    - type: tests\n      code: |-\n        test(\"ok\", function() {\n          expect(res.getStatus()).to.equal(200);\n        });\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n    - expression: res.body.token\n      operator: isDefined\n      description: has a token\n      disabled: true\n  actions:\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.body.token\n        method: jsonq\n      variable:\n        name: token\n        scope: runtime\n"
    );
    let reread = read_collection(&doc.write()).unwrap();
    let got = spec(&reread, &path);
    assert_eq!(got.scripts, spec_in.scripts);
    assert_eq!(got.assertions, spec_in.assertions);
    assert_eq!(got.actions, spec_in.actions);
}

#[test]
fn script_code_of_any_shape_reads_back() {
    let codes = [
        "",
        "x",
        "  leading spaces\nand more",
        "\n\nleading empty lines",
        "trailing empty lines\n\n",
        "trailing newline\n",
        "trailing spaces   \nnext",
        "tabs\tinside\n\tand indented",
        "windows\r\nline ends",
        "# not a comment",
        "- not a list",
        "key: value",
        "'quoted'",
        "\"double\"",
        "true",
        "42",
        "null",
        "---\ndocument marker",
        "unicode: äöü ✓",
        "ends with colon:",
    ];
    for nl in ["\n", "\r\n"] {
        let mut doc = read_collection(&files(&[
            (
                COLLECTION_FILE,
                &format!("opencollection: \"1.0.0\"{nl}info:{nl}  name: c{nl}"),
            ),
            (
                "a.yml",
                &format!(
                    "info:{nl}  name: a{nl}  type: http{nl}  seq: 1{nl}{nl}http:{nl}  method: GET{nl}  url: https://x{nl}"
                ),
            ),
        ]))
        .unwrap();
        for code in codes {
            let mut s = spec(&doc, "a.yml");
            s.scripts = vec![
                script(ScriptPhase::BeforeRequest, code),
                script(ScriptPhase::Tests, &format!("{code}{code}")),
            ];
            doc.set_request("a.yml", &s)
                .unwrap_or_else(|e| panic!("{code:?} ({nl:?}): {e}"));
            let written = doc.write();
            let reread = read_collection(&written).unwrap();
            assert_eq!(
                spec(&reread, "a.yml").scripts,
                s.scripts,
                "{code:?} ({nl:?})"
            );
            doc.set_folder_scripts("", &s.scripts)
                .unwrap_or_else(|e| panic!("collection {code:?} ({nl:?}): {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(
                reread.model().defaults.scripts,
                s.scripts,
                "{code:?} ({nl:?})"
            );
            if nl == "\r\n" {
                for file in ["a.yml", COLLECTION_FILE] {
                    let t = text(&doc.write(), file);
                    assert!(
                        !t.replace("\r\n", "").contains('\n'),
                        "{file}: lone LF for {code:?}:\n{t:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn crlf_files_stay_crlf() {
    let input = files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\r\ninfo:\r\n  name: crlf\r\n",
        ),
        (
            "f/folder.yml",
            "info:\r\n  name: f\r\n  type: folder\r\n  seq: 1\r\n",
        ),
        ("f/a.yml", &GRPC_AND_OTHER.replace('\n', "\r\n")),
    ]);
    let mut doc = read_collection(&input).unwrap();
    let s = edited(&spec(&doc, "f/a.yml"));
    doc.set_request("f/a.yml", &s).unwrap();
    let code = [script(ScriptPhase::BeforeRequest, "one();\ntwo();")];
    doc.set_folder_scripts("", &code).unwrap();
    doc.set_folder_scripts("f", &code).unwrap();
    let out = doc.write();
    for (path, bytes) in &out.files {
        let t = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            !t.replace("\r\n", "").contains('\n'),
            "{path}: lone LF\n{t:?}"
        );
        assert!(t.contains("code: |-\r\n"), "{path}: {t:?}");
    }
    let reread = read_collection(&out).unwrap();
    assert_eq!(spec(&reread, "f/a.yml"), s);
    assert_eq!(reread.model().defaults.scripts, code);
    assert_eq!(folder_scripts(&reread, "f"), code);
}

#[test]
fn collection_and_folder_scripts_are_added_and_removed() {
    let mut doc = read_collection(&fresh()).unwrap();
    let s = [script(
        ScriptPhase::BeforeRequest,
        "req.setHeader('a', 'b');\nbru.setVar('x', 1);",
    )];
    doc.set_folder_scripts("", &s).unwrap();
    doc.set_folder_scripts("users", &s).unwrap();
    let out = doc.write();
    assert_eq!(
        text(&out, COLLECTION_FILE),
        "opencollection: \"1.0.0\"\ninfo:\n  name: fresh\n\nrequest:\n  scripts:\n    - type: before-request\n      code: |-\n        req.setHeader('a', 'b');\n        bru.setVar('x', 1);\n"
    );
    assert_eq!(
        text(&out, "users/folder.yml"),
        "info:\n  name: users\n  type: folder\n  seq: 3\n\nrequest:\n  scripts:\n    - type: before-request\n      code: |-\n        req.setHeader('a', 'b');\n        bru.setVar('x', 1);\n"
    );
    let reread = read_collection(&out).unwrap();
    assert_eq!(reread.model().defaults.scripts, s);
    assert_eq!(folder_scripts(&reread, "users"), s);

    // Removing them restores the files.
    doc.set_folder_scripts("", &[]).unwrap();
    doc.set_folder_scripts("users", &[]).unwrap();
    assert!(changed(&fresh(), &doc.write()).is_empty());

    // Scripts next to headers keep the headers.
    let mut doc = read_collection(&corpus("response-timeline-headers-yml")).unwrap();
    doc.set_folder_scripts("", &[]).unwrap();
    let t = text(&doc.write(), COLLECTION_FILE);
    assert!(!t.contains("scripts:"));
    assert!(t.contains("    - name: shared-header-1\n      value: collection-shared-value-1\n"));
    assert_eq!(
        read_collection(&doc.write())
            .unwrap()
            .model()
            .defaults
            .headers
            .len(),
        8
    );

    assert!(doc.set_folder_scripts("missing", &s).is_err());
}

#[test]
fn a_folder_without_folder_file_gets_one() {
    let mut doc = read_collection(&files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: c\n",
        ),
        (
            "plain/a.yml",
            "info:\n  name: a\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://x\n",
        ),
    ]))
    .unwrap();
    // A directory is a folder even without `folder.yml`.
    assert!(folders(&doc).iter().any(|(p, _)| p == "plain"));
    // Nothing to write: no file appears.
    doc.set_folder_scripts("plain", &[]).unwrap();
    assert!(!doc.write().files.contains_key("plain/folder.yml"));
    let s = [script(ScriptPhase::Tests, "test('t', () => {});")];
    doc.set_folder_scripts("plain", &s).unwrap();
    let reread = read_collection(&doc.write()).unwrap();
    assert_eq!(folder_scripts(&reread, "plain"), s);
    assert_eq!(
        folders(&reread)
            .iter()
            .filter(|(p, _)| p == "plain")
            .count(),
        1
    );
    assert!(reread.model().request("plain/a.yml").is_some());
    assert_eq!(
        text(&doc.write(), "plain/folder.yml"),
        "info:\n  name: plain\n  type: folder\n\nrequest:\n  scripts:\n    - type: tests\n      code: test('t', () => {});\n"
    );
}
