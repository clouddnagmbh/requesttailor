// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Full-text search over the open collection (wave 4, strand E): fields, ranking, snippets
//! with UTF-16 ranges, unsaved changes, secrets, and speed on 2,000 requests.

use rt_app::search::{SEARCH_LIMIT, SNIPPET_CHARS, search};
use rt_app::{SearchField, SearchHit, Workspace};
use rt_core::model::{
    ApiKeyPlacement, Auth, Body, KeyValue, MultipartPart, Param, ParamKind, PartKind, RawKind,
    Variable,
};
use rt_format::{COLLECTION_FILE, FileSet};

fn files(list: &[(&str, String)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

fn request(name: &str, seq: u32, method: &str, url: &str) -> String {
    format!(
        "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n"
    )
}

/// Root: `ping.yml` (seq 1), folder `users` (seq 2) with `get.yml`, `create.yml`.
fn workspace() -> Workspace {
    Workspace::open(files(&[
        (COLLECTION_FILE, "info:\n  name: Search\n".into()),
        ("ping.yml", request("Ping", 1, "GET", "{{base}}/ping")),
        (
            "users/folder.yml",
            "info:\n  name: Users\n  seq: 2\n".into(),
        ),
        (
            "users/get.yml",
            request("Get user", 1, "GET", "{{base}}/users/{{id}}"),
        ),
        (
            "users/create.yml",
            request("Create user", 2, "POST", "{{base}}/users"),
        ),
    ]))
    .unwrap()
}

/// `field` and `snippet` of each hit.
fn short(hits: &[SearchHit]) -> Vec<(&str, SearchField, &str)> {
    hits.iter()
        .map(|h| (h.path.as_str(), h.field, h.snippet.as_str()))
        .collect()
}

/// The highlighted parts of a hit, cut out with JavaScript (UTF-16) offsets.
fn highlighted(hit: &SearchHit) -> Vec<String> {
    let units: Vec<u16> = hit.snippet.encode_utf16().collect();
    hit.ranges
        .iter()
        .map(|&(s, e)| String::from_utf16(&units[s as usize..e as usize]).unwrap())
        .collect()
}

#[test]
fn empty_query_finds_nothing() {
    let ws = workspace();
    assert!(search(&ws, "", 50).is_empty());
    assert!(search(&ws, "   ", 50).is_empty());
    assert!(search(&ws, "user", 0).is_empty());
}

#[test]
fn ranks_name_then_url_then_rest_and_keeps_tree_order() {
    let mut ws = workspace();
    let mut ping = ws.request("ping.yml").unwrap();
    ping.headers = vec![KeyValue::new("X-User", "ada")];
    ws.update_request("ping.yml", ping).unwrap();

    let hits = search(&ws, "user", 50);
    assert_eq!(
        short(&hits),
        [
            ("users/get.yml", SearchField::Name, "Get user"),
            ("users/create.yml", SearchField::Name, "Create user"),
            ("users/get.yml", SearchField::Url, "{{base}}/users/{{id}}"),
            ("users/create.yml", SearchField::Url, "{{base}}/users"),
            ("ping.yml", SearchField::Header, "X-User: ada"),
        ]
    );
    assert_eq!(hits[0].method, "GET");
    assert_eq!(hits[1].method, "POST");
    assert_eq!(hits[0].name, "Get user");
    assert_eq!(hits[0].ranges, [(4, 8)]);
    assert_eq!(hits[4].ranges, [(2, 6)]);
}

#[test]
fn is_case_insensitive() {
    let ws = workspace();
    let hits = search(&ws, "PING", 50);
    assert_eq!(
        short(&hits),
        [
            ("ping.yml", SearchField::Name, "Ping"),
            ("ping.yml", SearchField::Url, "{{base}}/ping"),
        ]
    );
    assert_eq!(highlighted(&hits[0]), ["Ping"]);
}

#[test]
fn searches_every_field_and_includes_unsaved_changes() {
    let mut ws = workspace();
    let mut spec = ws.request("users/create.yml").unwrap();
    spec.params = vec![Param {
        name: "needle".into(),
        value: "1".into(),
        kind: ParamKind::Query,
        enabled: true,
    }];
    spec.headers = vec![KeyValue::new("X-Trace", "needle-header")];
    spec.body = Body::Raw {
        kind: RawKind::Json,
        data: "{\n  \"name\": \"Ada\",\n    \"note\": \"a needle in the body\"\n}".into(),
    };
    spec.auth = Auth::Bearer {
        token: "{{needleToken}}".into(),
    };
    spec.vars = vec![
        Variable::new("needleVar", "x"),
        Variable {
            name: "needleSecret".into(),
            value: Some("needle-secret-value".into()),
            enabled: true,
            secret: true,
        },
    ];
    // Not saved: search sees the workspace as edited.
    ws.update_request("users/create.yml", spec).unwrap();

    let hits = search(&ws, "needle", 50);
    assert_eq!(
        short(&hits),
        [
            ("users/create.yml", SearchField::Param, "needle=1"),
            (
                "users/create.yml",
                SearchField::Header,
                "X-Trace: needle-header"
            ),
            (
                "users/create.yml",
                SearchField::Body,
                "\"note\": \"a needle in the body\""
            ),
            (
                "users/create.yml",
                SearchField::Auth,
                "token: {{needleToken}}"
            ),
            ("users/create.yml", SearchField::Variable, "needleVar=x"),
            ("users/create.yml", SearchField::Variable, "needleSecret"),
        ]
    );
    // The body hit is the matching line, leading whitespace dropped.
    assert_eq!(highlighted(&hits[2]), ["needle"]);
    // A secret variable matches by name only; its value is never searched or shown.
    assert!(search(&ws, "secret-value", 50).is_empty());
    for hit in &hits {
        assert!(!hit.snippet.contains("needle-secret-value"));
    }
}

#[test]
fn searches_form_fields_multipart_text_parts_and_auth_kinds() {
    let mut ws = workspace();
    let mut get = ws.request("users/get.yml").unwrap();
    get.body = Body::FormUrlEncoded {
        fields: vec![KeyValue::new("q", "findme"), KeyValue::new("x", "other")],
    };
    get.auth = Auth::Basic {
        username: "findme-user".into(),
        password: "findme-password".into(),
    };
    ws.update_request("users/get.yml", get).unwrap();
    let mut create = ws.request("users/create.yml").unwrap();
    create.body = Body::Multipart {
        parts: vec![
            MultipartPart {
                name: "comment".into(),
                kind: PartKind::Text,
                values: vec!["please findme".into()],
                content_type: None,
                enabled: true,
            },
            MultipartPart {
                name: "upload".into(),
                kind: PartKind::File,
                values: vec!["files/findme.bin".into()],
                content_type: None,
                enabled: true,
            },
        ],
    };
    create.auth = Auth::ApiKey {
        key: "X-Api-Key".into(),
        value: "findme-key".into(),
        placement: ApiKeyPlacement::Header,
    };
    ws.update_request("users/create.yml", create).unwrap();

    let hits = search(&ws, "findme", 50);
    assert_eq!(
        short(&hits),
        [
            ("users/get.yml", SearchField::Body, "q=findme"),
            ("users/get.yml", SearchField::Auth, "username: findme-user"),
            (
                "users/create.yml",
                SearchField::Body,
                "comment=please findme"
            ),
            (
                "users/create.yml",
                SearchField::Auth,
                "X-Api-Key: findme-key"
            ),
        ]
    );
    // The Basic password is not searched.
    assert!(search(&ws, "password", 50).is_empty());
}

#[test]
fn ranges_are_utf16_offsets_with_non_ascii_and_emoji() {
    let mut ws = workspace();
    let mut spec = ws.request("ping.yml").unwrap();
    spec.name = "🚀 Grüße an Ärzte 🚀 ÄRZTE".into();
    ws.update_request("ping.yml", spec).unwrap();

    let hits = search(&ws, "ärzte", 50);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.snippet, "🚀 Grüße an Ärzte 🚀 ÄRZTE");
    // 🚀 is two UTF-16 units: "🚀 Grüße an " is 12 units long.
    assert_eq!(hit.ranges, [(12, 17), (21, 26)]);
    assert_eq!(highlighted(hit), ["Ärzte", "ÄRZTE"]);

    let hits = search(&ws, "🚀 g", 50);
    assert_eq!(hits[0].ranges, [(0, 4)]);
    assert_eq!(highlighted(&hits[0]), ["🚀 G"]);
}

#[test]
fn long_text_is_cut_around_the_first_match() {
    let mut ws = workspace();
    let mut spec = ws.request("ping.yml").unwrap();
    let long = format!("{}MATCH{}", "é".repeat(200), "x".repeat(200));
    spec.body = Body::Raw {
        kind: RawKind::Text,
        data: format!("first line\n{long}\nlast line match"),
    };
    ws.update_request("ping.yml", spec).unwrap();

    let hits = search(&ws, "match", 50);
    assert_eq!(hits.len(), 1, "one hit per body");
    let hit = &hits[0];
    assert!(hit.snippet.starts_with('…') && hit.snippet.ends_with('…'));
    assert_eq!(hit.snippet.chars().count(), SNIPPET_CHARS + 2);
    assert!(!hit.snippet.contains('\n'));
    assert_eq!(highlighted(hit), ["MATCH"]);
    // 24 characters of context before the match, after the leading "…".
    assert_eq!(hit.ranges, [(25, 30)]);
}

#[test]
fn respects_the_limit_and_the_maximum() {
    let mut list = vec![(COLLECTION_FILE, "info:\n  name: Many\n".to_owned())];
    let names: Vec<String> = (0..300).map(|i| format!("r{i:03}.yml")).collect();
    for (i, path) in names.iter().enumerate() {
        list.push((
            path.as_str(),
            request(&format!("Item {i}"), i as u32 + 1, "GET", "http://x/item"),
        ));
    }
    let ws = Workspace::open(files(&list)).unwrap();
    let hits = search(&ws, "item", 5);
    assert_eq!(hits.len(), 5);
    assert!(hits.iter().all(|h| h.field == SearchField::Name));
    assert_eq!(hits[0].path, "r000.yml");
    assert_eq!(search(&ws, "item", 10_000).len(), SEARCH_LIMIT);
}

/// 2,000 requests with headers, params and a JSON body each (not ASCII-only).
fn big_workspace() -> Workspace {
    let mut list = vec![(
        COLLECTION_FILE.to_owned(),
        "info:\n  name: Big\n".to_owned(),
    )];
    for i in 0..2_000 {
        let body = (0..20)
            .map(|k| format!("      \"field{k}\": \"value {i} {k} lorem ipsum dolor, Grüße\""))
            .collect::<Vec<_>>()
            .join(",\n");
        list.push((
            format!("f{}/r{i}.yml", i % 20),
            format!(
                "info:\n  name: Request number {i}\n  type: http\n  seq: {i}\n\nhttp:\n  method: POST\n  url: \"{{{{base}}}}/api/v1/items/{i}?page=1\"\n  params:\n    - name: page\n      value: \"1\"\n      type: query\n  headers:\n    - name: Accept\n      value: application/json\n    - name: X-Request\n      value: \"req-{i}\"\n  body:\n    type: json\n    data: |-\n      {{\n{body}\n      }}\n",
            ),
        ));
    }
    let list: Vec<(&str, String)> = list.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    Workspace::open(files(&list)).unwrap()
}

#[test]
fn fast_on_2000_requests() {
    let ws = big_workspace();
    // Sanity: the requests and their bodies are there.
    assert_eq!(search(&ws, "request number 1999", 10).len(), 1);
    assert_eq!(
        search(&ws, "value 1999 19 lorem", 10)[0].field,
        SearchField::Body
    );

    // Worst cases: a word in no request (every field of every request is read) and a word
    // in every body (the snippet of every body is built).
    let bound_ms = if cfg!(debug_assertions) { 500.0 } else { 50.0 };
    for query in ["zzz-not-there", "dolor", "ÄÖÜ"] {
        let started = std::time::Instant::now();
        let hits = search(&ws, query, SEARCH_LIMIT);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "search {query:?} over 2,000 requests: {ms:.1} ms, {} hits",
            hits.len()
        );
        assert!(
            ms < bound_ms,
            "search {query:?} took {ms:.1} ms (bound {bound_ms} ms)"
        );
    }
}

#[test]
fn greek_final_sigma_matches_either_way() {
    let mut ws = workspace();
    let mut spec = ws.request("ping.yml").unwrap();
    spec.name = "ΟΔΟΣ οδος".into();
    ws.update_request("ping.yml", spec).unwrap();
    let hits = search(&ws, "ΟΔΟΣ", 50);
    assert_eq!(hits.len(), 1);
    assert_eq!(highlighted(&hits[0]), ["ΟΔΟΣ", "οδος"]);
    assert_eq!(highlighted(&search(&ws, "δοσ", 50)[0]), ["ΔΟΣ", "δος"]);
}
