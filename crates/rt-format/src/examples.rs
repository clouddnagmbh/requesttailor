// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved examples and documentation of a collection's items, read only (wave 8, strand F).
//!
//! `rt mock` answers with the `examples` of HTTP requests and `rt docs` renders them together
//! with `docs` and `info.description`. Neither is part of `rt_core::model::RequestSpec`: the
//! model stays what sending needs, and these fields stay in the files untouched (writing a
//! document never changes them, so round trips stay byte-identical).
//!
//! OpenCollection `HttpRequestExample`:
//!
//! ```yaml
//! examples:
//!   - name: Found
//!     description: One user          # string or {content, type}
//!     request:                       # all optional
//!       url: "{{baseUrl}}/users/42"
//!       method: GET
//!       headers: [{name, value, disabled}]
//!       params: [{name, value, type: query|path, disabled}]
//!       body: {type: json, data: ...}  # as http.body
//!     response:
//!       status: 200                  # missing: 200
//!       statusText: OK
//!       headers: [{name, value}]
//!       body: {type: json|text|xml|html|binary, data: ...}
//! ```

use std::collections::BTreeMap;

use rt_core::model::{Body, KeyValue, Param};

use crate::read::{self, Builder};
use crate::yaml::Node;
use crate::{COLLECTION_FILE, CollectionDoc, FOLDER_FILE, FileSet, FormatError, Source};

/// The examples saved with one HTTP request.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestExamples {
    /// The request file, relative to the collection root (as `RequestEntry::path`).
    pub path: String,
    /// In file order. Never empty in [`examples`].
    pub examples: Vec<Example>,
}

/// One saved request/response pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    /// `name`; empty if the file has none.
    pub name: String,
    pub description: Option<String>,
    pub request: ExampleRequest,
    pub response: ExampleResponse,
}

/// The request an example was recorded with. Every part is optional in the file; missing
/// parts are `None` or empty.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExampleRequest {
    pub url: Option<String>,
    pub method: Option<String>,
    pub headers: Vec<KeyValue>,
    pub params: Vec<Param>,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExampleResponse {
    /// `status`; 200 when the file has none (or not a valid status code).
    pub status: u16,
    pub status_text: Option<String>,
    /// `(name, value)` in file order.
    pub headers: Vec<(String, String)>,
    pub body: Option<ExampleBody>,
}

impl Default for ExampleResponse {
    fn default() -> Self {
        Self {
            status: 200,
            status_text: None,
            headers: Vec::new(),
            body: None,
        }
    }
}

/// A response body as saved: `kind` is `json`, `text`, `xml`, `html` or `binary` (or what
/// the file says), `data` the text as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleBody {
    pub kind: String,
    pub data: String,
}

/// Documentation of the collection, a folder or a request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemDocs {
    /// `info.description` (requests and folders; the collection's `info.description`).
    pub description: Option<String>,
    /// `docs`: a string or the `content` of `{content, type}`.
    pub docs: Option<String>,
}

/// The examples of every HTTP request of the collection in `files` that has at least one,
/// ordered by request path.
pub fn examples(files: &FileSet) -> Result<Vec<RequestExamples>, FormatError> {
    Ok(crate::read_collection(files)?.examples())
}

impl CollectionDoc {
    /// The examples of every HTTP request that has at least one, ordered by request path.
    /// Requests of nested collections are not included (as in the model).
    pub fn examples(&self) -> Vec<RequestExamples> {
        let mut out = Vec::new();
        for (path, source) in &self.files {
            let Source::Yaml(doc) = source else { continue };
            if !is_item(path) || !is_http(&doc.root) {
                continue;
            }
            let Some(list) = doc.root.get("examples").filter(|l| l.is_seq()) else {
                continue;
            };
            let examples: Vec<Example> = list
                .items()
                .filter(|e| e.is_map())
                .map(|e| example(path, e))
                .collect();
            if !examples.is_empty() {
                out.push(RequestExamples {
                    path: path.clone(),
                    examples,
                });
            }
        }
        out
    }

    /// The documentation of the item at `path`: `""` for the collection, a folder path as in
    /// `Folder::path`, or a request path as in `RequestEntry::path`. Empty if there is none
    /// (or no such item).
    pub fn item_docs(&self, path: &str) -> ItemDocs {
        let file = if path.is_empty() {
            COLLECTION_FILE.to_owned()
        } else if path.ends_with(".yml") {
            path.to_owned()
        } else {
            format!("{path}/{FOLDER_FILE}")
        };
        let Some(Source::Yaml(doc)) = self.files.get(&file) else {
            return ItemDocs::default();
        };
        let root = &doc.root;
        ItemDocs {
            description: description(root.get("info").and_then(|i| i.get("description"))),
            docs: description(root.get("docs")),
        }
    }
}

