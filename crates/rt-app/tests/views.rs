// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved views (wave 18, strand G): the companion file is stable and holds no response value,
//! JSONPath views over a stored response, tables with columns, the body limit, jq reported as
//! not available. Synthetic data only.

use std::time::Duration;

use rt_app::views::{
    self, ResponseView, VIEW_BODY_LIMIT, ViewDisplay, ViewLanguage, ViewsFile, companion_path,
};
use rt_app::{AppError, ResponseStore, Workspace};
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendReport};
use rt_core::transport::{HttpVersion, Timing};
use rt_format::{COLLECTION_FILE, FileSet};

const CANARY: &str = "RT-CANARY-views-secret-5";

fn workspace() -> Workspace {
    let mut set = FileSet::default();
    set.files.insert(
        COLLECTION_FILE.to_owned(),
        b"opencollection: \"1.0.0\"\ninfo:\n  name: shop\n".to_vec(),
    );
    set.files.insert(
        "users/get.yml".to_owned(),
        b"info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1/x\"\n  auth: inherit\n"
            .to_vec(),
    );
    Workspace::open(set).expect("fixture collection")
}

fn report(body: &str) -> SendReport {
    SendReport {
        tests: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        var_changes: Vec::new(),
        flow: None,
        notes: Vec::new(),
        version: REPORT_VERSION,
        request_path: "users/get.yml".into(),
        environment: None,
        request: None,
        response: Some(ReceivedResponse {
            tls: None,
            status: 200,
            version: HttpVersion::Http11,
            headers: vec![],
            url: "http://127.0.0.1/".into(),
            redirects: vec![],
            body: ReportBody::Utf8(body.to_owned()),
            size: body.len() as u64,
            truncated: false,
            timing: Timing {
                dns: None,
                connect: None,
                tls: None,
                ttfb: Duration::from_millis(1),
            },
            total: Duration::from_millis(2),
        }),
        variables: vec![],
        unresolved: vec![],
        error: None,
        checks: Vec::new(),
        rate_limit: None,
        trace: None,
        segments: Vec::new(),
    }
}

fn view(name: &str, expression: &str, display: ViewDisplay, columns: &[&str]) -> ResponseView {
    ResponseView {
        name: name.into(),
        language: ViewLanguage::Jsonpath,
        expression: expression.into(),
        display,
        columns: columns.iter().map(|c| (*c).to_owned()).collect(),
    }
}

const USERS: &str = r#"{"items":[{"id":1,"name":"Ada","tags":["a"]},{"id":2,"name":"Bob","email":"b@x.test"},{"id":3,"name":"Cy"}],"total":3}"#;

fn stored(body: &str) -> (ResponseStore, u64) {
    let mut store = ResponseStore::new();
    let id = store.insert(report(body)).response_id.unwrap();
    (store, id)
}

#[test]
fn the_file_is_plain_stable_json_next_to_the_request() {
    assert_eq!(companion_path("users/get.yml"), "users/get.views.json");
    let mut ws = workspace();
    views::save(
        &mut ws,
        "users/get.yml",
        view("Ids", "$.items[*].id", ViewDisplay::Json, &[]),
    )
    .unwrap();
    views::save(
        &mut ws,
        "users/get.yml",
        view("Users", "$.items", ViewDisplay::Table, &["id", "name"]),
    )
    .unwrap();
    let text = rt_app::companions::read(&ws, "users/get.views.json").expect("staged");
    assert_eq!(
        text,
        "{\n  \"version\": 1,\n  \"views\": [\n    {\n      \"name\": \"Ids\",\n      \"language\": \"jsonpath\",\n      \"expression\": \"$.items[*].id\",\n      \"display\": \"json\"\n    },\n    {\n      \"name\": \"Users\",\n      \"language\": \"jsonpath\",\n      \"expression\": \"$.items\",\n      \"display\": \"table\",\n      \"columns\": [\n        \"id\",\n        \"name\"\n      ]\n    }\n  ]\n}\n"
    );
    // Reading and writing again gives the same bytes.
    let file = views::parse_file(&text).unwrap();
    assert_eq!(views::render_file(&file), text);
    // Saving a view with an existing name replaces it in place; the order stays.
    let all = views::save(
        &mut ws,
        "users/get.yml",
        view("Ids", "$.items[*].name", ViewDisplay::Text, &["ignored"]),
    )
    .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "Ids");
    assert_eq!(all[0].expression, "$.items[*].name");
    assert!(all[0].columns.is_empty(), "columns belong to tables only");
    assert_eq!(views::list(&ws, "users/get.yml").unwrap(), all);
    let all = views::remove(&mut ws, "users/get.yml", "Ids").unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "Users");
}

