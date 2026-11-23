// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run wizard and the response check in the app: the workspace's request (unsaved edits,
//! inherited auth) and the variables of the chosen environment, the spec found or configured,
//! parsed once per collection, and a stored response against the spec.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use rt_app::contract::{InputPlace, ProblemKind, SpecCache, requirements, validate_response};
use rt_app::{AppError, ResponseStore, SendInput, Workspace};
use rt_core::model::{Param, ParamKind};
use rt_core::send::{ReceivedResponse, ReportBody, SendReport};
use rt_core::transport::{HttpVersion, Timing};
use rt_format::FileSet;
use serde_json::json;

fn spec_bytes(title: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "openapi": "3.0.3",
        "info": { "title": title, "version": "1.0.0" },
        "servers": [{ "url": "https://{region}.example.com/v1",
                      "variables": { "region": { "default": "eu", "enum": ["eu", "us"] } } }],
        "components": { "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } } },
        "paths": {
            "/pets/{petId}": {
                "get": {
                    "summary": "Info for a specific pet",
                    "parameters": [{ "name": "petId", "in": "path", "required": true,
                                     "schema": { "type": "integer", "minimum": 1 } }],
                    "responses": { "200": { "description": "ok", "content": { "application/json": {
                        "schema": { "type": "object", "required": ["id"],
                                    "properties": { "id": { "type": "integer" } } } } } } }
                }
            }
        }
    }))
    .unwrap()
}

fn collection(configured: bool) -> FileSet {
    let mut files = FileSet::default();
    let head = "opencollection: 1.0.0\ninfo:\n  name: Pets\n";
    let config = if configured {
        "extensions:\n  requesttailor:\n    contract:\n      spec: api/pets.json\n"
    } else {
        ""
    };
    let mut add = |path: &str, text: &str| {
        files
            .files
            .insert(path.to_owned(), text.as_bytes().to_vec());
    };
    add("opencollection.yml", &format!("{head}{config}"));
    add(
        "environments/Dev.yml",
        "name: Dev\nvariables:\n  - name: region\n    value: \"\"\n  - name: token\n    value: \"\"\n    enabled: true\n    secret: true\n",
    );
    add(
        "pets/folder.yml",
        "info:\n  name: pets\n  type: folder\n  seq: 1\n\nrequest:\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
    );
    add(
        "pets/get-pet.yml",
        "info:\n  name: Get pet\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/pets/:petId\"\n  headers:\n    - name: X-Region\n      value: \"{{region}}\"\n  params:\n    - name: petId\n      value: \"\"\n      type: path\n  auth: inherit\n",
    );
    files
}

/// Reads spec files from a map, like a shell reads its work tree.
struct Disk {
    files: RefCell<HashMap<String, Vec<u8>>>,
    reads: RefCell<u32>,
}

