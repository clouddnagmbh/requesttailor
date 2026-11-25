// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate of wave 18, strand J: the paging rules the OData plugin ships as data
//! (`contributes.paging`). The core's own matching (`rt_app::paging::rules::matches`) and
//! JSONPath (RFC 9535) read them: `@odata.nextLink` (V4) and `d.__next` (V2), with no
//! entry in the core. Fixtures are synthetic pages in the layouts of the test services.

use rt_app::paging::rules::matches;
use rt_app::paging::{MatchContext, PagingRule};
use rt_plugin_odata::paging;
use rt_plugin_sdk::api::PagingContribution;
use serde_json::{Value, json};
use serde_json_path::JsonPath;

fn manifest_rules() -> Vec<PagingContribution> {
    rt_plugin_sdk::parse_manifest(rt_plugin_odata::MANIFEST)
        .unwrap()
        .contributes
        .paging
}

/// The rule the core would offer for a response (the first that matches, as `effective`).
fn offered(url: &str, content_type: &str) -> Option<PagingContribution> {
    let context = MatchContext {
        url: url.into(),
        content_type: Some(content_type.into()),
    };
    manifest_rules()
        .into_iter()
        .find(|r| matches(&r.matches, &context))
}

fn text_at(path: &str, body: &Value) -> Option<String> {
    let found = JsonPath::parse(path).ok()?.query(body).all();
    found.first().and_then(|v| v.as_str()).map(str::to_owned)
}

fn array_at(path: &str, body: &Value) -> usize {
    let found = JsonPath::parse(path).unwrap().query(body).all();
    found
        .first()
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len())
}

fn next_of(rule: &PagingContribution, body: &Value) -> Option<String> {
    match &rule.rule.rule {
        PagingRule::JsonUrl { next } => text_at(next, body),
        other => panic!("unexpected rule {other:?}"),
    }
}

#[test]
fn the_manifest_and_the_module_agree() {
    let from_module = paging::contributions().paging;
    assert_eq!(manifest_rules(), from_module);
    assert_eq!(from_module.len(), 4);
}

#[test]
fn every_rule_is_valid_data() {
    for rule in manifest_rules() {
        let PagingRule::JsonUrl { next } = &rule.rule.rule else {
            panic!("{}: only json-url rules", rule.id);
        };
        JsonPath::parse(next).unwrap_or_else(|e| panic!("{}: {next}: {e}", rule.id));
        let merge = rule.rule.merge.as_deref().expect("a merge path");
        JsonPath::parse(merge).unwrap_or_else(|e| panic!("{}: {merge}: {e}", rule.id));
        // The plugin names no host: it follows the services of whichever system the user uses.
        assert!(rule.matches.hosts.is_empty(), "{}", rule.id);
        assert_eq!(rule.matches.content_types, vec!["application/json"]);
    }
}

#[test]
fn v4_follows_the_next_link_and_merges_value() {
    let page = json!({
        "@odata.context": "$metadata#SalesOrder",
        "value": [{ "ID": "1" }, { "ID": "2" }],
        "@odata.nextLink": "SalesOrder?$skiptoken=2"
    });
    for url in [
        "https://s4.example.com/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/SalesOrder",
        "http://localhost:4004/odata/v4/admin/Books?$top=2",
    ] {
        let rule = offered(url, "application/json;odata.metadata=minimal;charset=utf-8")
            .unwrap_or_else(|| panic!("no rule for {url}"));
        assert!(rule.id.starts_with("odata-v4"), "{url}: {}", rule.id);
        assert_eq!(
            next_of(&rule, &page).as_deref(),
            Some("SalesOrder?$skiptoken=2")
        );
        assert_eq!(array_at(rule.rule.merge.as_deref().unwrap(), &page), 2);
        // The last page has no link: the run ends.
        let last = json!({ "value": [{ "ID": "3" }] });
        assert_eq!(next_of(&rule, &last), None);
    }
}

#[test]
fn v2_follows_d_next_and_merges_d_results() {
    let page = json!({
        "d": {
            "results": [{ "ID": "1" }, { "ID": "2" }, { "ID": "3" }],
            "__next": "http://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet?$skiptoken=3"
        }
    });
    for url in [
        "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet?$format=json",
        "http://localhost:4004/odata/v2/admin/Books",
    ] {
        let rule = offered(url, "application/json").unwrap_or_else(|| panic!("no rule for {url}"));
        assert!(rule.id.starts_with("odata-v2"), "{url}: {}", rule.id);
        assert_eq!(
            next_of(&rule, &page).as_deref(),
            Some(
                "http://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet?$skiptoken=3"
            )
        );
        assert_eq!(array_at(rule.rule.merge.as_deref().unwrap(), &page), 3);
        assert_eq!(next_of(&rule, &json!({ "d": { "results": [] } })), None);
    }
}

#[test]
fn a_single_value_has_no_page_to_follow() {
    // V2 `$.d` without `results`: an entity or a property. There is no `__next`, the run ends.
    let rule = offered(
        "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet('1')",
        "application/json",
    )
    .unwrap();
    let entity = json!({ "d": { "ID": "1", "Customer": "100" } });
    assert_eq!(next_of(&rule, &entity), None);
    assert_eq!(array_at(rule.rule.merge.as_deref().unwrap(), &entity), 0);
}

#[test]
fn services_under_other_paths_and_other_media_have_no_rule() {
    // The user adds a rule for these; a guess would stop a run at the first page or follow
    // nothing, and the core would call it the plugin's doing.
    assert!(offered("https://api.example.com/v1/items", "application/json").is_none());
    // `$metadata`, XML and Atom are not paged by link here.
    assert!(
        offered(
            "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/$metadata",
            "application/xml"
        )
        .is_none()
    );
    assert!(
        offered(
            "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet",
            "application/atom+xml"
        )
        .is_none()
    );
}
