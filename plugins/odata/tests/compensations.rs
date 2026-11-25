// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate of wave 19, strand H: the clean-up rules the OData plugin ships as data
//! (`contributes.compensations`), played by a small executor in this file (the app's
//! executor is in the core and knows nothing of OData) against golden responses and the
//! OData test server: V2 (SAP Gateway, CAP) and V4 (RAP, CAP); key from `Location`,
//! `d.__metadata.uri`, `OData-EntityId`, `@odata.id`; ETag for `If-Match`; CSRF token fetch;
//! composite keys; drafts as "not reversible" with a reason.

use std::cell::RefCell;

use rt_odata_testserver::client::{self, CookieJar, Response, TestServer};
use rt_plugin_sdk::api::{CompensationContribution, Contributions};
use serde_json::{Value, json};
use serde_json_path::JsonPath;

const GW: &str = "/sap/opu/odata/sap/ZRT_SALES_SRV/";
const RAP: &str = "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";
const CAP4: &str = "/odata/v4/admin/";
const CAP2: &str = "/odata/v2/admin/";

fn rules() -> Contributions {
    rt_plugin_odata::compensations::contributions()
}

// -------------------------------------------------------------------------------------------
// The executor (what the app does with the data)

/// `*` matches any characters, everything else literally.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Path and query of a URL.
fn target(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.find('/')
        .map(|i| rest[i..].to_owned())
        .unwrap_or_default()
}

fn rule_for(all: &Contributions, method: &str, url: &str) -> Option<CompensationContribution> {
    all.compensations
        .iter()
        .find(|c| {
            let m = &c.matches;
            m.url_pattern.as_ref().is_none_or(|p| glob(p, &target(url)))
                && (m.methods.is_empty() || m.methods.iter().any(|x| x == method))
        })
        .cloned()
}

/// A header or JSONPath source, in order.
fn read(sources: &[String], headers: &[(String, String)], body: &Value) -> Option<String> {
    sources.iter().find_map(|s| {
        if let Some(name) = s.strip_prefix("header:") {
            headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        } else {
            let path = JsonPath::parse(s).expect("valid JSONPath");
            path.query(body)
                .first()
                .and_then(|v| v.as_str().map(str::to_owned))
        }
    })
}

fn not_reversible(rule: &CompensationContribution, url: &str) -> Option<String> {
    let t = target(url);
    rule.rule
        .not_reversible
        .iter()
        .find(|n| glob(&n.url_pattern, &t))
        .map(|n| n.reason.clone())
}

fn body_of(r: &Response) -> Value {
    if r.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&r.body).unwrap_or(Value::Null)
    }
}

// -------------------------------------------------------------------------------------------
// Golden responses (synthetic, `testdata/compensations/golden.json`)

#[test]
fn golden_responses_give_the_resource_url_and_the_version() {
    let golden: Value =
        serde_json::from_str(include_str!("../testdata/compensations/golden.json")).unwrap();
    let all = rules();
    for case in golden["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let url = case["url"].as_str().unwrap();
        let method = case["method"].as_str().unwrap();
        let headers: Vec<(String, String)> = case["headers"]
            .as_object()
            .map(|h| {
                h.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let rule = rule_for(&all, method, url).unwrap_or_else(|| panic!("{name}: no rule"));
        assert_eq!(rule.id, case["rule"], "{name}: rule");
        if let Some(reason) = case["not_reversible"].as_str() {
            assert_eq!(
                not_reversible(&rule, url).as_deref(),
                Some(reason),
                "{name}: not reversible"
            );
            continue;
        }
        assert_eq!(not_reversible(&rule, url), None, "{name}: reversible");
        let id = read(&rule.rule.id_from, &headers, &case["body"]);
        assert_eq!(id.as_deref(), case["id"].as_str(), "{name}: id");
        let etag = read(&rule.rule.etag_from, &headers, &case["body"]);
        assert_eq!(etag.as_deref(), case["etag"].as_str(), "{name}: etag");
    }
}

// -------------------------------------------------------------------------------------------
// The rules as data

#[test]
fn one_rule_per_layout_with_token_and_ids() {
    let all = rules();
    let ids: Vec<_> = all.compensations.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "odata-v4-sap",
            "odata-v4-cap",
            "odata-v2-sap",
            "odata-v2-cap"
        ]
    );
    for c in &all.compensations {
        let token = c.rule.token.as_ref().expect("token");
        assert_eq!(
            (
                token.fetch_header.as_str(),
                token.fetch_value.as_str(),
                token.read_header.as_str(),
                token.send_header.as_str()
            ),
            ("x-csrf-token", "fetch", "x-csrf-token", "x-csrf-token")
        );
        assert!(c.rule.id_from[0] == "header:Location", "{}", c.id);
        assert!(c.rule.etag_from[0] == "header:ETag", "{}", c.id);
        assert!(c.rule.resource_url.is_none());
        assert!(c.matches.hosts.is_empty(), "rules never name hosts");
        assert!(c.matches.methods.is_empty(), "all writing methods");
        assert!(!c.rule.not_reversible.is_empty());
        assert!(c.rule.not_reversible.iter().all(|n| !n.reason.is_empty()));
    }
    // The plugin has no network right for this: rules are data only.
    let layouts = [
        (
            "PATCH",
            format!("https://h:44300{RAP}SalesOrder(1)"),
            "odata-v4-sap",
        ),
        ("POST", format!("https://h{CAP4}Books"), "odata-v4-cap"),
        (
            "DELETE",
            format!("https://h{GW}SalesOrderItemSet('1','2')"),
            "odata-v2-sap",
        ),
        (
            "MERGE",
            format!("https://h{CAP2}Authors(1)"),
            "odata-v2-cap",
        ),
    ];
    for (method, url, id) in layouts {
        assert_eq!(rule_for(&all, method, &url).unwrap().id, id, "{url}");
    }
    assert!(rule_for(&all, "POST", "https://h/api/orders").is_none());
}

