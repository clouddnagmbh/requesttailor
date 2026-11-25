// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate of wave 18, strand J: Gateway errors (V2 JSON and XML, V4 JSON) with transaction ID,
//! lookup hint and note, and `sap-statistics` as time segments. The formats are golden files in
//! `testdata/gateway/` (synthetic; format and unit of the header are not verified, see
//! `testdata/SOURCES.md`); the hooks run through the plugin's JSON dispatch like the host does,
//! and the last tests send against the OData test server.

use std::fs;
use std::path::PathBuf;

use rt_odata_testserver::client::{self, TestServer};
use rt_plugin_odata::OData;
use rt_plugin_odata::gateway::{error, statistics};
use rt_plugin_sdk::api::{
    Block, Call, NotesReply, PluginBody, PluginNote, PluginRequest, PluginResponse, PrepareReply,
    TabTarget, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{NoteLevel, header};
use serde_json::{Value, json};

fn golden(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/gateway")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn golden_json(name: &str) -> Value {
    serde_json::from_str(&golden(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// -------------------------------------------------------------------------------------------
// The readers

#[test]
fn errors_of_every_layout_are_read_into_the_same_shape() {
    let cases = [
        ("v2-error.json", "application/json", "v2-error.golden.json"),
        (
            "v2-error.xml",
            "application/xml",
            "v2-error-xml.golden.json",
        ),
        (
            "v4-error.json",
            "application/json;odata.metadata=minimal",
            "v4-error.golden.json",
        ),
        (
            "cap-error.json",
            "application/json",
            "cap-error.golden.json",
        ),
    ];
    for (file, content_type, expected) in cases {
        let found = error::parse(Some(content_type), &golden(file))
            .unwrap_or_else(|| panic!("{file}: no error found"));
        assert_eq!(
            serde_json::to_value(&found).unwrap(),
            golden_json(expected),
            "{file}"
        );
    }
}

#[test]
fn what_is_not_an_odata_error_is_not_read_as_one() {
    let html = golden("html-error.html");
    assert!(error::parse(Some("text/html"), &html).is_none());
    assert!(error::parse(Some("application/json"), "{}").is_none());
    assert!(error::parse(Some("application/json"), r#"{"value":[]}"#).is_none());
    assert!(error::parse(Some("application/json"), r#"{"error":"flat text"}"#).is_none());
    assert!(error::parse(Some("application/json"), "{not json").is_none());
    assert!(error::parse(Some("application/xml"), "<feed/>").is_none());
    assert!(error::parse(None, "").is_none());
}

#[test]
fn the_time_is_readable_and_the_summary_names_the_transaction() {
    let e = error::parse(Some("application/json"), &golden("v2-error.json")).unwrap();
    assert_eq!(e.time().as_deref(), Some("2026-11-24 10:30:15 UTC"));
    assert_eq!(
        e.summary(),
        "OData error ZRT_SALES/004: Sales order 4711 is locked by user RT_USER2 \
         (transaction ID 5F3A9C1E2B7D4E0A9C1D000000000001)"
    );
}

#[test]
fn statistics_golden_files() {
    for name in [
        "basic", "units", "micro", "seconds", "unknown", "no-total", "over", "garbage",
    ] {
        let found = statistics::parse(golden(&format!("stats-{name}.header")).trim());
        let got = json!({
            "segments": found.segments.iter().map(|s| json!([s.id, s.ms])).collect::<Vec<_>>(),
            "unknown": found.unknown,
        });
        assert_eq!(
            got,
            golden_json(&format!("stats-{name}.golden.json")),
            "{name}"
        );
        // The segments sit inside the server's wait and have names a person reads.
        for s in &found.segments {
            assert_eq!(s.parent.as_deref(), Some("server"), "{name}/{}", s.id);
            assert!(!s.label.is_empty());
        }
    }
}

#[test]
fn the_rest_is_the_total_minus_the_parts_and_never_negative() {
    let basic = statistics::parse("total=48,fw=7,app=21,db=9");
    let parts: u64 = basic
        .segments
        .iter()
        .filter(|s| s.id != "rest")
        .map(|s| s.ms)
        .sum();
    assert_eq!(basic.segments.last().unwrap().ms, 48 - parts);
    let over = statistics::parse("total=10, fw=5, app=20");
    assert!(over.segments.iter().all(|s| s.id != "rest"));
}

// -------------------------------------------------------------------------------------------
// The hooks

fn host() -> (MockHost, rt_plugin_sdk::testing::HostGuard) {
    let manifest = rt_plugin_sdk::parse_manifest(rt_plugin_odata::MANIFEST).unwrap();
    let host = MockHost::new()
        .granted(manifest.permissions)
        .request_host("s4.example.com");
    let guard = host.install();
    (host, guard)
}

const SERVICE: &str = "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/";

fn get(url: &str, headers: &[(&str, &str)]) -> PluginRequest {
    let mut r = testing::request("GET", url);
    r.headers = headers
        .iter()
        .map(|(n, v)| ((*n).into(), (*v).into()))
        .collect();
    r
}

fn answer(status: u16, content_type: &str, text: &str, headers: &[(&str, &str)]) -> PluginResponse {
    let mut r = testing::response(status, content_type, text);
    r.headers.extend(
        headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned())),
    );
    r
}

fn prepare(request: PluginRequest) -> PrepareReply {
    testing::call(
        &OData,
        &Call::Prepare {
            request,
            context: testing::context("orders/list.yml"),
        },
    )
    .expect("prepare")
}

fn response_hook(request: PluginRequest, response: PluginResponse) -> NotesReply {
    testing::call(
        &OData,
        &Call::Response {
            request,
            response,
            context: testing::context("orders/list.yml"),
        },
    )
    .expect("response")
}

fn spec(url: &str) -> RequestSpec {
    RequestSpec {
        url: url.into(),
        ..RequestSpec::default()
    }
}

fn error_tab(response: Option<PluginResponse>) -> ViewReply {
    testing::call(
        &OData,
        &Call::RenderTab {
            tab: "error".into(),
            target: TabTarget {
                request_path: "orders/list.yml".into(),
                response_id: Some(1),
            },
            spec: spec(SERVICE),
            response,
            state: None,
            context: None,
        },
    )
    .expect("render error tab")
}

fn statistics_tab(action: Option<(&str, &str)>) -> ViewReply {
    let target = TabTarget {
        request_path: "orders/list.yml".into(),
        response_id: None,
    };
    match action {
        None => testing::call(
            &OData,
            &Call::RenderTab {
                tab: "statistics".into(),
                target,
                spec: spec(&format!("{SERVICE}SalesOrderSet")),
                response: None,
                state: None,
                context: None,
            },
        ),
        Some((id, value)) => testing::call(
            &OData,
            &Call::Action {
                tab: "statistics".into(),
                action: id.into(),
                target,
                spec: spec(&format!("{SERVICE}SalesOrderSet")),
                response: None,
                state: None,
                input: vec![(id.into(), value.into())],
                context: None,
            },
        ),
    }
    .expect("statistics tab")
}

fn text_of(view: &ViewReply) -> String {
    let mut out = String::new();
    for block in &view.view.blocks {
        match block {
            Block::Heading { text } | Block::Text { text } | Block::Code { text, .. } => {
                out.push_str(text)
            }
            Block::Notice { text, .. } => out.push_str(text),
            Block::KeyValue { rows } => {
                for (k, v) in rows {
                    out.push_str(&format!("{k}: {v}\n"));
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    out.push_str(&row.join(" | "));
                    out.push('\n');
                }
            }
            _ => {}
        }
        out.push('\n');
    }
    out
}

fn notes_text(notes: &[PluginNote]) -> String {
    notes
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_v2_json_error_becomes_a_note_and_a_tab_with_the_transaction_id() {
    let (_host, _guard) = host();
    let r = answer(400, "application/json", &golden("v2-error.json"), &[]);
    let reply = response_hook(get(SERVICE, &[]), r.clone());
    assert_eq!(reply.notes.len(), 1);
    assert_eq!(reply.notes[0].level, NoteLevel::Warning);
    assert!(reply.notes[0].text.contains("ZRT_SALES/004"));
    assert!(reply.notes[0].text.contains("locked by user RT_USER2"));
    assert!(
        reply.notes[0]
            .text
            .contains("5F3A9C1E2B7D4E0A9C1D000000000001")
    );
    assert!(reply.segments.is_empty());

    let tab = error_tab(Some(r));
    let text = text_of(&tab);
    assert!(text.contains("Transaction ID: 5F3A9C1E2B7D4E0A9C1D000000000001"));
    assert!(text.contains("/IWFND/ERROR_LOG"));
    assert!(text.contains("/IWBEP/ERROR_LOG"));
    assert!(text.contains("2026-11-24 10:30:15 UTC"));
    // Details as a table, severity first.
    assert!(text.contains(
        "error | ZRT_SALES/004 | Sales order 4711 is locked by user RT_USER2 | SalesOrderID"
    ));
    assert!(text.contains("warning | ZRT_SALES/017 | Delivery date is in the past | DeliveryDate"));
    // What the system itself suggests.
    assert!(text.contains("SAP Note: See SAP Note 9999999 for details"));
    // The ID is a code block, so it has a Copy button.
    assert!(tab.view.blocks.iter().any(|b| matches!(
        b, Block::Code { text, .. } if text == "5F3A9C1E2B7D4E0A9C1D000000000001"
    )));
}

#[test]
fn xml_and_v4_errors_are_decoded_too() {
    let (_host, _guard) = host();
    let xml = answer(400, "application/xml", &golden("v2-error.xml"), &[]);
    let reply = response_hook(get(SERVICE, &[]), xml.clone());
    assert!(
        reply.notes[0]
            .text
            .contains("transaction ID 5F3A9C1E2B7D4E0A9C1D000000000001")
    );
    let text = text_of(&error_tab(Some(xml)));
    assert!(text.contains("Delivery date is in the past & must be corrected"));

    let v4 = answer(
        422,
        "application/json;odata.metadata=minimal",
        &golden("v4-error.json"),
        &[],
    );
    let url = "https://s4.example.com/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/SalesOrder";
    let reply = response_hook(get(url, &[]), v4.clone());
    assert!(reply.notes[0].text.contains("ZRT_SALESORDER/012"));
    let text = text_of(&error_tab(Some(v4)));
    assert!(text.contains("Transaction ID: 6A4B0D2F3C8E5F1BAD2E000000000002"));
    assert!(text.contains("Target: SoldToParty"));
    assert!(
        text.contains("warning | ZRT_SALESORDER/031 | Payment terms are missing | PaymentTerms")
    );
}

#[test]
fn an_error_without_a_transaction_id_has_no_lookup_hint() {
    let (_host, _guard) = host();
    let r = answer(404, "application/json", &golden("cap-error.json"), &[]);
    let reply = response_hook(get(SERVICE, &[]), r.clone());
    assert_eq!(notes_text(&reply.notes), "OData error 404: Not Found");
    let text = text_of(&error_tab(Some(r)));
    assert!(text.contains("Not Found"));
    assert!(!text.contains("ERROR_LOG"));
}

#[test]
fn no_note_and_no_details_for_answers_that_are_no_errors() {
    let (_host, _guard) = host();
    // 200 with a body that looks like an error: not read, the status says it worked.
    let ok = answer(200, "application/json", &golden("v2-error.json"), &[]);
    assert!(
        response_hook(get(SERVICE, &[]), ok.clone())
            .notes
            .is_empty()
    );
    assert!(text_of(&error_tab(Some(ok))).contains("not an error"));
    // 500 with HTML.
    let html = answer(500, "text/html", &golden("html-error.html"), &[]);
    assert!(
        response_hook(get(SERVICE, &[]), html.clone())
            .notes
            .is_empty()
    );
    assert!(text_of(&error_tab(Some(html))).contains("not an OData error"));
    assert!(text_of(&error_tab(None)).contains("Send the request"));
}

#[test]
fn a_binary_body_is_read_as_text() {
    let (_host, _guard) = host();
    let mut r = answer(400, "application/json", "", &[]);
    r.body = PluginBody {
        content_type: Some("application/json".into()),
        text: None,
        base64: Some(rt_plugin_sdk::base64_encode(
            golden("v2-error.json").as_bytes(),
        )),
    };
    assert!(
        response_hook(get(SERVICE, &[]), r).notes[0]
            .text
            .contains("ZRT_SALES/004")
    );
}

#[test]
fn the_header_goes_out_only_when_the_user_switched_it_on_for_that_service() {
    let (host, _guard) = host();
    let url = format!("{SERVICE}SalesOrderSet");
    // Off by default: nothing is set.
    let off = prepare(get(&url, &[]));
    assert!(off.patch.set_headers.is_empty());
    assert!(off.notes.is_empty());

    // The tab shows the switch off, then the user turns it on for this service.
    let view = statistics_tab(None);
    assert!(view.view.blocks.iter().any(|b| matches!(
        b, Block::Form { id, fields, .. } if id == "measure" && fields[0].value == "false"
    )));
    let saved = statistics_tab(Some(("measure", "true")));
    assert!(text_of(&saved).contains("the next send asks the server"));
    assert!(saved.view.blocks.iter().any(|b| matches!(
        b, Block::Form { fields, .. } if fields[0].value == "true"
    )));

    let on = prepare(get(&url, &[]));
    assert_eq!(
        on.patch.set_headers,
        vec![("sap-statistics".into(), "true".into())]
    );
    assert!(notes_text(&on.notes).contains("sap-statistics: true"));

    // Another service of the same host is unaffected.
    let other = prepare(get("https://s4.example.com/odata/v4/admin/Books", &[]));
    assert!(other.patch.set_headers.is_empty());
    // A header the user wrote stays as it is.
    let own = prepare(get(&url, &[("sap-statistics", "false")]));
    assert!(own.patch.set_headers.is_empty());

    // Off again: the entry is gone from the storage, not set to false.
    statistics_tab(Some(("measure", "false")));
    assert!(prepare(get(&url, &[])).patch.set_headers.is_empty());
    assert!(host.storage().keys().all(|k| !k.starts_with("statistics:")));
}

#[test]
fn the_header_of_the_answer_becomes_segments_and_unknown_parts_a_note() {
    let (_host, _guard) = host();
    let r = answer(
        200,
        "application/json",
        "{}",
        &[("sap-statistics", "total=48,fw=7,app=21,db=9,xyz=3")],
    );
    let reply = response_hook(get(SERVICE, &[]), r);
    assert_eq!(
        reply
            .segments
            .iter()
            .map(|s| (s.id.as_str(), s.ms))
            .collect::<Vec<_>>(),
        vec![
            ("gateway", 7),
            ("application", 21),
            ("database", 9),
            ("rest", 11)
        ]
    );
    assert_eq!(notes_text(&reply.notes), "sap-statistics: ignored xyz=3");

    // Nothing understood: a note, no segments, no failure.
    let garbage = answer(
        200,
        "application/json",
        "{}",
        &[("sap-statistics", "hello world")],
    );
    let reply = response_hook(get(SERVICE, &[]), garbage);
    assert!(reply.segments.is_empty());
    assert_eq!(
        notes_text(&reply.notes),
        "sap-statistics: not understood: hello world"
    );

    // No header: nothing.
    let plain = answer(200, "application/json", "{}", &[]);
    let reply = response_hook(get(SERVICE, &[]), plain);
    assert!(reply.segments.is_empty() && reply.notes.is_empty());
}

// -------------------------------------------------------------------------------------------
// Against the test server

const GW: &str = "/sap/opu/odata/sap/ZRT_SALES_SRV/";
const RAP: &str = "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";

fn over_the_wire(server: &TestServer, path: &str, headers: &[(String, String)]) -> PluginResponse {
    let mut all = headers.to_vec();
    all.push(("Authorization".into(), client::s4_basic_auth()));
    let r = client::send("GET", &server.url(path), &all, b"").expect("server reachable");
    PluginResponse {
        status: r.status,
        url: server.url(path),
        body: PluginBody {
            content_type: r.header("Content-Type").map(str::to_owned),
            text: Some(r.text()),
            base64: None,
        },
        headers: r.headers,
        truncated: false,
    }
}

#[test]
fn the_switch_and_the_server_agree_end_to_end() {
    let server = TestServer::start();
    let manifest = rt_plugin_sdk::parse_manifest(rt_plugin_odata::MANIFEST).unwrap();
    let host = MockHost::new()
        .granted(manifest.permissions)
        .request_host("127.0.0.1");
    let _guard = host.install();
    let url = server.url(&format!("{GW}SalesOrderSet?$top=1"));

    // Switched off: the server does not report, there are no segments.
    let request = get(&url, &[]);
    let patch = prepare(request.clone()).patch;
    assert!(patch.set_headers.is_empty());
    let r = over_the_wire(&server, &format!("{GW}SalesOrderSet?$top=1"), &[]);
    assert!(header(&r.headers, "sap-statistics").is_none());
    assert!(response_hook(request, r).segments.is_empty());

    // Switched on: the header goes out, the server answers with its time.
    let tab = Call::Action {
        tab: "statistics".into(),
        action: "measure".into(),
        target: TabTarget {
            request_path: "a.yml".into(),
            response_id: None,
        },
        spec: spec(&url),
        response: None,
        state: None,
        input: vec![("measure".into(), "true".into())],
        context: None,
    };
    let _: ViewReply = testing::call(&OData, &tab).unwrap();
    let request = get(&url, &[]);
    let patch = prepare(request.clone()).patch;
    assert_eq!(
        patch.set_headers,
        vec![("sap-statistics".into(), "true".into())]
    );
    let r = over_the_wire(
        &server,
        &format!("{GW}SalesOrderSet?$top=1"),
        &patch.set_headers,
    );
    assert_eq!(
        header(&r.headers, "sap-statistics"),
        Some("total=48,fw=7,app=21,db=9")
    );
    let reply = response_hook(request, r);
    assert_eq!(
        reply.segments.iter().map(|s| s.ms).collect::<Vec<_>>(),
        vec![7, 21, 9, 11]
    );
}

#[test]
fn errors_of_the_test_server_decode_in_every_layout() {
    let server = TestServer::start();
    let manifest = rt_plugin_sdk::parse_manifest(rt_plugin_odata::MANIFEST).unwrap();
    let host = MockHost::new()
        .granted(manifest.permissions)
        .request_host("127.0.0.1");
    let _guard = host.install();
    let tx = "0000000000000000000000000000RT01";

    // V2 as JSON and as XML.
    for accept in ["application/json", "application/xml"] {
        let path = format!("{GW}SalesOrderSet('does-not-exist')");
        let r = over_the_wire(&server, &path, &[("Accept".into(), accept.into())]);
        assert!(r.status >= 400, "{accept}");
        let reply = response_hook(get(&server.url(&path), &[]), r.clone());
        assert!(
            reply.notes[0].text.contains(tx),
            "{accept}: {:?}",
            reply.notes
        );
        let text = text_of(&error_tab(Some(r)));
        assert!(text.contains(&format!("Transaction ID: {tx}")), "{accept}");
        assert!(text.contains("2026-11-24 10:30:15 UTC"), "{accept}");
        assert!(text.contains("/IWFND/ERROR_LOG"), "{accept}");
    }

    // V4.
    let path = format!("{RAP}SalesOrder(ID=0b7e2f0a-6c1d-4e5f-9a10-0000000000ff)");
    let r = over_the_wire(
        &server,
        &path,
        &[("Accept".into(), "application/json".into())],
    );
    assert!(r.status >= 400);
    let reply = response_hook(get(&server.url(&path), &[]), r.clone());
    assert!(reply.notes[0].text.contains(tx), "{:?}", reply.notes);
    assert!(text_of(&error_tab(Some(r))).contains(&format!("Transaction ID: {tx}")));
}