#[test]
fn nothing_is_written_by_itself_and_bad_input_is_refused() {
    let mut ws = workspace();
    assert!(views::list(&ws, "users/get.yml").unwrap().is_empty());
    assert!(rt_app::companions::read(&ws, "users/get.views.json").is_none());
    assert!(matches!(
        views::save(&mut ws, "nope.yml", view("A", "$", ViewDisplay::Json, &[])),
        Err(AppError::RequestNotFound(_))
    ));
    for bad in [
        view("", "$", ViewDisplay::Json, &[]),
        view("A", "", ViewDisplay::Json, &[]),
        view("A", "$.[", ViewDisplay::Json, &[]),
    ] {
        assert!(views::save(&mut ws, "users/get.yml", bad).is_err());
    }
    assert!(rt_app::companions::read(&ws, "users/get.views.json").is_none());
    assert!(matches!(
        views::parse_file("{\"version\":2,\"views\":[]}"),
        Err(AppError::Format(_))
    ));
    assert!(views::parse_file("{\"version\":1,\"views\":[],\"extra\":1}").is_err());
    assert!(views::parse_file("not json").is_err());
}

#[test]
fn the_file_holds_the_expression_never_a_value_of_a_response() {
    let mut ws = workspace();
    let body = format!(r#"{{"token":"{CANARY}","items":[{{"id":1}}]}}"#);
    let (store, id) = stored(&body);
    let v = view("Token", "$.token", ViewDisplay::Text, &[]);
    let result = views::apply(&store, id, &v).unwrap();
    assert_eq!(result.values, vec![serde_json::json!(CANARY)]);
    views::save(&mut ws, "users/get.yml", v).unwrap();
    let text = rt_app::companions::read(&ws, "users/get.views.json").unwrap();
    assert!(!text.contains(CANARY));
}

#[test]
fn json_and_text_views_return_the_matches() {
    let (store, id) = stored(USERS);
    let r = views::apply(
        &store,
        id,
        &view("n", "$.items[*].name", ViewDisplay::Json, &[]),
    )
    .unwrap();
    assert_eq!(r.display, ViewDisplay::Json);
    assert_eq!(
        r.values,
        vec![
            serde_json::json!("Ada"),
            serde_json::json!("Bob"),
            serde_json::json!("Cy")
        ]
    );
    assert!(r.notice.is_none());
    let r = views::apply(&store, id, &view("t", "$.total", ViewDisplay::Text, &[])).unwrap();
    assert_eq!(r.display, ViewDisplay::Text);
    assert_eq!(r.values, vec![serde_json::json!(3)]);
}

#[test]
fn a_table_has_the_chosen_columns_or_the_first_keys() {
    let (store, id) = stored(USERS);
    let r = views::apply(
        &store,
        id,
        &view("u", "$.items", ViewDisplay::Table, &["name", "id", "email"]),
    )
    .unwrap();
    assert_eq!(r.display, ViewDisplay::Table);
    assert_eq!(r.columns, vec!["name", "id", "email"]);
    assert_eq!(r.rows.len(), 3);
    assert_eq!(
        r.rows[1],
        vec![
            serde_json::json!("Bob"),
            serde_json::json!(2),
            serde_json::json!("b@x.test")
        ]
    );
    assert_eq!(
        r.rows[0][2],
        serde_json::Value::Null,
        "a missing key is null"
    );
    // Without columns: the keys of the rows, first seen first. `[*]` matches give the same rows.
    let r = views::apply(
        &store,
        id,
        &view("u", "$.items[*]", ViewDisplay::Table, &[]),
    )
    .unwrap();
    assert_eq!(r.columns, vec!["id", "name", "tags", "email"]);
    assert_eq!(r.rows.len(), 3);
}

#[test]
fn what_cannot_be_shown_comes_with_a_notice() {
    let (store, id) = stored(USERS);
    // Not an array of objects: the values as JSON, with the reason.
    let r = views::apply(&store, id, &view("t", "$.total", ViewDisplay::Table, &[])).unwrap();
    assert_eq!(r.display, ViewDisplay::Json);
    assert_eq!(r.values, vec![serde_json::json!(3)]);
    assert!(r.notice.as_deref().unwrap().contains("table"));
    // No match.
    let r = views::apply(&store, id, &view("x", "$.nothing", ViewDisplay::Json, &[])).unwrap();
    assert!(r.values.is_empty());
    assert!(r.notice.as_deref().unwrap().contains("matched nothing"));
    // Not JSON.
    let (store, id) = stored("plain text");
    let r = views::apply(&store, id, &view("x", "$", ViewDisplay::Json, &[])).unwrap();
    assert!(r.values.is_empty());
    assert!(r.notice.as_deref().unwrap().contains("not JSON"));
    // A broken expression is an error.
    assert!(matches!(
        views::apply(&store, id, &view("x", "$.[", ViewDisplay::Json, &[])),
        Err(AppError::Filter(_))
    ));
    assert!(matches!(
        views::apply(&store, 99, &view("x", "$", ViewDisplay::Json, &[])),
        Err(AppError::ResponseNotFound(99))
    ));
}

#[test]
fn big_results_are_cut_so_the_ui_never_gets_the_whole_body() {
    let rows: Vec<String> = (0..200_000)
        .map(|i| format!(r#"{{"id":{i},"name":"user {i}"}}"#))
        .collect();
    let body = format!("[{}]", rows.join(","));
    let (store, id) = stored(&body);
    let r = views::apply(&store, id, &view("all", "$", ViewDisplay::Table, &[])).unwrap();
    assert!(r.rows.len() < 200_000);
    assert!(r.notice.as_deref().unwrap().contains("first"));
    let r = views::apply(&store, id, &view("all", "$[*].id", ViewDisplay::Json, &[])).unwrap();
    assert!(r.values.len() < 200_000);
    assert!(r.notice.is_some());
}

#[test]
fn a_body_over_the_limit_gets_a_hint_not_a_view() {
    // Exactly the limit is still viewed: a long JSON string padded to the size.
    let pad = VIEW_BODY_LIMIT - r#"{"a":""}"#.len();
    let body = format!(r#"{{"a":"{}"}}"#, "x".repeat(pad));
    assert_eq!(body.len(), VIEW_BODY_LIMIT);
    let r = views::apply_body(body.as_bytes(), &view("k", "$", ViewDisplay::Json, &[])).unwrap();
    // The body was parsed; the result is cut, not the view refused.
    assert!(r.notice.as_deref().unwrap().to_lowercase().contains("cut"));
    let over = format!("{body} ");
    let r = views::apply_body(over.as_bytes(), &view("k", "$.a", ViewDisplay::Json, &[])).unwrap();
    assert!(r.values.is_empty());
    assert!(r.notice.as_deref().unwrap().contains("32 MiB"));
}

#[test]
fn jq_is_reserved_and_said_not_to_be_available() {
    assert_eq!(views::languages(), vec![ViewLanguage::Jsonpath]);
    let jq = ResponseView {
        language: ViewLanguage::Jq,
        ..view("j", ".items", ViewDisplay::Json, &[])
    };
    let (store, id) = stored(USERS);
    match views::apply(&store, id, &jq) {
        Err(AppError::Unsupported(m)) => assert!(m.contains("jq is not available in this build")),
        other => panic!("{other:?}"),
    }
    let mut ws = workspace();
    assert!(matches!(
        views::save(&mut ws, "users/get.yml", jq),
        Err(AppError::Unsupported(_))
    ));
    // A file written by a build that has jq stays readable and is kept as it is.
    let file = views::parse_file(
        "{\n  \"version\": 1,\n  \"views\": [\n    {\n      \"name\": \"J\",\n      \"language\": \"jq\",\n      \"expression\": \".a\",\n      \"display\": \"json\"\n    }\n  ]\n}\n",
    )
    .unwrap();
    assert_eq!(file.views[0].language, ViewLanguage::Jq);
    let _: ViewsFile = file;
}

/// A JSON body of about `size` bytes: `{"items":[{"id":…,"name":…,"email":…,"active":…},…]}`.
fn big_body(size: usize) -> String {
    let mut body = String::with_capacity(size + 128);
    body.push_str("{\"items\":[");
    let mut i = 0usize;
    while body.len() < size {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            r#"{{"id":{i},"name":"user number {i}","email":"user{i}@example.test","active":{},"score":{}.5}}"#,
            i.is_multiple_of(2),
            i % 100
        ));
        i += 1;
    }
    body.push_str("]}");
    body
}

fn median_ms(body: &[u8], view: &ResponseView) -> (f64, Vec<f64>) {
    const RUNS: usize = 9;
    let mut runs = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = web_time::Instant::now();
        let result = views::apply_body(body, view).unwrap();
        runs.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(result.notice.is_none() || result.notice.as_deref().unwrap().contains("first"));
        assert!(!result.values.is_empty() || !result.rows.is_empty());
    }
    runs.sort_by(f64::total_cmp);
    (runs[RUNS / 2], runs)
}

/// The budget `view_apply`: a JSONPath view and a table over a 10 MiB JSON body, evaluated on
/// the bytes the `ResponseStore` keeps (no copy), median of 9, the slower of the two; and the
/// same on a body at the 32 MiB limit. `scripts/perf/views.mjs` runs this in a release build with
/// `RT_PERF_OUT=<dir>` and reads `<dir>/views.json`.
#[test]
fn view_apply_is_measured() {
    let mut store = ResponseStore::new();
    let body = big_body(10 * 1024 * 1024);
    let id = store.insert(report(&body)).response_id.unwrap();
    let kept = store.body(id).unwrap();
    let json = view(
        "ids",
        "$.items[?@.active == true].id",
        ViewDisplay::Json,
        &[],
    );
    let table = view(
        "users",
        "$.items",
        ViewDisplay::Table,
        &["id", "name", "email"],
    );
    let (json_ms, json_runs) = median_ms(kept, &json);
    let (table_ms, table_runs) = median_ms(kept, &table);
    let view_apply = json_ms.max(table_ms);

    let big = big_body(32 * 1024 * 1024 - 256);
    assert!(big.len() <= VIEW_BODY_LIMIT);
    let (big_ms, _) = median_ms(big.as_bytes(), &table);
    println!(
        "view_apply: json {json_ms:.1} ms, table {table_ms:.1} ms (10 MiB); table at 32 MiB {big_ms:.1} ms"
    );
    if !cfg!(debug_assertions) {
        assert!(big_ms < 500.0, "32 MiB took {big_ms:.0} ms");
    }
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let out = serde_json::json!({
            "view_apply": view_apply,
            "json_runs": json_runs,
            "table_runs": table_runs,
            "at_32_mib": big_ms,
        });
        std::fs::write(
            std::path::Path::new(&dir).join("views.json"),
            serde_json::to_string_pretty(&out).unwrap(),
        )
        .unwrap();
    }
}