#[test]
fn drafts_are_not_reversible_with_a_reason() {
    let all = rules();
    let cases = [
        // CAP draft actions and instances.
        (
            format!("https://h{CAP4}Books(ID=201,IsActiveEntity=true)/AdminService.draftEdit"),
            true,
        ),
        (
            format!("https://h{CAP4}Books(ID=201,IsActiveEntity=false)/AdminService.draftActivate"),
            true,
        ),
        (
            format!("https://h{CAP4}Books(ID=201,IsActiveEntity=false)"),
            true,
        ),
        // RAP actions.
        (
            format!(
                "https://h{RAP}SalesOrder(SalesOrderUUID=u,IsActiveEntity=true)/com.sap.gateway.srvd.zrt.v0001.Edit?x=1"
            ),
            true,
        ),
        (
            format!(
                "https://h{RAP}SalesOrder(SalesOrderUUID=u,IsActiveEntity=false)/SAP__self.Activate"
            ),
            true,
        ),
        (
            format!("https://h{RAP}SalesOrder(SalesOrderUUID=u,IsActiveEntity=true)/Prepare"),
            true,
        ),
        // Gateway draft function imports.
        (
            format!("https://h{GW}SalesOrderActivationAction?SalesOrderID='1'"),
            true,
        ),
        // Ordinary entities stay reversible.
        (
            format!("https://h{CAP4}Books(ID=201,IsActiveEntity=true)"),
            false,
        ),
        (
            format!("https://h{RAP}SalesOrder(SalesOrderUUID=u,IsActiveEntity=true)"),
            false,
        ),
        (format!("https://h{GW}SalesOrderSet('1')"), false),
        (format!("https://h{CAP2}Authors(7)"), false),
    ];
    for (url, draft) in cases {
        let rule = rule_for(&all, "POST", &url).unwrap();
        let reason = not_reversible(&rule, &url);
        assert_eq!(reason.is_some(), draft, "{url}");
        if let Some(r) = reason {
            assert!(r.to_lowercase().contains("draft"), "{r}");
        }
    }
}

// -------------------------------------------------------------------------------------------
// Against the test server

struct App {
    server: TestServer,
    jar: RefCell<CookieJar>,
}

impl App {
    fn new() -> Self {
        Self {
            server: TestServer::start(),
            jar: RefCell::new(CookieJar::default()),
        }
    }

    fn call(&self, method: &str, url: &str, extra: &[(&str, &str)], body: &str) -> Response {
        let mut headers: Vec<(String, String)> = extra
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        if url.contains("/sap/") {
            headers.push(("Authorization".into(), client::s4_basic_auth()));
        }
        if !body.is_empty() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        headers.push(("Accept".into(), "application/json".into()));
        if let Some(c) = self.jar.borrow().header() {
            headers.push(("Cookie".into(), c));
        }
        let r = client::send(method, url, &headers, body.as_bytes()).unwrap();
        self.jar.borrow_mut().store(&r.headers);
        r
    }

