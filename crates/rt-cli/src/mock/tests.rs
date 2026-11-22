// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Matching of `rt mock` against the fixture collection `testdata/shop`.

use std::path::{Path, PathBuf};

use super::*;

fn shop() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/shop")
}

fn mock(env: Option<&str>) -> Mock {
    load(&Args {
        collection: shop(),
        environment: env.map(str::to_owned),
        ..Args::default()
    })
    .unwrap()
}

fn get(m: &Mock, path_and_query: &str) -> Answer {
    send(m, "GET", path_and_query, &[], "")
}

fn send(
    m: &Mock,
    method: &str,
    path_and_query: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Answer {
    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    m.answer(&MockRequest {
        method,
        path,
        query,
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        body: body.as_bytes(),
    })
}

fn example(a: &Answer) -> &str {
    a.matched.as_ref().map_or("<none>", |(_, e)| e.as_str())
}

fn header<'a>(a: &'a Answer, name: &str) -> Option<&'a str> {
    a.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn routes_follow_the_environment() {
    let routes = mock(Some("Local")).routes();
    let lines: Vec<String> = routes
        .iter()
        .map(|r| format!("{} {} {}", r.method, r.path, r.examples.join("|")))
        .collect();
    assert_eq!(
        lines,
        [
            "GET /api/v1/users/:id Found|Not found",
            "GET /api/v1/users First page|Second page",
            "POST /api/v1/users Created|Invalid",
            "GET /api/v1/orders/{orderId} As JSON|As XML",
        ]
    );
    // Without an environment the base URL placeholder is not part of the path.
    let paths: Vec<String> = mock(None).routes().into_iter().map(|r| r.path).collect();
    assert_eq!(paths[0], "/v1/users/:id");
}

#[test]
fn the_example_with_the_saved_path_wins() {
    let m = mock(Some("Local"));
    let found = get(&m, "/api/v1/users/42");
    assert_eq!(found.status, 200);
    assert_eq!(example(&found), "Found");
    assert_eq!(header(&found, "x-request-id"), Some("example-1"));
    assert_eq!(header(&found, "content-type"), Some("application/json"));
    assert!(String::from_utf8_lossy(&found.body).contains("\"Ada\""));

    let missing = get(&m, "/api/v1/users/999");
    assert_eq!(missing.status, 404);
    assert_eq!(example(&missing), "Not found");
    assert_eq!(missing.reason.as_deref(), Some("Not Found"));
    // Body type json without a Content-Type header in the example.
    assert_eq!(header(&missing, "content-type"), Some("application/json"));

    // Any other id fits the template; the first example answers.
    assert_eq!(example(&get(&m, "/api/v1/users/7")), "Found");
    // Trailing slash and percent-encoding do not matter.
    assert_eq!(example(&get(&m, "/api/v1/users/%34%32/")), "Found");
}

#[test]
fn query_header_and_body_pick_among_examples() {
    let m = mock(Some("Local"));
    assert_eq!(example(&get(&m, "/api/v1/users?page=2")), "Second page");
    assert_eq!(example(&get(&m, "/api/v1/users?page=1")), "First page");
    assert_eq!(example(&get(&m, "/api/v1/users")), "First page");

    let xml = send(
        &m,
        "GET",
        "/api/v1/orders/A-1",
        &[("Accept", "application/xml")],
        "",
    );
    assert_eq!(example(&xml), "As XML");
    assert_eq!(header(&xml, "content-type"), Some("application/xml"));
    let json = send(
        &m,
        "get",
        "/api/v1/orders/A-1",
        &[("accept", "application/json")],
        "",
    );
    assert_eq!(example(&json), "As JSON");

    // JSON bodies compare as JSON, whatever the whitespace.
    let created = send(&m, "POST", "/api/v1/users", &[], "{ \"name\" : \"Grace\" }");
    assert_eq!(created.status, 201);
    assert_eq!(header(&created, "location"), Some("/api/v1/users/43"));
    let invalid = send(&m, "POST", "/api/v1/users", &[], "{}");
    assert_eq!(invalid.status, 400);
    assert_eq!(
        header(&invalid, "content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(invalid.body, b"name is required");
}

#[test]
fn no_example_is_a_404_with_the_routes() {
    let m = mock(Some("Local"));
    for (method, path) in [
        ("GET", "/api/health"),
        ("DELETE", "/api/v1/users/42"),
        ("GET", "/api/v1/users/42/roles"),
    ] {
        let a = send(&m, method, path, &[], "");
        assert_eq!(a.status, 404, "{method} {path}");
        assert_eq!(a.matched, None);
        let json: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
        assert_eq!(
            json["error"],
            format!("no saved example matches {method} {path}")
        );
        assert_eq!(json["routes"].as_array().unwrap().len(), 4);
        assert_eq!(json["routes"][0]["path"], "/api/v1/users/:id");
        assert_eq!(json["routes"][0]["request"], "users/get-user.yml");
    }
}

#[test]
fn an_unknown_environment_is_an_error() {
    let err = load(&Args {
        collection: shop(),
        environment: Some("Prod".into()),
        ..Args::default()
    })
    .unwrap_err();
    assert!(err.contains("no environment \"Prod\""), "{err}");
    assert!(err.contains("Local"), "{err}");
}

#[test]
fn a_folder_inside_the_collection_serves_the_whole_collection() {
    let m = load(&Args {
        collection: shop().join("users"),
        environment: Some("Local".into()),
        ..Args::default()
    })
    .unwrap();
    assert_eq!(m.routes().len(), 4);
}

#[test]
fn parses_arguments() {
    let parse_args = |args: &[&str]| parse(&mut lexopt::Parser::from_args(args.iter().copied()));
    let a = parse_args(&[]).unwrap().unwrap();
    assert_eq!(a, Args::default());
    assert!(a.host.is_loopback());
    let a = parse_args(&["coll", "--port", "0", "-e", "Local", "--host", "::1"])
        .unwrap()
        .unwrap();
    assert_eq!(a.collection, PathBuf::from("coll"));
    assert_eq!(a.port, 0);
    assert_eq!(a.environment.as_deref(), Some("Local"));
    assert_eq!(a.host.to_string(), "::1");
    assert!(parse_args(&["--help"]).unwrap().unwrap().help);
    assert!(parse_args(&["--port", "x"]).is_err());
    assert!(parse_args(&["a", "b"]).is_err());
}

#[test]
fn banner_lists_the_routes() {
    let m = mock(Some("Local"));
    let text = banner(&m, "127.0.0.1:4010".parse().unwrap());
    assert!(
        text.starts_with("rt mock: listening on http://127.0.0.1:4010\n"),
        "{text}"
    );
    assert!(
        text.contains("/api/v1/users/:id  (users/get-user.yml: Found, Not found)"),
        "{text}"
    );
}
