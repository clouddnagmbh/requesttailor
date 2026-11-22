// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand D, `prepare`: `sap-client` and `sap-language` from the environment, the sandbox
//! header rules.

use rt_plugin_odata::{MANIFEST, OData, landscape};
use rt_plugin_sdk::api::{Call, PluginContext, PluginRequest, PrepareReply};
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{NoteLevel, info};

const V2: &str = "https://s4d.example.corp:44300/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet";
const V4: &str = "https://s4d.example.corp/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/SalesOrder";

fn context(vars: &[(&str, &str)]) -> PluginContext {
    PluginContext {
        request_path: "sales/list.yml".into(),
        environment: Some("S4H DEV".into()),
        variables: vars
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn prepare(request: PluginRequest, vars: &[(&str, &str)]) -> PrepareReply {
    landscape::prepare(&request, &context(vars)).unwrap()
}

fn url(reply: &PrepareReply) -> Option<&str> {
    reply.patch.url.as_deref()
}

const SAP: &[(&str, &str)] = &[("sapClient", "100"), ("sapLanguage", "EN")];

#[test]
fn v2_service_gets_client_and_language() {
    let reply = prepare(testing::request("GET", V2), SAP);
    assert_eq!(
        url(&reply),
        Some(&*format!("{V2}?sap-client=100&sap-language=EN"))
    );
    assert_eq!(
        reply.notes,
        vec![info(
            "SAP: added sap-client=100, sap-language=EN (environment S4H DEV)"
        )]
    );
    assert!(reply.patch.set_headers.is_empty() && reply.patch.remove_headers.is_empty());
}

#[test]
fn v4_service_keeps_its_query_and_fragment() {
    let reply = prepare(testing::request("GET", &format!("{V4}?$top=5#x")), SAP);
    assert_eq!(
        url(&reply),
        Some(&*format!("{V4}?$top=5&sap-client=100&sap-language=EN#x"))
    );
}

#[test]
fn present_parameters_win_and_are_never_duplicated() {
    // sap-client in the request (any case): only the language is added.
    let reply = prepare(
        testing::request("GET", &format!("{V2}?SAP-Client=200")),
        SAP,
    );
    assert_eq!(
        url(&reply),
        Some(&*format!("{V2}?SAP-Client=200&sap-language=EN"))
    );
    // Both present: nothing to do, no note.
    let both = format!("{V2}?sap-language=DE&sap-client=200");
    let reply = prepare(testing::request("GET", &both), SAP);
    assert_eq!(reply, PrepareReply::default());
    // Running again on the patched URL changes nothing (idempotent).
    let first = prepare(testing::request("GET", V2), SAP);
    let again = prepare(testing::request("GET", url(&first).unwrap()), SAP);
    assert_eq!(again, PrepareReply::default());
}

#[test]
fn only_what_is_set_is_added() {
    let reply = prepare(testing::request("GET", V2), &[("sapLanguage", "DE")]);
    assert_eq!(url(&reply), Some(&*format!("{V2}?sap-language=DE")));
    let reply = prepare(testing::request("GET", V2), &[("sapClient", "  ")]);
    assert_eq!(reply, PrepareReply::default());
    // An SAP service without SAP variables: nothing.
    let reply = prepare(testing::request("GET", V2), &[("baseUrl", "https://x")]);
    assert_eq!(reply, PrepareReply::default());
}

#[test]
fn other_urls_only_with_sap_client_in_the_environment() {
    let cap = "http://localhost:4004/odata/v4/catalog/Books";
    // Not an SAP Gateway URL and no sapClient: no-op, even with a language.
    let reply = prepare(testing::request("GET", cap), &[("sapLanguage", "EN")]);
    assert_eq!(reply, PrepareReply::default());
    let reply = prepare(
        testing::request("GET", "https://api.example.com/users"),
        &[],
    );
    assert_eq!(reply, PrepareReply::default());
    // With sapClient the environment says "SAP system": any URL.
    let reply = prepare(testing::request("GET", cap), SAP);
    assert_eq!(
        url(&reply),
        Some("http://localhost:4004/odata/v4/catalog/Books?sap-client=100&sap-language=EN")
    );
}

#[test]
fn masked_or_unresolved_values_are_not_copied() {
    let reply = prepare(
        testing::request("GET", V2),
        &[("sapClient", "***"), ("sapLanguage", "{{lang}}")],
    );
    assert_eq!(reply.patch.url, None);
    assert_eq!(reply.notes.len(), 2);
    assert!(reply.notes.iter().all(|n| n.level == NoteLevel::Warning));
    assert!(!format!("{:?}", reply.patch).contains("***"));

    // A secret in the URL is masked: patching the URL would send the mask.
    let reply = prepare(testing::request("GET", &format!("{V2}?token=***")), SAP);
    assert_eq!(reply.patch.url, None);
    assert_eq!(reply.notes.len(), 1);
    assert_eq!(reply.notes[0].level, NoteLevel::Warning);
    assert!(
        reply.notes[0]
            .text
            .contains("sap-client=100, sap-language=EN not added")
    );
}

#[test]
fn values_are_encoded() {
    let reply = prepare(testing::request("GET", V2), &[("sapClient", "1 0&")]);
    assert_eq!(url(&reply), Some(&*format!("{V2}?sap-client=1%200%26")));
}

#[test]
fn api_key_header_without_value_is_dropped() {
    let mut request = testing::request("GET", V2);
    request.headers.push(("APIKey".into(), "{{apiKey}}".into()));
    let reply = prepare(request, SAP);
    assert_eq!(reply.patch.remove_headers, vec!["APIKey".to_owned()]);
    // A key that is set (masked as `***`) stays.
    let mut request = testing::request("GET", V2);
    request.headers.push(("apikey".into(), "***".into()));
    let reply = prepare(request, SAP);
    assert!(reply.patch.remove_headers.is_empty());
}

#[test]
fn sandbox_sends_no_authorization() {
    let sandbox =
        "https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_RT_PRODUCT_SRV/A_RtProduct";
    let mut request = testing::request("GET", sandbox);
    request.headers.push(("Authorization".into(), "***".into()));
    request.headers.push(("APIKey".into(), "***".into()));
    let reply = prepare(request.clone(), &[("sapSandbox", "true")]);
    assert_eq!(reply.patch.remove_headers, vec!["Authorization".to_owned()]);
    assert_eq!(reply.patch.url, None);
    // Without the sandbox marker the auth stays.
    let reply = prepare(request, SAP);
    assert!(reply.patch.remove_headers.is_empty());
}

#[test]
fn through_the_plugin_dispatch() {
    let host = MockHost::new()
        .granted(rt_plugin_sdk::parse_manifest(MANIFEST).unwrap().permissions)
        .request_host("s4d.example.corp");
    let _guard = host.install();
    let reply: PrepareReply = testing::call(
        &OData,
        &Call::Prepare {
            request: testing::request("GET", V2),
            context: context(SAP),
        },
    )
    .unwrap();
    assert_eq!(
        url(&reply),
        Some(&*format!("{V2}?sap-client=100&sap-language=EN"))
    );
    assert!(reply.notes.contains(&info(
        "SAP: added sap-client=100, sap-language=EN (environment S4H DEV)"
    )));
}