impl Disk {
    fn new(path: &str, bytes: Vec<u8>) -> Self {
        Self {
            files: RefCell::new(HashMap::from([(path.to_owned(), bytes)])),
            reads: RefCell::new(0),
        }
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, AppError> {
        *self.reads.borrow_mut() += 1;
        Ok(self.files.borrow().get(path).cloned())
    }
}

fn dev(secrets: &[(&str, &str)], vars: &[(&str, &str)]) -> SendInput {
    let pairs = |p: &[(&str, &str)]| {
        p.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    };
    SendInput {
        environment: Some("Dev".into()),
        vars: pairs(vars),
        secrets: pairs(secrets),
    }
}

const GET_PET: &str = "pets/get-pet.yml";

#[test]
fn variables_of_the_environment_and_inherited_auth() {
    let ws = Workspace::open(collection(true)).unwrap();
    let disk = Disk::new("api/pets.json", spec_bytes("Pets"));
    let mut cache = SpecCache::new();
    let read = |p: &str| disk.read(p);
    let req = requirements(&ws, GET_PET, &dev(&[], &[]), &mut cache, &read)
        .unwrap()
        .expect("the operation matches");
    assert_eq!(req.spec, "api/pets.json");
    assert_eq!(req.operation, "GET /pets/{petId}");
    assert!(!req.ready);
    let ids: Vec<&str> = req.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "variable:baseUrl",
            "variable:region",
            "variable:token",
            "path:petId"
        ]
    );
    let msg = |id: &str| {
        req.fields
            .iter()
            .find(|f| f.id == id)
            .and_then(|f| f.problem.as_ref())
            .map(|p| (p.kind, p.message.as_str()))
    };
    assert_eq!(
        msg("variable:baseUrl"),
        Some((ProblemKind::Missing, "baseUrl has no value"))
    );
    assert_eq!(
        msg("variable:region"),
        Some((ProblemKind::Missing, "region is empty"))
    );
    assert_eq!(
        msg("variable:token"),
        Some((ProblemKind::Missing, "token has no value"))
    );
    assert_eq!(
        msg("path:petId"),
        Some((ProblemKind::Missing, "petId is required"))
    );
    let token = req
        .fields
        .iter()
        .find(|f| f.id == "variable:token")
        .unwrap();
    assert_eq!(token.place, InputPlace::Variable);
    assert!(token.secret, "declared secret in the environment");
    assert!(
        token.auth,
        "used by the bearer auth inherited from the folder"
    );
    assert_eq!(token.value, None);
    let region = req
        .fields
        .iter()
        .find(|f| f.id == "variable:region")
        .unwrap();
    assert!(!region.secret && !region.auth);
    assert_eq!(region.value.as_deref(), Some(""));
}

#[test]
fn a_request_with_everything_set_is_ready_and_secrets_stay_out() {
    let mut ws = Workspace::open(collection(true)).unwrap();
    // Unsaved edit: the path parameter now has a value.
    let mut spec = ws.request(GET_PET).unwrap();
    spec.params = vec![Param {
        name: "petId".into(),
        value: "0".into(),
        kind: ParamKind::Path,
        enabled: true,
    }];
    ws.update_request(GET_PET, spec.clone()).unwrap();
    let disk = Disk::new("api/pets.json", spec_bytes("Pets"));
    let mut cache = SpecCache::new();
    let read = |p: &str| disk.read(p);
    let input = dev(
        &[("token", "s3cr3t-value")],
        &[("baseUrl", "https://eu.example.com/v1"), ("region", "eu")],
    );
    let req = requirements(&ws, GET_PET, &input, &mut cache, &read)
        .unwrap()
        .unwrap();
    assert_eq!(req.fields[0].id, "path:petId");
    assert_eq!(
        req.fields[0].problem.as_ref().unwrap().message,
        "must be at least 1"
    );
    let text = serde_json::to_string(&req).unwrap();
    assert!(!text.contains("s3cr3t-value"), "{text}");

    spec.params[0].value = "7".into();
    ws.update_request(GET_PET, spec).unwrap();
    let req = requirements(&ws, GET_PET, &input, &mut cache, &read)
        .unwrap()
        .unwrap();
    assert!(req.ready, "{:#?}", req.fields);

    // An empty secret value is not a value.
    let input = dev(
        &[("token", "")],
        &[("baseUrl", "https://eu.example.com/v1"), ("region", "eu")],
    );
    let req = requirements(&ws, GET_PET, &input, &mut cache, &read)
        .unwrap()
        .unwrap();
    let ids: Vec<&str> = req.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["variable:token", "path:petId"]);
}

#[test]
fn the_spec_is_found_or_absent() {
    let ws = Workspace::open(collection(false)).unwrap();
    let mut cache = SpecCache::new();
    let disk = Disk::new("/openapi.json", spec_bytes("Pets"));
    let read = |p: &str| disk.read(p);
    let req = requirements(&ws, GET_PET, &dev(&[], &[]), &mut cache, &read)
        .unwrap()
        .unwrap();
    assert_eq!(req.spec, "/openapi.json");

    let none = Disk::new("elsewhere.json", spec_bytes("Pets"));
    let read = |p: &str| none.read(p);
    assert_eq!(
        requirements(&ws, GET_PET, &dev(&[], &[]), &mut cache, &read).unwrap(),
        None
    );
    assert!(matches!(
        requirements(&ws, "nope.yml", &dev(&[], &[]), &mut cache, &read),
        Err(AppError::RequestNotFound(_))
    ));
}

