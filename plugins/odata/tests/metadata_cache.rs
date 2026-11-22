// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The metadata cache (storage format strands B, C and D rely on) and the `response` hook
//! that fills it from `$metadata` responses.

use rt_plugin_odata::OData;
use rt_plugin_odata::metadata::{self, cached, forget, known_roots, parse, store};
use rt_plugin_odata::service::storage_key;
use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{Call, NotesReply, Permissions, PluginResponse};
use rt_plugin_sdk::testing::{self, MockHost};

const S4_V2: &str = include_str!("../testdata/s4/ZRT_SALES_SRV.edmx");
const CAP_V4: &str = include_str!("../testdata/cap-bookshop/admin-v4.edmx");

const V2_ROOT: &str = "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/";
const V4_ROOT: &str = "http://localhost:4004/odata/v4/admin/";

fn storage_host() -> MockHost {
    MockHost::new().granted(Permissions {
        storage: true,
        ..Permissions::default()
    })
}

#[test]
fn store_writes_the_documented_format() {
    let host = storage_host();
    let _guard = host.install();
    let v2 = parse(S4_V2, V2_ROOT).unwrap();
    let v4 = parse(CAP_V4, V4_ROOT).unwrap();
    assert_eq!(known_roots().unwrap(), Vec::<String>::new());
    assert_eq!(cached(V2_ROOT).unwrap(), None);

    store(&v2).unwrap();
    store(&v4).unwrap();
    store(&v2).unwrap(); // again: the root is listed once

    let storage = host.storage();
    assert_eq!(
        storage[&storage_key("metadata", V2_ROOT)],
        serde_json::to_value(&v2).unwrap()
    );
    assert_eq!(
        storage[&storage_key("metadata", V4_ROOT)],
        serde_json::to_value(&v4).unwrap()
    );
    assert_eq!(
        storage["metadata-roots"],
        serde_json::json!([V2_ROOT, V4_ROOT])
    );
    assert_eq!(metadata::ROOTS_KEY, "metadata-roots");
    assert_eq!(storage.len(), 3);

    assert_eq!(cached(V2_ROOT).unwrap(), Some(v2));
    assert_eq!(known_roots().unwrap(), vec![V2_ROOT, V4_ROOT]);

    forget(V2_ROOT).unwrap();
    assert_eq!(cached(V2_ROOT).unwrap(), None);
    assert_eq!(known_roots().unwrap(), vec![V4_ROOT]);
    assert_eq!(host.storage().len(), 2);
}

#[test]
fn the_cache_reads_what_others_wrote() {
    // A model written by another plugin version or strand, as plain JSON.
    let host = storage_host();
    let _guard = host.install();
    let model = parse(CAP_V4, V4_ROOT).unwrap();
    rt_plugin_sdk::host::storage_set(
        storage_key("metadata", V4_ROOT),
        serde_json::to_value(&model).unwrap(),
    )
    .unwrap();
    rt_plugin_sdk::host::storage_set("metadata-roots", serde_json::json!([V4_ROOT, 7])).unwrap();
    assert_eq!(known_roots().unwrap(), vec![V4_ROOT]);
    assert_eq!(cached(V4_ROOT).unwrap(), Some(model));

    rt_plugin_sdk::host::storage_set(storage_key("metadata", V2_ROOT), serde_json::json!(1))
        .unwrap();
    assert!(cached(V2_ROOT).is_err());
}

fn xml_response(status: u16, body: &str) -> PluginResponse {
    testing::response(status, "application/xml;charset=utf-8", body)
}

fn run_response(url: &str, response: PluginResponse) -> NotesReply {
    metadata::response(
        &testing::request("GET", url),
        &response,
        &testing::context("metadata.yml"),
    )
    .expect("the hook never fails")
}