    fn url(&self, path: &str) -> String {
        self.server.url(path)
    }

    /// What the app does with a rule's token: fetch with the fetch header, read the answer.
    fn token(&self, rule: &CompensationContribution, resource: &str) -> (String, String) {
        let t = rule.rule.token.as_ref().expect("token");
        let r = self.call("GET", resource, &[(&t.fetch_header, &t.fetch_value)], "");
        let value = r
            .header(&t.read_header)
            .unwrap_or_else(|| panic!("no token in {:?}", r.headers))
            .to_owned();
        (t.send_header.clone(), value)
    }

    fn create(&self, root: &str, set: &str, body: &str) -> (CompensationContribution, Response) {
        let url = self.url(&format!("{root}{set}"));
        let rule = rule_for(&rules(), "POST", &url).expect("rule for the service");
        let (name, token) = self.token(&rule, &self.url(root));
        let r = self.call("POST", &url, &[(&name, &token)], body);
        assert_eq!(r.status, 201, "create: {}", r.text());
        (rule, r)
    }

    /// The resource URL a rule derives from a response (an absolute URL, or `None`).
    fn derived(rule: &CompensationContribution, r: &Response) -> String {
        read(&rule.rule.id_from, &r.headers, &body_of(r)).expect("an id")
    }

    fn delete(&self, rule: &CompensationContribution, resource: &str) -> Response {
        let (name, token) = self.token(rule, resource);
        let read = self.call("GET", resource, &[], "");
        let mut extra = vec![(name.as_str(), token.as_str())];
        let etag = read_etag(rule, &read);
        if let Some(e) = etag.as_deref() {
            extra.push(("If-Match", e));
        }
        self.call("DELETE", resource, &extra, "")
    }
}

fn read_etag(rule: &CompensationContribution, r: &Response) -> Option<String> {
    read(&rule.rule.etag_from, &r.headers, &body_of(r))
}

#[test]
fn v2_gateway_composite_key_from_location_and_delete_with_csrf() {
    let app = App::new();
    let (rule, r) = app.create(
        GW,
        "SalesOrderItemSet",
        r#"{"SalesOrderID":"0500000001","ProductID":"RT-7","Quantity":"2.000","QuantityUnit":"EA"}"#,
    );
    assert_eq!(rule.id, "odata-v2-sap");
    let url = App::derived(&rule, &r);
    assert!(
        url.contains("SalesOrderItemSet(SalesOrderID='0500000001',ItemPosition='"),
        "{url}"
    );
    // The body names the same entity (the second source).
    assert_eq!(
        read(&rule.rule.id_from[1..], &[], &body_of(&r)).as_deref(),
        Some(url.as_str())
    );
    assert_eq!(app.call("GET", &url, &[], "").status, 200);
    // Without the token the server refuses (so the rule's token is needed).
    assert_eq!(app.call("DELETE", &url, &[], "").status, 403);
    assert_eq!(app.delete(&rule, &url).status, 204);
    assert_eq!(app.call("GET", &url, &[], "").status, 404);
}

#[test]
fn v2_gateway_etag_from_header_and_body_is_the_if_match() {
    let app = App::new();
    let (rule, r) = app.create(
        GW,
        "SalesOrderSet",
        r#"{"CustomerID":"0000100001","Currency":"EUR"}"#,
    );
    let url = App::derived(&rule, &r);
    assert!(url.contains("SalesOrderSet('"), "{url}");
    let from_header = read_etag(&rule, &r).unwrap();
    // The `d.__metadata.etag` source gives the same value.
    let from_body = read(&rule.rule.etag_from[1..], &[], &body_of(&r)).unwrap();
    assert_eq!(from_header, from_body);
    let (name, token) = app.token(&rule, &url);
    // Without If-Match the S/4 service asks for it, with a stale one it refuses.
    let patch = r#"{"Currency":"USD"}"#;
    assert_eq!(
        app.call("PATCH", &url, &[(&name, &token)], patch).status,
        428
    );
    let stale = "W/\"datetimeoffset'2000-01-01T00%3A00%3A00.0000000Z'\"";
    assert_eq!(
        app.call(
            "PATCH",
            &url,
            &[(&name, &token), ("If-Match", stale)],
            patch
        )
        .status,
        412
    );
    let ok = app.call(
        "PATCH",
        &url,
        &[(&name, &token), ("If-Match", &from_header)],
        patch,
    );
    assert!(ok.status == 200 || ok.status == 204, "{}", ok.text());
    // The server did not allow deleting the order: the rule has no say in that.
    assert_ne!(app.delete(&rule, &url).status, 204);
}

