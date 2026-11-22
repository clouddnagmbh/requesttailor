// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance: writing the model back to files (wave 3, strand F).
//!
//! - Editing a request rewrites only that file and only what changed; reading the result
//!   gives the edited request. Checked for every request of the Bruno corpus.
//! - Writing a request or environment unchanged changes no byte.
//! - New requests and environments are laid out the way Bruno writes them.
//! - Secret values never reach a file.

use std::path::{Path, PathBuf};

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, Item, KeyValue, MultipartPart, Param, ParamKind,
    PartKind, RawKind, RequestSpec, Settings, Variable,
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

fn request_paths(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Folder(f) => request_paths(&f.items, out),
            Item::Request(r) => out.push(r.path.clone()),
        }
    }
}

fn requests(doc: &CollectionDoc) -> Vec<String> {
    let mut out = Vec::new();
    request_paths(&doc.model().items, &mut out);
    out
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
    out
}

/// Only the format's files: `read_collection` skips non-YAML files and nested collections.
fn owned(set: &FileSet, doc: &CollectionDoc) -> FileSet {
    let written = doc.write();
    FileSet {
        files: set
            .files
            .iter()
            .filter(|(p, _)| written.files.contains_key(*p))
            .map(|(p, b)| (p.clone(), b.clone()))
            .collect(),
    }
}

#[test]
fn unchanged_requests_and_environments_change_no_byte() {
    let mut count = 0;
    for root in all_roots() {
        let input = FileSet::from_dir(&root).unwrap();
        let mut doc = read_collection(&input).unwrap();
        let original = owned(&input, &doc);
        for path in requests(&doc) {
            let s = spec(&doc, &path);
            doc.set_request(&path, &s)
                .unwrap_or_else(|e| panic!("{}: {path}: {e}", root.display()));
            count += 1;
        }
        for env in doc.model().environments.clone() {
            doc.set_environment(&env)
                .unwrap_or_else(|e| panic!("{}: {}: {e}", root.display(), env.name));
        }
        assert_eq!(
            changed(&original, &doc.write()),
            Vec::<String>::new(),
            "{}",
            root.display()
        );
    }
    assert!(count > 150, "only {count} requests");
}

/// A spec that differs from `s` in every field the model has.
fn edited(s: &RequestSpec) -> RequestSpec {
    let mut e = s.clone();
    e.name = format!("{} (edited)", s.name);
    e.seq = Some(s.seq.unwrap_or(0.0) + 10.0);
    e.method = if s.method == "POST" { "PUT" } else { "POST" }.into();
    e.url = format!("{}/edited?x={{{{y}}}}", s.url);
    e.params.push(Param {
        name: "x".into(),
        value: "{{y}}".into(),
        kind: ParamKind::Query,
        enabled: true,
    });
    if let Some(p) = e.params.first_mut() {
        p.enabled = !p.enabled;
    }
    e.headers
        .insert(0, KeyValue::new("X-Edited", "yes: really"));
    if e.headers.len() > 2 {
        e.headers.remove(1);
    }
    e.body = match &s.body {
        Body::Raw { data, .. } => Body::Raw {
            kind: RawKind::Text,
            data: format!("{data}\nmore"),
        },
        Body::FormUrlEncoded { fields } => {
            let mut fields = fields.clone();
            fields.push(KeyValue::new("added", "1"));
            Body::FormUrlEncoded { fields }
        }
        Body::Multipart { parts } => {
            let mut parts = parts.clone();
            parts.push(MultipartPart {
                name: "file".into(),
                kind: PartKind::File,
                values: vec!["a.bin".into(), "b.bin".into()],
                content_type: Some("application/octet-stream".into()),
                enabled: false,
            });
            Body::Multipart { parts }
        }
        Body::File { path, .. } => Body::File {
            path: format!("{path}.new"),
            content_type: Some("text/plain".into()),
        },
        // HTTP items never read as GraphQL.
        Body::GraphQl { .. } => unreachable!("an http item with a GraphQL body"),
        Body::None => Body::Raw {
            kind: RawKind::Json,
            data: "{\n  \"a\": 1\n}".into(),
        },
    };
    e.auth = match &s.auth {
        Auth::Unsupported { .. } => s.auth.clone(),
        Auth::Bearer { token } => Auth::Bearer {
            token: format!("{token}2"),
        },
        Auth::Basic { .. } => Auth::ApiKey {
            key: "k".into(),
            value: "{{v}}".into(),
            placement: ApiKeyPlacement::Query,
        },
        _ => Auth::Basic {
            username: "u".into(),
            password: "{{pw}}".into(),
        },
    };
    e.vars.push(Variable::new("added", "value"));
    e.vars.push(Variable {
        name: "token".into(),
        value: None,
        enabled: true,
        secret: true,
    });
    e.settings = Settings {
        timeout_ms: Some(1500),
        follow_redirects: Some(!s.settings.follow_redirects.unwrap_or(true)),
        max_redirects: None,
        encode_url: s.settings.encode_url,
    };
    e
}

