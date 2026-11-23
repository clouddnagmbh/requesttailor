// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run wizard ("Ready to send?") and the response check: a request against its spec
//! operation before sending (missing and invalid inputs, variables) and its response against
//! the declared responses. Specs are synthetic, built here.

use rt_core::model::{Body, KeyValue, Param, ParamKind, Protocol, RawKind, RequestSpec};
use rt_format::contract::{
    DifferencePlace, InputField, InputPlace, MAX_CHECKED_BODY, ParsedSpec, ProblemKind,
    RequestFacts, ResponseCheck, ResponseFacts, SendRequirements, VarFact, VarState, requirements,
    validate_response,
};
use serde_json::{Value, json};

fn pets() -> ParsedSpec {
    let spec = json!({
        "openapi": "3.0.3",
        "info": { "title": "Pets", "version": "1.0.0" },
        "servers": [{
            "url": "https://{region}.example.com/v1",
            "description": "Production",
            "variables": { "region": {
                "default": "eu", "enum": ["eu", "us"], "description": "Region of the data centre"
            } }
        }],
        "components": {
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer" },
                "key": { "type": "apiKey", "in": "header", "name": "X-Api-Key" }
            },
            "schemas": {
                "Pet": {
                    "type": "object",
                    "required": ["id", "name"],
                    "properties": {
                        "id": { "type": "integer", "minimum": 1 },
                        "name": { "type": "string", "maxLength": 10 },
                        "status": { "$ref": "#/components/schemas/Status" },
                        "tags": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
                        "parent": { "$ref": "#/components/schemas/Pet" }
                    }
                },
                "Status": { "type": "string", "enum": ["available", "pending", "sold"] },
                "NewPet": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string", "minLength": 1 },
                        "status": { "$ref": "#/components/schemas/Status" }
                    }
                }
            }
        },
        "paths": {
            "/pets": {
                "get": {
                    "summary": "List pets",
                    "parameters": [
                        { "name": "limit", "in": "query", "required": true, "example": 20,
                          "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                        { "name": "status", "in": "query", "schema": { "$ref": "#/components/schemas/Status" } },
                        { "name": "since", "in": "query", "schema": { "type": "string", "format": "date" } },
                        { "name": "ids", "in": "query", "schema": { "type": "array", "items": { "type": "integer" } } },
                        { "name": "X-Request-Id", "in": "header", "required": true,
                          "schema": { "type": "string", "format": "uuid" } },
                        { "name": "X-Api-Key", "in": "header", "schema": { "type": "string" } },
                        { "name": "Accept", "in": "header", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": {
                            "description": "ok",
                            "headers": { "X-Total": { "required": true, "schema": { "type": "integer" } } },
                            "content": { "application/json": {
                                "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Pet" } }
                            } }
                        },
                        "default": {
                            "description": "error",
                            "content": { "application/json": { "schema": {
                                "type": "object", "required": ["message"],
                                "properties": { "message": { "type": "string" } }
                            } } }
                        }
                    }
                },
                "post": {
                    "summary": "Create a pet",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/NewPet" } } }
                    },
                    "responses": {
                        "201": { "description": "created", "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/Pet" } } } },
                        "4XX": { "description": "client error" }
                    }
                }
            },
            "/pets/{petId}": {
                "get": {
                    "summary": "Info for a specific pet",
                    "deprecated": true,
                    "parameters": [{
                        "name": "petId", "in": "path", "required": true,
                        "description": "The id of the pet",
                        "schema": { "type": "string", "pattern": "^[a-z]+$" }
                    }],
                    "responses": {
                        "200": { "description": "ok", "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/Pet" } } } },
                        "404": { "description": "not found" }
                    }
                }
            },
            "/owners/{ownerId}/since/{day}": {
                "get": {
                    "parameters": [
                        { "name": "ownerId", "in": "path", "required": true,
                          "schema": { "type": "string", "format": "uuid" } },
                        { "name": "day", "in": "path", "required": true,
                          "schema": { "type": "string", "format": "date" } }
                    ],
                    "responses": { "200": { "description": "ok" } }
                }
            },
            "/forms": {
                "post": {
                    "requestBody": { "content": { "application/x-www-form-urlencoded": { "schema": {
                        "type": "object", "required": ["a", "b"],
                        "properties": { "a": { "type": "string" }, "b": { "type": "string" } }
                    } } } },
                    "responses": { "204": { "description": "ok" } }
                }
            }
        }
    });
    ParsedSpec::parse(&serde_json::to_vec(&spec).unwrap()).unwrap()
}