#[test]
fn v4_rap_key_with_uuid_and_is_active_entity_and_if_match() {
    let app = App::new();
    let (rule, r) = app.create(
        RAP,
        "SalesOrder",
        r#"{"CustomerID":"0000188888","Currency":"EUR","IsActiveEntity":true}"#,
    );
    assert_eq!(rule.id, "odata-v4-sap");
    let url = App::derived(&rule, &r);
    assert!(url.contains("SalesOrder(SalesOrderUUID="), "{url}");
    assert!(url.ends_with("IsActiveEntity=true)"), "{url}");
    assert_eq!(not_reversible(&rule, &url), None);
    let etag = read_etag(&rule, &r).unwrap();
    // `@odata.etag` of the body is the same version.
    assert_eq!(
        read(&rule.rule.etag_from[1..], &[], &body_of(&r)),
        Some(etag.clone())
    );
    let (name, token) = app.token(&rule, &url);
    let patch = r#"{"Currency":"USD"}"#;
    let ok = app.call(
        "PATCH",
        &url,
        &[(&name, &token), ("If-Match", &etag)],
        patch,
    );
    assert!(ok.status == 200 || ok.status == 204, "{}", ok.text());
    // The old version is stale now.
    let again = app.call(
        "PATCH",
        &url,
        &[(&name, &token), ("If-Match", &etag)],
        patch,
    );
    assert_eq!(again.status, 412);
}

#[test]
fn v4_cap_key_from_location_and_delete() {
    let app = App::new();
    let (rule, r) = app.create(CAP4, "Authors", r#"{"ID":500,"name":"Clean Author"}"#);
    assert_eq!(rule.id, "odata-v4-cap");
    let url = App::derived(&rule, &r);
    assert!(url.ends_with("Authors(500)"), "{url}");
    assert_eq!(app.delete(&rule, &url).status, 204);
    assert_eq!(app.call("GET", &url, &[], "").status, 404);
}

#[test]
fn v4_cap_draft_enabled_entity_has_a_two_part_key() {
    let app = App::new();
    let url = app.url(&format!("{CAP4}Books(ID=201,IsActiveEntity=true)"));
    let rule = rule_for(&rules(), "PATCH", &url).unwrap();
    let read = app.call("GET", &url, &[], "");
    assert_eq!(read.status, 200);
    let etag = read_etag(&rule, &read).expect("etag");
    let (name, token) = app.token(&rule, &url);
    let r = app.call(
        "PATCH",
        &url,
        &[(&name, &token), ("If-Match", &etag)],
        r#"{"stock":9}"#,
    );
    assert!(r.status == 200 || r.status == 204, "{}", r.text());
    // The draft of the same book is a draft: not reversible, with the reason.
    let draft = app.url(&format!("{CAP4}Books(ID=201,IsActiveEntity=false)"));
    assert!(not_reversible(&rule, &draft).unwrap().contains("draft"));
}

#[test]
fn v2_cap_adapter_create_and_delete() {
    let app = App::new();
    let (rule, r) = app.create(CAP2, "Authors", r#"{"ID":501,"name":"Clean V2 Author"}"#);
    assert_eq!(rule.id, "odata-v2-cap");
    let url = App::derived(&rule, &r);
    assert!(url.ends_with("Authors(501)"), "{url}");
    assert_eq!(app.delete(&rule, &url).status, 204);
    assert_eq!(json!(app.call("GET", &url, &[], "").status), json!(404));
}

#[test]
fn v4_create_names_the_entity_in_odata_entity_id() {
    let app = App::new();
    let (rule, r) = app.create(CAP4, "Authors", r#"{"ID":502,"name":"Another"}"#);
    let location = r.header("Location").unwrap().to_owned();
    assert_eq!(r.header("OData-EntityId"), Some(location.as_str()));
    // The second source alone derives the same URL.
    let sources = &rule.rule.id_from[1..];
    assert_eq!(read(sources, &r.headers, &Value::Null), Some(location));
}