#[test]
fn every_corpus_request_can_be_edited_and_restored() {
    for root in all_roots() {
        let input = FileSet::from_dir(&root).unwrap();
        let base = read_collection(&input).unwrap();
        let original = base.write();
        for path in requests(&base) {
            let ctx = format!("{}: {path}", root.display());
            let mut doc = base.clone();
            let before = spec(&doc, &path);
            let after = edited(&before);
            doc.set_request(&path, &after)
                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
            let written = doc.write();
            assert_eq!(changed(&original, &written), vec![path.clone()], "{ctx}");
            // The file reads back as the edited request, also from scratch.
            let reread = read_collection(&written).unwrap();
            assert_eq!(spec(&reread, &path), after, "{ctx}");
            assert_eq!(spec(&doc, &path), after, "{ctx}: model view");
            // Editing back restores the request.
            doc.set_request(&path, &before)
                .unwrap_or_else(|e| panic!("{ctx}: back: {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(spec(&reread, &path), before, "{ctx}: back");
        }
    }
}

#[test]
fn changing_the_url_rewrites_one_line_and_keeps_descriptions() {
    let input = corpus("request-description-yml");
    let path = "request-with-descriptions.yml";
    let mut doc = read_collection(&input).unwrap();
    let mut s = spec(&doc, path);
    s.url = "https://example.com/api/v2".into();
    doc.set_request(path, &s).unwrap();
    let before = text(&input, path);
    let after = text(&doc.write(), path);
    assert_eq!(
        after,
        before.replace(
            "  url: https://example.com/api\n",
            "  url: https://example.com/api/v2\n"
        )
    );
}

#[test]
fn removing_a_header_keeps_the_others_with_their_descriptions() {
    let input = corpus("request-description-yml");
    let path = "request-with-descriptions.yml";
    let mut doc = read_collection(&input).unwrap();
    let mut s = spec(&doc, path);
    s.headers.remove(0);
    s.headers[0].value = "changed".into();
    s.headers[1].enabled = false;
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(
        after.contains(
            "  headers:\n    - name: X-Multi\n      value: changed\n      description: |-\n        Header line one\n        Header line two\n    - name: X-Plain\n      value: plain-value\n      disabled: true\n  params:\n"
        ),
        "{after}"
    );
    assert!(!after.contains("X-Version"));
}

fn fresh_collection() -> FileSet {
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
        (
            "ping.yml",
            "info:\n  name: ping\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://example.com/ping\n",
        ),
    ])
}

#[test]
fn new_requests_are_laid_out_like_bruno() {
    let mut doc = read_collection(&fresh_collection()).unwrap();
    let spec = RequestSpec {
        name: "Create User".into(),
        method: "POST".into(),
        url: "{{baseUrl}}/users?dry=1".into(),
        params: vec![Param {
            name: "dry".into(),
            value: "1".into(),
            kind: ParamKind::Query,
            enabled: true,
        }],
        headers: vec![
            KeyValue::new("Content-Type", "application/json"),
            KeyValue {
                name: "X-Off".into(),
                value: "".into(),
                enabled: false,
            },
        ],
        body: Body::Raw {
            kind: RawKind::Json,
            data: "{\n  \"name\": \"a\"\n}".into(),
        },
        auth: Auth::Bearer {
            token: "{{token}}".into(),
        },
        vars: vec![Variable::new("id", "7")],
        settings: Settings {
            timeout_ms: Some(0),
            ..Settings::default()
        },
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &spec).unwrap();
    assert_eq!(path, "create-user.yml");
    let out = doc.write();
    assert_eq!(
        text(&out, &path),
        "info:
  name: Create User
  type: http
  seq: 4

http:
  method: POST
  url: \"{{baseUrl}}/users?dry=1\"
  params:
    - name: dry
      value: \"1\"
      type: query
  headers:
    - name: Content-Type
      value: application/json
    - name: X-Off
      value: \"\"
      disabled: true
  body:
    type: json
    data: |-
      {
        \"name\": \"a\"
      }
  auth:
    type: bearer
    token: \"{{token}}\"

runtime:
  variables:
    - name: id
      value: \"7\"

settings:
  timeout: 0
"
    );
    let reread = read_collection(&out).unwrap();
    let got = reread.model().request(&path).unwrap().request.spec.clone();
    assert_eq!(
        got,
        RequestSpec {
            seq: Some(4.0),
            ..spec
        }
    );
}