fn request(method: &str, url: &str) -> RequestSpec {
    RequestSpec {
        name: "R".into(),
        method: method.into(),
        url: url.into(),
        ..RequestSpec::default()
    }
}

fn path_param(name: &str, value: &str) -> Param {
    Param {
        name: name.into(),
        value: value.into(),
        kind: ParamKind::Path,
        enabled: true,
    }
}

fn json_body(text: &str) -> Body {
    Body::Raw {
        kind: RawKind::Json,
        data: text.into(),
    }
}

fn reqs(spec: &ParsedSpec, request: &RequestSpec, vars: &[VarFact]) -> SendRequirements {
    requirements(
        spec,
        &RequestFacts {
            request,
            variables: vars,
        },
    )
    .expect("an operation matches")
}

fn field<'a>(r: &'a SendRequirements, id: &str) -> &'a InputField {
    r.fields
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| panic!("no field {id} in {:#?}", r.fields))
}

fn message(r: &SendRequirements, id: &str) -> Option<String> {
    field(r, id).problem.as_ref().map(|p| p.message.clone())
}

/// A valid list request: the base for the query and header cases.
fn list(query: &str) -> RequestSpec {
    let mut r = request("GET", &format!("{{{{baseUrl}}}}/pets?{query}"));
    r.headers = vec![KeyValue::new(
        "X-Request-Id",
        "3fa85f64-5717-4562-b3fc-2c963f66afa6",
    )];
    r
}

// ---- before sending ----------------------------------------------------------------------

#[test]
fn a_path_parameter_without_value_is_missing() {
    let spec = pets();
    let mut r = request("GET", "{{baseUrl}}/pets/:petId");
    r.params = vec![path_param("petId", " ")];
    let req = reqs(&spec, &r, &[]);
    assert_eq!(req.operation, "GET /pets/{petId}");
    assert_eq!(req.summary.as_deref(), Some("Info for a specific pet"));
    assert!(req.deprecated);
    assert!(!req.ready);
    let f = field(&req, "path:petId");
    assert_eq!(f.place, InputPlace::Path);
    assert!(f.required);
    assert_eq!(f.problem.as_ref().unwrap().kind, ProblemKind::Missing);
    assert_eq!(message(&req, "path:petId").unwrap(), "petId is required");
    assert_eq!(f.description.as_deref(), Some("The id of the pet"));
    assert_eq!(
        f.schema,
        Some(json!({ "type": "string", "pattern": "^[a-z]+$" }))
    );

    // No `:petId` parameter at all: the same.
    r.params.clear();
    assert_eq!(
        message(&reqs(&spec, &r, &[]), "path:petId").unwrap(),
        "petId is required"
    );
}

#[test]
fn path_values_are_checked_strictly() {
    let spec = pets();
    let mut r = request("GET", "{{baseUrl}}/pets/:petId");
    r.params = vec![path_param("petId", "Rex1")];
    let req = reqs(&spec, &r, &[]);
    let p = field(&req, "path:petId").problem.as_ref().unwrap();
    assert_eq!(p.kind, ProblemKind::Invalid);
    assert_eq!(p.message, "must match ^[a-z]+$");

    r.params = vec![path_param("petId", "rex")];
    let req = reqs(&spec, &r, &[]);
    assert!(req.ready, "{:#?}", req.fields);
    assert_eq!(field(&req, "path:petId").value.as_deref(), Some("rex"));

    // Literal segments are the values.
    let r = request("GET", "{{baseUrl}}/owners/not-a-uuid/since/2024-02-30");
    let req = reqs(&spec, &r, &[]);
    assert_eq!(message(&req, "path:ownerId").unwrap(), "must be a UUID");
    assert_eq!(
        message(&req, "path:day").unwrap(),
        "must be a date (YYYY-MM-DD)"
    );
}