/// A request file: not the collection or a folder file, not an environment.
fn is_item(path: &str) -> bool {
    path != COLLECTION_FILE
        && path.rsplit('/').next() != Some(FOLDER_FILE)
        && !path.starts_with(&format!("{}/", read::ENVIRONMENTS_DIR))
}

fn is_http(root: &Node) -> bool {
    match root
        .get("info")
        .and_then(|i| i.get("type"))
        .and_then(Node::text)
    {
        Some(kind) => kind == "http",
        None => root.get("http").is_some(),
    }
}

/// A `Description` or `Documentation`: a string, `{content, type}` or null.
fn description(node: Option<&Node>) -> Option<String> {
    let node = node.filter(|n| !n.is_null())?;
    let text = if node.is_map() {
        read::text(node.get("content"))
    } else {
        node.text()?.to_owned()
    };
    (!text.trim().is_empty()).then_some(text)
}

fn example(path: &str, e: &Node) -> Example {
    let request = e.get("request").filter(|r| r.is_map());
    let response = e.get("response").filter(|r| r.is_map());
    let opt = |n: Option<&Node>, k: &str| {
        n.and_then(|n| n.get(k))
            .and_then(Node::text)
            .map(str::to_owned)
    };
    let docs = BTreeMap::new();
    let mut builder = Builder::new(&docs);
    Example {
        name: read::text(e.get("name")),
        description: description(e.get("description")),
        request: ExampleRequest {
            url: opt(request, "url"),
            method: opt(request, "method"),
            headers: read::key_values(request.and_then(|r| r.get("headers"))),
            params: read::params(request.and_then(|r| r.get("params"))),
            body: builder.body(path, request.and_then(|r| r.get("body"))),
        },
        response: ExampleResponse {
            status: response
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_f64().or_else(|| s.text()?.trim().parse().ok()))
                .filter(|s| s.fract() == 0.0 && (100.0..=999.0).contains(s))
                .map_or(200, |s| s as u16),
            status_text: opt(response, "statusText"),
            headers: response
                .and_then(|r| r.get("headers"))
                .map(|l| {
                    l.items()
                        .map(|h| (read::text(h.get("name")), read::text(h.get("value"))))
                        .collect()
                })
                .unwrap_or_default(),
            body: response
                .and_then(|r| r.get("body"))
                .filter(|b| b.is_map())
                .map(|b| ExampleBody {
                    kind: b
                        .get("type")
                        .and_then(Node::text)
                        .unwrap_or("text")
                        .to_owned(),
                    data: read::text(b.get("data")),
                }),
        },
    }
}

#[cfg(test)]
mod tests {
    use rt_core::model::{ParamKind, RawKind};

    use super::*;

    fn files(list: &[(&str, &str)]) -> FileSet {
        FileSet {
            files: list
                .iter()
                .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
                .collect(),
        }
    }

    const COLLECTION: &str =
        "opencollection: 1.0.0\ninfo:\n  name: Shop\ndocs: |-\n  The shop API.\n";

    const GET_USER: &str = r#"info:
  name: Get user
  type: http
  description: Reads one user.

http:
  method: GET
  url: "{{baseUrl}}/users/:id"

docs: Returns one user by id.

examples:
  - name: Found
    description:
      content: The user exists.
      type: text/markdown
    request:
      url: "{{baseUrl}}/users/42?expand=roles"
      method: GET
      headers:
        - name: Accept
          value: application/json
        - name: X-Debug
          value: "1"
          disabled: true
      params:
        - name: expand
          value: roles
          type: query
        - name: id
          value: "42"
          type: path
    response:
      status: 200
      statusText: OK
      headers:
        - name: Content-Type
          value: application/json
      body:
        type: json
        data: |-
          {"id": 42}
  - name: Missing
    response:
      status: 404
"#;

    const CREATE_USER: &str = r#"info:
  name: Create user
  type: http
http:
  method: POST
  url: "{{baseUrl}}/users"
examples:
  - name: Created
    request:
      body:
        type: json
        data: '{"name": "Ada"}'
    response:
      status: "201"
      body:
        type: text
        data: created
"#;