#[test]
fn new_requests_get_unique_names_and_the_next_seq_in_their_folder() {
    let mut doc = read_collection(&fresh_collection()).unwrap();
    let new = |name: &str| RequestSpec {
        name: name.into(),
        method: "GET".into(),
        auth: Auth::Inherit,
        ..RequestSpec::default()
    };
    let a = doc.add_request("users", &new("Get")).unwrap();
    let b = doc.add_request("users", &new("get")).unwrap();
    let c = doc.add_request("", &new("Ping")).unwrap();
    assert_eq!(
        (a.as_str(), b.as_str(), c.as_str()),
        ("users/get-2.yml", "users/get-3.yml", "ping-2.yml")
    );
    let out = doc.write();
    assert_eq!(
        text(&out, &a),
        "info:\n  name: Get\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"\"\n  auth: inherit\n"
    );
    let reread = read_collection(&out).unwrap();
    let seq = |p: &str| reread.model().request(p).unwrap().request.spec.seq;
    assert_eq!(
        (seq(&a), seq(&b), seq(&c)),
        (Some(3.0), Some(4.0), Some(4.0))
    );
    // Unknown folders are refused.
    assert!(doc.add_request("nope", &new("x")).is_err());
}

#[test]
fn crlf_files_stay_crlf() {
    let input = files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\r\ninfo:\r\n  name: crlf\r\n",
        ),
        (
            "a.yml",
            "info:\r\n  name: a\r\n  type: http\r\n  seq: 1\r\n\r\nhttp:\r\n  method: GET\r\n  url: https://x\r\n",
        ),
    ]);
    let mut doc = read_collection(&input).unwrap();
    let mut s = spec(&doc, "a.yml");
    s.headers.push(KeyValue::new("A", "b"));
    s.body = Body::Raw {
        kind: RawKind::Text,
        data: "one\ntwo".into(),
    };
    doc.set_request("a.yml", &s).unwrap();
    let out = text(&doc.write(), "a.yml");
    assert_eq!(
        out,
        "info:\r\n  name: a\r\n  type: http\r\n  seq: 1\r\n\r\nhttp:\r\n  method: GET\r\n  url: https://x\r\n  headers:\r\n    - name: A\r\n      value: b\r\n  body:\r\n    type: text\r\n    data: |-\r\n      one\r\n      two\r\n"
    );
}

#[test]
fn unsupported_auth_is_kept_but_cannot_be_set() {
    let input = corpus("auth-oauth1-yml");
    let mut doc = read_collection(&input).unwrap();
    let path = requests(&doc)
        .into_iter()
        .find(|p| matches!(spec(&doc, p).auth, Auth::Unsupported { .. }))
        .expect("an OAuth 1 request");
    let mut s = spec(&doc, &path);
    s.url.push_str("/x");
    doc.set_request(&path, &s).unwrap();
    s.auth = Auth::Unsupported {
        kind: "digest".into(),
    };
    assert!(doc.set_request(&path, &s).is_err());
    // The failed edit left the file as it was after the first one.
    assert_eq!(read_collection(&doc.write()).unwrap().model(), doc.model());
}

// ---------------------------------------------------------------------------------------------
// Environments

const SECRET: &str = "s3cr3t-value-that-must-not-leak";

fn all_bytes(set: &FileSet) -> String {
    set.files
        .values()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn new_environment_writes_secrets_without_value() {
    let mut doc = read_collection(&fresh_collection()).unwrap();
    let env = Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "Local".into(),
        variables: vec![
            Variable::new("baseUrl", "http://localhost:8081"),
            Variable {
                name: "token".into(),
                value: Some(SECRET.into()),
                enabled: true,
                secret: true,
            },
            Variable {
                name: "off".into(),
                value: Some("x".into()),
                enabled: false,
                secret: false,
            },
        ],
    };
    let path = doc.set_environment(&env).unwrap();
    assert_eq!(path, "environments/Local.yml");
    let out = doc.write();
    assert_eq!(
        text(&out, &path),
        "name: Local\nvariables:\n  - name: baseUrl\n    value: http://localhost:8081\n  - secret: true\n    name: token\n  - name: off\n    value: x\n    disabled: true\n"
    );
    assert!(!all_bytes(&out).contains(SECRET));
    let reread = read_collection(&out).unwrap();
    let got = reread.model().environment("Local").unwrap();
    assert_eq!(got.variables[1].value, None);
    assert!(got.variables[1].secret);
    assert_eq!(got.variables[0], env.variables[0]);
    // The model view never holds the value either.
    assert!(!format!("{:?}", doc.model()).contains(SECRET));
}