#[test]
fn the_plugin_runs_the_hook() {
    let host = storage_host();
    let _guard = host.install();
    let reply: NotesReply = testing::call(
        &OData,
        &Call::Response {
            request: testing::request("GET", &format!("{V4_ROOT}$metadata")),
            response: xml_response(200, CAP_V4),
            context: testing::context("metadata.yml"),
        },
    )
    .unwrap();
    assert!(
        reply
            .notes
            .iter()
            .any(|n| n.text == "OData V4 metadata cached: 4 entity sets"),
        "{:?}",
        reply.notes
    );
    assert_eq!(known_roots().unwrap(), vec![V4_ROOT]);
}

#[test]
fn metadata_responses_are_cached() {
    let host = storage_host();
    let _guard = host.install();
    let reply = run_response(
        &format!("{V2_ROOT}$metadata?sap-client=100&sap-language=EN"),
        xml_response(200, S4_V2),
    );
    assert_eq!(reply.notes.len(), 1);
    assert_eq!(reply.notes[0].level, NoteLevel::Info);
    assert_eq!(
        reply.notes[0].text,
        "OData V2 metadata cached: 3 entity sets"
    );
    let model = cached(V2_ROOT).unwrap().unwrap();
    assert_eq!(model.service_root, V2_ROOT);
    assert_eq!(model, parse(S4_V2, V2_ROOT).unwrap());

    let reply = run_response(&format!("{V4_ROOT}$metadata"), xml_response(200, CAP_V4));
    assert_eq!(
        reply.notes[0].text,
        "OData V4 metadata cached: 4 entity sets"
    );
    assert_eq!(known_roots().unwrap(), vec![V2_ROOT, V4_ROOT]);
}

#[test]
fn other_responses_are_left_alone() {
    let host = storage_host();
    let _guard = host.install();
    let quiet = |url: &str, response: PluginResponse| {
        assert_eq!(run_response(url, response).notes, vec![], "{url}");
    };
    quiet(&format!("{V2_ROOT}SalesOrderSet"), xml_response(200, S4_V2));
    quiet(&format!("{V2_ROOT}$metadata"), xml_response(401, "<html/>"));
    quiet(
        &format!("{V4_ROOT}$metadata?$format=json"),
        testing::response(200, "application/json", "{\"$Version\":\"4.0\"}"),
    );
    assert!(host.storage().is_empty());
}

#[test]
fn problems_become_warnings_never_errors() {
    let host = storage_host();
    let _guard = host.install();
    let reply = run_response(
        &format!("{V2_ROOT}$metadata"),
        xml_response(200, "<edmx:Edmx xmlns:edmx='x' Version='1.0'><broken>"),
    );
    assert_eq!(reply.notes[0].level, NoteLevel::Warning);
    assert!(
        reply.notes[0]
            .text
            .starts_with("OData metadata could not be read:"),
        "{}",
        reply.notes[0].text
    );

    let mut cut = xml_response(200, &S4_V2[..2000]);
    cut.truncated = true;
    let reply = run_response(&format!("{V2_ROOT}$metadata"), cut);
    assert_eq!(reply.notes[0].level, NoteLevel::Warning);
    assert!(reply.notes[0].text.contains("cut"));
    assert!(host.storage().is_empty());

    // Without storage the send still succeeds, with a warning.
    let no_storage = MockHost::new();
    let _guard = no_storage.install();
    let reply = run_response(&format!("{V2_ROOT}$metadata"), xml_response(200, S4_V2));
    assert_eq!(reply.notes[0].level, NoteLevel::Warning);
    assert!(
        reply.notes[0].text.contains("permission denied"),
        "{}",
        reply.notes[0].text
    );
}

#[test]
fn base64_bodies_are_read_too() {
    let host = storage_host();
    let _guard = host.install();
    let mut response = xml_response(200, "");
    response.body.text = None;
    response.body.base64 = Some(rt_plugin_sdk::base64_encode(CAP_V4.as_bytes()));
    let reply = run_response(&format!("{V4_ROOT}$metadata"), response);
    assert_eq!(reply.notes[0].level, NoteLevel::Info, "{:?}", reply.notes);
}