    const NO_EXAMPLES: &str =
        "info:\n  name: Health\n  type: http\nhttp:\n  method: GET\n  url: /health\n";

    const GRPC: &str = "info:\n  name: Stream\n  type: grpc\ngrpc:\n  url: localhost:50051\n  method: /a.B/C\nexamples:\n  - name: not http\n";

    fn fixture() -> FileSet {
        files(&[
            ("opencollection.yml", COLLECTION),
            (
                "users/folder.yml",
                "info:\n  name: Users\n  description: Everything about users.\n",
            ),
            ("users/get-user.yml", GET_USER),
            ("users/create-user.yml", CREATE_USER),
            ("health.yml", NO_EXAMPLES),
            ("stream.yml", GRPC),
            (
                "nested/opencollection.yml",
                "opencollection: 1.0.0\ninfo:\n  name: Other\n",
            ),
            ("nested/item.yml", CREATE_USER),
        ])
    }

    #[test]
    fn reads_the_examples_of_http_requests() {
        let all = examples(&fixture()).unwrap();
        let paths: Vec<&str> = all.iter().map(|r| r.path.as_str()).collect();
        // Requests without examples, other protocols and nested collections are left out.
        assert_eq!(paths, ["users/create-user.yml", "users/get-user.yml"]);

        let get = &all[1].examples;
        assert_eq!(get.len(), 2);
        let found = &get[0];
        assert_eq!(found.name, "Found");
        assert_eq!(found.description.as_deref(), Some("The user exists."));
        assert_eq!(
            found.request.url.as_deref(),
            Some("{{baseUrl}}/users/42?expand=roles")
        );
        assert_eq!(found.request.method.as_deref(), Some("GET"));
        assert_eq!(found.request.headers.len(), 2);
        assert!(!found.request.headers[1].enabled);
        assert_eq!(found.request.params[1].kind, ParamKind::Path);
        assert_eq!(found.response.status, 200);
        assert_eq!(found.response.status_text.as_deref(), Some("OK"));
        assert_eq!(
            found.response.headers,
            [("Content-Type".to_owned(), "application/json".to_owned())]
        );
        assert_eq!(
            found.response.body,
            Some(ExampleBody {
                kind: "json".into(),
                data: "{\"id\": 42}".into()
            })
        );

        let missing = &get[1];
        assert_eq!(missing.request, ExampleRequest::default());
        assert_eq!(missing.response.status, 404);
        assert_eq!(missing.response.body, None);

        let created = &all[0].examples[0];
        assert_eq!(
            created.request.body,
            Body::Raw {
                kind: RawKind::Json,
                data: "{\"name\": \"Ada\"}".into()
            }
        );
        // A quoted status is still a status.
        assert_eq!(created.response.status, 201);
    }

    #[test]
    fn reads_docs_and_descriptions() {
        let doc = crate::read_collection(&fixture()).unwrap();
        assert_eq!(
            doc.item_docs(""),
            ItemDocs {
                description: None,
                docs: Some("The shop API.".into())
            }
        );
        assert_eq!(
            doc.item_docs("users").description.as_deref(),
            Some("Everything about users.")
        );
        let get = doc.item_docs("users/get-user.yml");
        assert_eq!(get.description.as_deref(), Some("Reads one user."));
        assert_eq!(get.docs.as_deref(), Some("Returns one user by id."));
        assert_eq!(doc.item_docs("health.yml"), ItemDocs::default());
        assert_eq!(doc.item_docs("nope.yml"), ItemDocs::default());
    }

    #[test]
    fn reading_examples_leaves_the_files_unchanged() {
        let files = fixture();
        let doc = crate::read_collection(&files).unwrap();
        let _ = doc.examples();
        let _ = doc.item_docs("users/get-user.yml");
        let out = doc.write();
        for (path, bytes) in &out.files {
            assert_eq!(bytes, &files.files[path], "{path}");
        }
    }

    #[test]
    fn imported_postman_examples_are_read() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../testdata/import/postman/synthetic-v21-features.bundle.expected/collection",
        );
        let all = examples(&FileSet::from_dir(&dir).unwrap()).unwrap();
        let get = all
            .iter()
            .find(|r| r.path == "users/get-user.yml")
            .expect("get-user has examples");
        assert_eq!(get.examples[0].name, "Found");
        assert_eq!(get.examples[0].response.status, 200);
        assert!(
            get.examples[0]
                .response
                .body
                .as_ref()
                .unwrap()
                .data
                .contains("\"Ada\"")
        );
    }
}