#[test]
fn editing_an_environment_keeps_typed_values_and_descriptions() {
    let input = corpus("variables-tab-variables-tab");
    let mut doc = read_collection(&input).unwrap();
    let mut env = doc.model().environment("Local").unwrap().clone();
    let page = env
        .variables
        .iter_mut()
        .find(|v| v.name == "page_size")
        .unwrap();
    page.value = Some("50".into());
    // Turning a plain variable into a secret drops its value from the file.
    let api = env
        .variables
        .iter_mut()
        .find(|v| v.name == "api_url")
        .unwrap();
    api.secret = true;
    api.value = Some(SECRET.into());
    let path = doc.set_environment(&env).unwrap();
    assert_eq!(path, "environments/Local.yml");
    let out = doc.write();
    assert_eq!(changed(&owned(&input, &doc), &out), vec![path.clone()]);
    let after = text(&out, &path);
    assert!(after.starts_with("name: Local\n\nvariables:\n  - secret: true\n    name: api_url\n  - name: page_size\n    value:\n      type: number\n      data: \"50\"\n"), "{after}");
    assert!(!after.contains(SECRET));
    assert!(!after.contains("https://api.example.com"));
}

#[test]
fn a_secret_value_already_in_a_file_is_removed_on_save() {
    let input = files(&[
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: x\n",
        ),
        (
            "environments/Dev.yml",
            "name: Dev\nvariables:\n  - secret: true\n    name: token\n    value: leaked\n",
        ),
    ]);
    let mut doc = read_collection(&input).unwrap();
    let env = doc.model().environment("Dev").unwrap().clone();
    doc.set_environment(&env).unwrap();
    assert_eq!(
        text(&doc.write(), "environments/Dev.yml"),
        "name: Dev\nvariables:\n  - secret: true\n    name: token\n"
    );
}

#[test]
fn environments_can_be_removed() {
    let input = corpus("environments-environment-inheritance-workspace-collections-yml");
    let mut doc = read_collection(&input).unwrap();
    let names: Vec<String> = doc
        .model()
        .environments
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert!(names.contains(&"qa".to_string()), "{names:?}");
    doc.remove_environment("qa").unwrap();
    let out = doc.write();
    assert!(!out.files.contains_key("environments/qa.yml"));
    assert!(doc.model().environment("qa").is_none());
    assert_eq!(
        read_collection(&out).unwrap().model().environments.len(),
        names.len() - 1
    );
    assert!(doc.remove_environment("qa").is_err());
}

#[test]
fn inline_environments_are_edited_in_place() {
    let input = files(&[(
        COLLECTION_FILE,
        "opencollection: \"1.0.0\"\ninfo:\n  name: x\nconfig:\n  environments:\n    - name: A\n      variables:\n        - name: v\n          value: \"1\"\n    - name: B\n      variables: []\n",
    )]);
    let mut doc = read_collection(&input).unwrap();
    let env = Environment {
        client_certificates: Vec::new(),
        external_secrets: None,
        name: "B".into(),
        variables: vec![Variable::new("w", "2")],
    };
    assert_eq!(doc.set_environment(&env).unwrap(), COLLECTION_FILE);
    assert_eq!(
        text(&doc.write(), COLLECTION_FILE),
        "opencollection: \"1.0.0\"\ninfo:\n  name: x\nconfig:\n  environments:\n    - name: A\n      variables:\n        - name: v\n          value: \"1\"\n    - name: B\n      variables:\n        - name: w\n          value: \"2\"\n"
    );
    doc.remove_environment("A").unwrap();
    assert_eq!(
        text(&doc.write(), COLLECTION_FILE),
        "opencollection: \"1.0.0\"\ninfo:\n  name: x\nconfig:\n  environments:\n    - name: B\n      variables:\n        - name: w\n          value: \"2\"\n"
    );
    let reread = read_collection(&doc.write()).unwrap();
    assert_eq!(reread.model().environments, vec![env]);
}

#[test]
fn request_secret_variables_have_no_value_in_the_file() {
    let mut doc = read_collection(&fresh_collection()).unwrap();
    let mut s = spec(&doc, "ping.yml");
    s.vars.push(Variable {
        name: "pw".into(),
        value: Some(SECRET.into()),
        enabled: true,
        secret: true,
    });
    doc.set_request("ping.yml", &s).unwrap();
    let out = doc.write();
    assert!(!all_bytes(&out).contains(SECRET));
}