#[test]
fn templated_values_are_not_judged() {
    let spec = pets();
    let mut r = request("GET", "{{baseUrl}}/pets/:petId");
    r.params = vec![path_param("petId", "{{id}}")];
    assert!(field(&reqs(&spec, &r, &[]), "path:petId").problem.is_none());

    let r = request("GET", "{{baseUrl}}/pets/{{id}}");
    assert!(field(&reqs(&spec, &r, &[]), "path:petId").problem.is_none());

    let mut r = list("limit={{limit}}&status={{s}}");
    r.headers = vec![KeyValue::new("X-Request-Id", "{{rid}}")];
    let req = reqs(&spec, &r, &[]);
    assert!(req.ready, "{:#?}", req.fields);

    let mut r = request("POST", "{{baseUrl}}/pets");
    r.body = json_body(r#"{"name": {{name}}}"#);
    assert!(field(&reqs(&spec, &r, &[]), "body").problem.is_none());
    r.body = json_body(r#"{"name": "{{name}}", "status": "{{status}}"}"#);
    assert!(field(&reqs(&spec, &r, &[]), "body").problem.is_none());
}

#[test]
fn query_values_enum_range_type_and_format() {
    let spec = pets();
    let req = reqs(&spec, &list("limit=20&status=lost"), &[]);
    assert_eq!(
        message(&req, "query:status").unwrap(),
        "must be one of available, pending, sold"
    );
    assert_eq!(field(&req, "query:status").value.as_deref(), Some("lost"));
    assert!(field(&req, "query:limit").problem.is_none());
    // The enum of the referenced schema is inlined for the form.
    assert_eq!(
        field(&req, "query:status").schema,
        Some(json!({ "type": "string", "enum": ["available", "pending", "sold"] }))
    );
    assert_eq!(field(&req, "query:limit").example.as_deref(), Some("20"));
    assert_eq!(
        field(&req, "query:status").example.as_deref(),
        Some("available")
    );

    let cases = [
        ("limit=0", "must be at least 1"),
        ("limit=500", "must be at most 100"),
        ("limit=abc", "must be an integer"),
        ("limit=1.5", "must be an integer"),
    ];
    for (query, want) in cases {
        let req = reqs(&spec, &list(query), &[]);
        assert_eq!(message(&req, "query:limit").unwrap(), want, "{query}");
    }
    let req = reqs(&spec, &list("limit=5&since=2024-13-01&ids=1,x"), &[]);
    assert_eq!(
        message(&req, "query:since").unwrap(),
        "must be a date (YYYY-MM-DD)"
    );
    assert_eq!(
        message(&req, "query:ids").unwrap(),
        "$[1]: must be an integer"
    );
    let req = reqs(&spec, &list("limit=5&since=2024-02-29&ids=1&ids=3"), &[]);
    assert!(req.ready, "{:#?}", req.fields);
}

#[test]
fn required_query_and_header_parameters() {
    let spec = pets();
    let r = request("GET", "{{baseUrl}}/pets");
    let req = reqs(&spec, &r, &[]);
    assert_eq!(message(&req, "query:limit").unwrap(), "limit is required");
    assert_eq!(
        message(&req, "header:X-Request-Id").unwrap(),
        "X-Request-Id is required"
    );
    // Optional ones are listed, without a problem; Accept and API keys are not asked.
    assert!(field(&req, "query:status").problem.is_none());
    assert!(!req.fields.iter().any(|f| f.id == "header:Accept"));
    assert!(!req.fields.iter().any(|f| f.id == "header:X-Api-Key"));

    let mut r = list("limit=");
    r.headers.push(KeyValue {
        name: "x-request-id".into(),
        value: "nope".into(),
        enabled: false,
    });
    r.headers.remove(0);
    let req = reqs(&spec, &r, &[]);
    assert_eq!(message(&req, "query:limit").unwrap(), "limit is required");
    assert_eq!(
        message(&req, "header:X-Request-Id").unwrap(),
        "X-Request-Id is required"
    );

    let mut r = list("limit=5");
    r.headers = vec![KeyValue::new("x-request-id", "nope")];
    let req = reqs(&spec, &r, &[]);
    assert_eq!(
        message(&req, "header:X-Request-Id").unwrap(),
        "must be a UUID"
    );
}

#[test]
fn request_bodies() {
    let spec = pets();
    let mut r = request("POST", "{{baseUrl}}/pets");
    let req = reqs(&spec, &r, &[]);
    let body = field(&req, "body");
    assert_eq!(body.place, InputPlace::Body);
    assert!(body.required);
    assert_eq!(body.value, None);
    let p = body.problem.as_ref().unwrap();
    assert_eq!(p.kind, ProblemKind::Missing);
    assert_eq!(p.message, "a request body is required");
    // An example generated from the schema, and the schema inlined.
    let example: Value = serde_json::from_str(body.example.as_deref().unwrap()).unwrap();
    assert_eq!(example, json!({ "name": "string", "status": "available" }));
    let schema = body.schema.as_ref().unwrap();
    assert_eq!(schema["properties"]["status"]["enum"][2], "sold");

    r.body = json_body(r#"{"status": "lost", "name": ""}"#);
    let req = reqs(&spec, &r, &[]);
    let p = field(&req, "body").problem.clone().unwrap();
    assert_eq!(p.kind, ProblemKind::Invalid);
    assert_eq!(p.message, "the body does not match the schema");
    assert_eq!(
        p.details,
        vec![
            "$.name: must be at least 1 character",
            "$.status: must be one of available, pending, sold"
        ]
    );
    r.body = json_body(r#"{"status": "sold"}"#);
    let p = field(&reqs(&spec, &r, &[]), "body")
        .problem
        .clone()
        .unwrap();
    assert_eq!(p.details, vec!["$.name: required property is missing"]);
    assert_eq!(
        field(&reqs(&spec, &r, &[]), "body").value.as_deref(),
        Some(r#"{"status": "sold"}"#)
    );

    r.body = json_body(r#"{"name": "Rex"}"#);
    assert!(reqs(&spec, &r, &[]).ready);

    r.body = json_body("{ not json");
    assert_eq!(
        message(&reqs(&spec, &r, &[]), "body").unwrap(),
        "the body is not valid JSON"
    );
    r.body = Body::Raw {
        kind: RawKind::Xml,
        data: "<pet/>".into(),
    };
    assert_eq!(
        message(&reqs(&spec, &r, &[]), "body").unwrap(),
        "the body is application/xml, the operation accepts application/json"
    );

    // Forms: missing required fields.
    let mut r = request("POST", "{{baseUrl}}/forms");
    r.body = Body::FormUrlEncoded {
        fields: vec![KeyValue::new("a", "1")],
    };
    let req = reqs(&spec, &r, &[]);
    let p = field(&req, "body").problem.clone().unwrap();
    assert_eq!(p.kind, ProblemKind::Missing);
    assert_eq!(p.message, "the body is missing required fields");
    assert_eq!(p.details, vec!["b: required field is missing"]);
    assert_eq!(field(&req, "body").value.as_deref(), Some("a=1"));
    // Not required: no body is fine.
    r.body = Body::None;
    assert!(reqs(&spec, &r, &[]).ready);
}

fn fact(name: &str, state: VarState, secret: bool, auth: bool) -> VarFact {
    VarFact {
        name: name.into(),
        state,
        secret,
        auth,
        value: (state == VarState::Empty).then(String::new),
    }
}

#[test]
fn variables_servers_and_order() {
    let spec = pets();
    let mut r = request("GET", "{{baseUrl}}/pets/:petId");
    r.params = vec![path_param("petId", "")];
    let vars = [
        fact("baseUrl", VarState::Unresolved, false, false),
        fact("region", VarState::Empty, false, false),
        fact("token", VarState::Unresolved, true, true),
        fact("fine", VarState::Set, false, false),
    ];
    let req = reqs(&spec, &r, &vars);
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
    let base = field(&req, "variable:baseUrl");
    assert_eq!(base.place, InputPlace::Variable);
    assert_eq!(
        message(&req, "variable:baseUrl").unwrap(),
        "baseUrl has no value"
    );
    assert_eq!(
        base.schema,
        Some(json!({ "type": "string", "format": "uri" }))
    );
    assert_eq!(
        base.example.as_deref(),
        Some("https://{{region}}.example.com/v1")
    );
    let region = field(&req, "variable:region");
    assert_eq!(message(&req, "variable:region").unwrap(), "region is empty");
    assert_eq!(
        region.schema,
        Some(json!({ "type": "string", "enum": ["eu", "us"], "default": "eu" }))
    );
    assert_eq!(region.example.as_deref(), Some("eu"));
    assert_eq!(
        region.description.as_deref(),
        Some("Region of the data centre")
    );
    let token = field(&req, "variable:token");
    assert!(token.secret && token.auth && token.required);
    assert_eq!(token.value, None);
    assert_eq!(
        message(&req, "variable:token").unwrap(),
        "token has no value"
    );

    assert_eq!(req.servers.len(), 1);
    assert_eq!(req.servers[0].url, "https://{{region}}.example.com/v1");
    assert_eq!(req.servers[0].description.as_deref(), Some("Production"));

    // Problems first, fine fields after, each part in place order.
    let r = list("limit=0");
    let req = reqs(&spec, &r, &vars[..1]);
    let ids: Vec<&str> = req.fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "variable:baseUrl",
            "query:limit",
            "query:status",
            "query:since",
            "query:ids",
            "header:X-Request-Id"
        ]
    );
}

#[test]
fn no_operation_or_not_http() {
    let spec = pets();
    let facts = |r: &RequestSpec| {
        requirements(
            &spec,
            &RequestFacts {
                request: r,
                variables: &[],
            },
        )
        .is_some()
    };
    assert!(!facts(&request("GET", "{{baseUrl}}/cats")));
    assert!(!facts(&request("DELETE", "{{baseUrl}}/pets")));
    let mut ws = request("WS", "{{baseUrl}}/pets");
    ws.protocol = Protocol::WebSocket(Box::default());
    assert!(!facts(&ws));
    let response = ResponseFacts {
        status: 200,
        headers: &[],
        body: None,
    };
    assert!(validate_response(&spec, &request("GET", "{{baseUrl}}/cats"), &response).is_none());
}

#[test]
fn swagger_2_operations() {
    let spec = json!({
        "swagger": "2.0",
        "info": { "title": "Old", "version": "1" },
        "host": "api.example.com",
        "basePath": "/v2",
        "schemes": ["https"],
        "produces": ["application/json"],
        "definitions": {
            "Pet": { "type": "object", "required": ["name"],
                     "properties": { "name": { "type": "string" }, "id": { "type": "integer" } } }
        },
        "paths": {
            "/pets/{id}": {
                "get": {
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "type": "integer", "minimum": 1 },
                        { "name": "tags", "in": "query", "type": "array", "collectionFormat": "pipes",
                          "items": { "type": "string", "enum": ["a", "b"] } }
                    ],
                    "responses": { "200": { "description": "ok", "schema": { "$ref": "#/definitions/Pet" } } }
                }
            },
            "/pets": {
                "post": {
                    "parameters": [{ "name": "pet", "in": "body", "required": true,
                                      "schema": { "$ref": "#/definitions/Pet" } }],
                    "responses": { "201": { "description": "created" } }
                }
            }
        }
    });
    let spec = ParsedSpec::parse(&serde_json::to_vec(&spec).unwrap()).unwrap();
    let req = reqs(&spec, &request("GET", "{{baseUrl}}/pets/0?tags=a|c"), &[]);
    assert_eq!(req.operation, "GET /pets/{id}");
    assert_eq!(message(&req, "path:id").unwrap(), "must be at least 1");
    assert_eq!(
        message(&req, "query:tags").unwrap(),
        "$[1]: must be one of a, b"
    );
    assert_eq!(req.servers[0].url, "https://api.example.com/v2");
    assert_eq!(
        field(&req, "path:id").schema,
        Some(json!({ "type": "integer", "minimum": 1 }))
    );

    let mut r = request("POST", "https://api.example.com/v2/pets");
    let req = reqs(&spec, &r, &[]);
    assert_eq!(message(&req, "body").unwrap(), "a request body is required");
    r.body = json_body(r#"{"id": 1}"#);
    let req = reqs(&spec, &r, &[]);
    assert_eq!(
        field(&req, "body").problem.as_ref().unwrap().details,
        vec!["$.name: required property is missing"]
    );

    let check = validate_response(
        &spec,
        &request("GET", "{{baseUrl}}/pets/1"),
        &ResponseFacts {
            status: 200,
            headers: &[("Content-Type".into(), "application/json".into())],
            body: Some(br#"{"name": 5}"#),
        },
    )
    .unwrap();
    assert_eq!(check.differences.len(), 1);
    assert_eq!(check.differences[0].path, "$.name");
    assert_eq!(check.differences[0].expected, "string");
    assert_eq!(check.differences[0].got, "integer 5");
}

// ---- the response check ------------------------------------------------------------------

fn json_headers(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut h = vec![(
        "content-type".to_owned(),
        "application/json; charset=utf-8".to_owned(),
    )];
    h.extend(
        extra
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned())),
    );
    h
}

fn check_list(status: u16, headers: &[(String, String)], body: &[u8]) -> ResponseCheck {
    validate_response(
        &pets(),
        &list("limit=5"),
        &ResponseFacts {
            status,
            headers,
            body: Some(body),
        },
    )
    .unwrap()
}

fn diffs(c: &ResponseCheck) -> Vec<(DifferencePlace, &str, &str, &str)> {
    c.differences
        .iter()
        .map(|d| {
            (
                d.place,
                d.path.as_str(),
                d.expected.as_str(),
                d.got.as_str(),
            )
        })
        .collect()
}

#[test]
fn a_matching_response() {
    let headers = json_headers(&[("X-Total", "1")]);
    let c = check_list(
        200,
        &headers,
        br#"[{"id": 1, "name": "Rex", "status": "sold"}]"#,
    );
    assert_eq!(c.operation, "GET /pets");
    assert_eq!(c.status, 200);
    assert_eq!(c.matched.as_deref(), Some("200"));
    assert!(c.differences.is_empty(), "{:#?}", c.differences);
    assert_eq!(c.body_skipped, None);
}

#[test]
fn body_differences_at_paths() {
    let headers = json_headers(&[("X-Total", "1")]);
    let c = check_list(
        200,
        &headers,
        br#"[{"id": 1, "name": "Rex"}, {"id": "x", "name": "abcdefghijk", "tags": ["a", "a"]}, {"id": 0}]"#,
    );
    assert_eq!(
        diffs(&c),
        [
            (DifferencePlace::Body, "$[1].id", "integer", "string \"x\""),
            (
                DifferencePlace::Body,
                "$[1].name",
                "at most 10 characters",
                "11 characters"
            ),
            (
                DifferencePlace::Body,
                "$[1].tags",
                "unique items",
                "string \"a\" twice"
            ),
            (
                DifferencePlace::Body,
                "$[2].name",
                "required property",
                "missing"
            ),
            (DifferencePlace::Body, "$[2].id", "at least 1", "integer 0"),
        ]
    );
    let c = check_list(200, &headers, br#"{"id": 1}"#);
    assert_eq!(diffs(&c), [(DifferencePlace::Body, "$", "array", "object")]);
    let c = check_list(200, &headers, b"not json");
    assert_eq!(
        diffs(&c),
        [(
            DifferencePlace::Body,
            "$",
            "JSON",
            "text that is not valid JSON"
        )]
    );
    // At most 20.
    let many: Vec<Value> = (0..50).map(|_| json!({ "id": "x", "name": "n" })).collect();
    let c = check_list(200, &headers, &serde_json::to_vec(&many).unwrap());
    assert_eq!(c.differences.len(), 20);
}

#[test]
fn undeclared_status_and_status_classes() {
    let spec = pets();
    let mut get = request("GET", "{{baseUrl}}/pets/:petId");
    get.params = vec![path_param("petId", "rex")];
    let c = validate_response(
        &spec,
        &get,
        &ResponseFacts {
            status: 500,
            headers: &[],
            body: Some(b""),
        },
    )
    .unwrap();
    assert_eq!(c.matched, None);
    assert_eq!(
        diffs(&c),
        [(DifferencePlace::Status, "status", "one of 200, 404", "500")]
    );
    assert!(c.body_skipped.is_some());

    // `4XX` without content.
    let mut post = request("POST", "{{baseUrl}}/pets");
    post.body = json_body(r#"{"name": "Rex"}"#);
    let c = validate_response(
        &spec,
        &post,
        &ResponseFacts {
            status: 422,
            headers: &[],
            body: Some(b""),
        },
    )
    .unwrap();
    assert_eq!(c.matched.as_deref(), Some("4XX"));
    assert!(c.differences.is_empty());

    // `default`.
    let c = check_list(500, &json_headers(&[]), br#"{"message": 1}"#);
    assert_eq!(c.matched.as_deref(), Some("default"));
    assert_eq!(
        diffs(&c),
        [(DifferencePlace::Body, "$.message", "string", "integer 1")]
    );
}

#[test]
fn content_type_and_headers() {
    let html = vec![
        ("Content-Type".to_owned(), "text/html".to_owned()),
        ("X-Total".to_owned(), "1".to_owned()),
    ];
    let c = check_list(200, &html, b"<p>hi</p>");
    assert_eq!(
        diffs(&c),
        [(
            DifferencePlace::ContentType,
            "Content-Type",
            "application/json",
            "text/html"
        )]
    );
    assert!(c.body_skipped.is_some());

    let c = check_list(200, &json_headers(&[]), b"[]");
    assert_eq!(
        diffs(&c),
        [(DifferencePlace::Header, "X-Total", "present", "missing")]
    );
    let c = check_list(200, &json_headers(&[("x-total", "many")]), b"[]");
    assert_eq!(
        diffs(&c),
        [(
            DifferencePlace::Header,
            "X-Total",
            "integer",
            "string \"many\""
        )]
    );
}

#[test]
fn large_or_missing_bodies_are_skipped() {
    let headers = json_headers(&[("X-Total", "1")]);
    let big = vec![b' '; MAX_CHECKED_BODY + 1];
    let c = check_list(200, &headers, &big);
    assert!(c.differences.is_empty());
    assert_eq!(
        c.body_skipped.as_deref(),
        Some("the body is larger than 8 MB")
    );

    let c = validate_response(
        &pets(),
        &list("limit=5"),
        &ResponseFacts {
            status: 200,
            headers: &headers,
            body: None,
        },
    )
    .unwrap();
    assert_eq!(
        c.body_skipped.as_deref(),
        Some("the body was not kept in full")
    );
}

#[test]
fn json_shapes_of_the_results_stay() {
    // The UI reads these names (apps/ui/src/commands/types.ts).
    let spec = pets();
    let mut r = request("GET", "{{baseUrl}}/pets/:petId");
    r.params = vec![path_param("petId", "")];
    let v = serde_json::to_value(reqs(&spec, &r, &[])).unwrap();
    for key in [
        "operation",
        "summary",
        "deprecated",
        "spec",
        "ready",
        "fields",
        "servers",
    ] {
        assert!(v.get(key).is_some(), "{key}");
    }
    assert_eq!(v["fields"][0]["place"], "path");
    assert_eq!(v["fields"][0]["problem"]["kind"], "missing");
}