#[test]
fn the_cache_parses_once_per_version_of_the_spec() {
    let files = collection(true);
    let disk = Disk::new("api/pets.json", spec_bytes("Pets"));
    let read = |p: &str| disk.read(p);
    let mut cache = SpecCache::new();
    let (path, a) = cache.get(&files, &read).unwrap().unwrap();
    assert_eq!(path, "api/pets.json");
    let (_, b) = cache.get(&files, &read).unwrap().unwrap();
    assert!(Arc::ptr_eq(&a, &b), "unchanged bytes: the same parse");
    disk.files
        .borrow_mut()
        .insert("api/pets.json".into(), spec_bytes("Pets 2"));
    let (_, c) = cache.get(&files, &read).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&a, &c), "changed bytes: parsed again");
    assert_eq!(c.title(), "Pets 2");
}

fn stored(store: &mut ResponseStore, status: u16, body: &str) -> u64 {
    let report = SendReport {
        tests: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        var_changes: Vec::new(),
        flow: None,
        notes: Vec::new(),
        version: 1,
        request_path: GET_PET.into(),
        environment: Some("Dev".into()),
        request: None,
        response: Some(ReceivedResponse {
            status,
            version: HttpVersion::Http11,
            headers: vec![("Content-Type".into(), "application/json".into())],
            url: "https://eu.example.com/v1/pets/7".into(),
            redirects: vec![],
            body: ReportBody::Utf8(body.to_owned()),
            size: body.len() as u64,
            truncated: false,
            timing: Timing {
                dns: None,
                connect: None,
                tls: None,
                ttfb: std::time::Duration::ZERO,
            },
            total: std::time::Duration::ZERO,
        }),
        variables: vec![],
        unresolved: vec![],
        error: None,
    };
    store.insert(report).response_id.unwrap()
}

#[test]
fn a_stored_response_against_the_spec() {
    let ws = Workspace::open(collection(true)).unwrap();
    let disk = Disk::new("api/pets.json", spec_bytes("Pets"));
    let read = |p: &str| disk.read(p);
    let mut cache = SpecCache::new();
    let mut store = ResponseStore::new();

    let ok = stored(&mut store, 200, r#"{"id": 7}"#);
    let c = validate_response(&ws, GET_PET, &store, ok, &mut cache, &read)
        .unwrap()
        .unwrap();
    assert_eq!(c.matched.as_deref(), Some("200"));
    assert!(c.differences.is_empty(), "{:?}", c.differences);

    let bad = stored(&mut store, 200, r#"{"id": "seven"}"#);
    let c = validate_response(&ws, GET_PET, &store, bad, &mut cache, &read)
        .unwrap()
        .unwrap();
    assert_eq!(c.differences.len(), 1);
    assert_eq!(c.differences[0].path, "$.id");
    assert_eq!(c.differences[0].expected, "integer");
    assert_eq!(c.differences[0].got, "string \"seven\"");

    let gone = stored(&mut store, 500, "");
    let c = validate_response(&ws, GET_PET, &store, gone, &mut cache, &read)
        .unwrap()
        .unwrap();
    assert_eq!(c.matched, None);
    assert_eq!(c.differences[0].expected, "200");

    assert!(matches!(
        validate_response(&ws, GET_PET, &store, 999, &mut cache, &read),
        Err(AppError::ResponseNotFound(999))
    ));
    // The spec was read for every call (and parsed once).
    assert!(*disk.reads.borrow() >= 3);
}

#[test]
fn the_configured_spec_path_for_shells_that_read_first() {
    let named = Workspace::open(collection(true)).unwrap();
    assert_eq!(
        rt_app::contract::configured_spec(&named)
            .unwrap()
            .as_deref(),
        Some("api/pets.json")
    );
    let unnamed = Workspace::open(collection(false)).unwrap();
    assert_eq!(rt_app::contract::configured_spec(&unnamed).unwrap(), None);
}
