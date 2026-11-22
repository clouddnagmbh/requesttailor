// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate of wave 6, strand C: the plugin's hooks and tabs against the OData test server, for
//! OData V2 (SAP Gateway `ZRT_SALES_SRV`, CAP `admin-v2`) and V4 (RAP `zrt_salesorder`, CAP
//! `admin-v4`): read, create with CSRF, update with ETag (and a stale ETag), `$batch` with
//! two changesets (the second fails and is rolled back alone), drafts.
//!
//! The hooks run through the plugin's JSON dispatch with a `MockHost` that plays the app:
//! `http` host calls go to the server over HTTP, `session: true` calls share the cookie jar of
//! the send, `credentials: true` adds the request's Basic auth. The send itself (between
//! `prepare` and `response`) goes out the same way, with the patch applied.

use std::cell::RefCell;
use std::rc::Rc;

use rt_odata_testserver::client::{self, CookieJar, TestServer};
use rt_plugin_odata::OData;
use rt_plugin_odata::service::storage_key;
use rt_plugin_sdk::api::{
    Block, Call, HostCall, NotesReply, PluginBody, PluginNote, PluginRequest, PluginResponse,
    PrepareReply, RequestPatch, TabTarget, ViewReply,
};
use rt_plugin_sdk::model::{Body, KeyValue, RequestSpec};
use rt_plugin_sdk::testing::{self, HostGuard, MockHost};
use serde_json::{Value, json};

const GW: &str = "/sap/opu/odata/sap/ZRT_SALES_SRV/";
const RAP: &str = "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";
const CAP4: &str = "/odata/v4/admin/";
const CAP2: &str = "/odata/v2/admin/";
const UUID1: &str = "0b7e2f0a-6c1d-4e5f-9a10-000000000001";
const UUID2: &str = "0b7e2f0a-6c1d-4e5f-9a10-000000000002";
const REQUEST_PATH: &str = "orders/request.yml";

// -------------------------------------------------------------------------------------------
// The app around the plugin

/// What goes on the wire: Basic auth for the S/4 services when `credentials`, cookies of
/// `jar` when given.
fn wire(
    request: &PluginRequest,
    credentials: bool,
    jar: Option<&RefCell<CookieJar>>,
) -> PluginResponse {
    let mut headers = request.headers.clone();
    if credentials && request.url.contains("/sap/") {
        headers.push(("Authorization".into(), client::s4_basic_auth()));
    }
    if let Some(cookie) = jar.and_then(|j| j.borrow().header()) {
        headers.push(("Cookie".into(), cookie));
    }
    let body = request
        .body
        .as_ref()
        .map(|b| rt_plugin_sdk::body_bytes(b).unwrap())
        .unwrap_or_default();
    let r = client::send(&request.method, &request.url, &headers, &body).expect("server reachable");
    if let Some(jar) = jar {
        jar.borrow_mut().store(&r.headers);
    }
    let content_type = r.header("Content-Type").map(str::to_owned);
    PluginResponse {
        status: r.status,
        url: request.url.clone(),
        body: PluginBody {
            content_type,
            text: Some(r.text()),
            base64: None,
        },
        headers: r.headers,
        truncated: false,
    }
}

/// The request after the patch, as the host applies it (API.md order).
fn apply(mut request: PluginRequest, patch: &RequestPatch) -> PluginRequest {
    if let Some(url) = &patch.url {
        request.url = url.clone();
    }
    request.headers.retain(|(n, _)| {
        !patch
            .remove_headers
            .iter()
            .chain(patch.set_headers.iter().map(|(s, _)| s))
            .any(|r| r.eq_ignore_ascii_case(n))
    });
    request.headers.extend(patch.set_headers.iter().cloned());
    if let Some(body) = &patch.body {
        request.body = Some(body.clone());
    }
    request
}

struct Sent {
    request: PluginRequest,
    response: PluginResponse,
    prepare: Vec<PluginNote>,
    notes: Vec<PluginNote>,
}

impl Sent {
    fn status(&self) -> u16 {
        self.response.status
    }

    fn json(&self) -> Value {
        serde_json::from_str(self.response.body.text.as_deref().unwrap_or_default())
            .unwrap_or_else(|_| panic!("not JSON: {:?}", self.response.body.text))
    }

    fn header(&self, name: &str) -> Option<&str> {
        rt_plugin_sdk::header(&self.response.headers, name)
    }

    fn sent(&self, name: &str) -> Option<&str> {
        rt_plugin_sdk::header(&self.request.headers, name)
    }

    fn noted(&self, text: &str) -> bool {
        self.prepare
            .iter()
            .chain(&self.notes)
            .any(|n| n.text.contains(text))
    }

    fn all_notes(&self) -> Vec<String> {
        self.prepare
            .iter()
            .chain(&self.notes)
            .map(|n| n.text.clone())
            .collect()
    }
}

struct App {
    server: TestServer,
    host: MockHost,
    jar: Rc<RefCell<CookieJar>>,
    _guard: HostGuard,
}

impl App {
    fn new() -> Self {
        let server = TestServer::start();
        let manifest = rt_plugin_sdk::parse_manifest(rt_plugin_odata::MANIFEST).unwrap();
        let jar = Rc::new(RefCell::new(CookieJar::default()));
        let host = MockHost::new()
            .granted(manifest.permissions)
            .request_host("127.0.0.1");
        let calls = host.clone();
        let session_jar = jar.clone();
        let host = host.on_http(move |request, credentials| {
            let session = matches!(
                calls.calls().last(),
                Some(HostCall::Http { session: true, .. })
            );
            Ok(wire(request, credentials, session.then_some(&*session_jar)))
        });
        let guard = host.install();
        Self {
            server,
            host,
            jar,
            _guard: guard,
        }
    }

    fn url(&self, path: &str) -> String {
        self.server.url(path)
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> PluginRequest {
        let mut headers: Vec<(String, String)> = headers
            .iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect();
        headers.push(("Accept".into(), "application/json".into()));
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        PluginRequest {
            method: method.into(),
            url: self.url(path),
            headers,
            body: body.map(|b| rt_plugin_sdk::text_body(Some("application/json"), b)),
        }
    }

    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> Sent {
        self.send_request(self.request(method, path, headers, body))
    }

    /// prepare → wire (with the send's cookie jar and auth) → response.
    fn send_request(&self, request: PluginRequest) -> Sent {
        let context = testing::context(REQUEST_PATH);
        let prepared: PrepareReply = testing::call(
            &OData,
            &Call::Prepare {
                request: request.clone(),
                context: context.clone(),
            },
        )
        .expect("prepare");
        assert!(
            !serde_json::to_string(&prepared.patch)
                .unwrap()
                .contains("***"),
            "a patch never carries the mask"
        );
        let sent = apply(request, &prepared.patch);
        let response = wire(&sent, true, Some(&self.jar));
        let notes: NotesReply = testing::call(
            &OData,
            &Call::Response {
                request: sent.clone(),
                response: response.clone(),
                context,
            },
        )
        .expect("response");
        Sent {
            request: sent,
            response,
            prepare: prepared.notes,
            notes: notes.notes,
        }
    }

    /// The request a tab rewrote, sent.
    fn send_spec(&self, spec: &RequestSpec) -> Sent {
        let headers: Vec<(String, String)> = spec
            .headers
            .iter()
            .filter(|h| h.enabled)
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect();
        let body = match &spec.body {
            Body::Raw { data, .. } => Some(rt_plugin_sdk::text_body(
                rt_plugin_sdk::header(&headers, "Content-Type"),
                data.clone(),
            )),
            _ => None,
        };
        self.send_request(PluginRequest {
            method: spec.method.clone(),
            url: spec.url.clone(),
            headers,
            body,
        })
    }

    /// CSRF fetches the plugin made so far.
    fn fetches(&self) -> Vec<String> {
        self.host
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                HostCall::Http {
                    request, session, ..
                } if rt_plugin_sdk::header(&request.headers, "X-CSRF-Token") == Some("Fetch") => {
                    assert!(session, "the token fetch runs in the session of the send");
                    Some(request.url)
                }
                _ => None,
            })
            .collect()
    }

    /// Stores a model as strand A caches it (`metadata:<root>` → JSON of `edm::Model`).
    fn seed_model(&self, model: Value) {
        let parsed: rt_plugin_odata::edm::Model =
            serde_json::from_value(model.clone()).expect("model JSON");
        rt_plugin_sdk::host::storage_set(storage_key("metadata", &parsed.service_root), model)
            .unwrap();
    }

    fn spec(&self, method: &str, path: &str) -> RequestSpec {
        RequestSpec {
            name: "request".into(),
            method: method.into(),
            url: self.url(path),
            headers: vec![KeyValue::new("Accept", "application/json")],
            ..RequestSpec::default()
        }
    }

    fn render(&self, tab: &str, spec: &RequestSpec, state: Option<Value>) -> ViewReply {
        testing::call(
            &OData,
            &Call::RenderTab {
                tab: tab.into(),
                target: target(),
                spec: spec.clone(),
                response: None,
                state,
                context: Some(testing::context(REQUEST_PATH)),
            },
        )
        .expect("render-tab")
    }

    fn act(
        &self,
        tab: &str,
        action: &str,
        spec: &RequestSpec,
        state: Option<Value>,
        input: &[(&str, &str)],
    ) -> ViewReply {
        testing::call(
            &OData,
            &Call::Action {
                tab: tab.into(),
                action: action.into(),
                target: target(),
                spec: spec.clone(),
                response: None,
                state,
                input: input
                    .iter()
                    .map(|(n, v)| ((*n).into(), (*v).into()))
                    .collect(),
                context: Some(testing::context(REQUEST_PATH)),
            },
        )
        .expect("action")
    }

    /// A GET outside the plugin (own session), for checking the server's data.
    fn check(&self, path: &str) -> client::Response {
        let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        if path.starts_with("/sap/") {
            headers.push(("Authorization".into(), client::s4_basic_auth()));
        }
        client::send("GET", &self.url(path), &headers, b"").unwrap()
    }

    /// Another user changes an entity behind the plugin's back (new ETag on the server).
    fn change_elsewhere(&self, root: &str, entity: &str, body: &str) {
        let mut jar = CookieJar::default();
        let mut call = |method: &str, path: &str, extra: &[(String, String)], body: &str| {
            let mut headers = extra.to_vec();
            if path.starts_with("/sap/") {
                headers.push(("Authorization".into(), client::s4_basic_auth()));
            }
            if let Some(c) = jar.header() {
                headers.push(("Cookie".into(), c));
            }
            let r = client::send(method, &self.url(path), &headers, body.as_bytes()).unwrap();
            jar.store(&r.headers);
            r
        };
        let token = call("GET", root, &[("X-CSRF-Token".into(), "Fetch".into())], "");
        let token = token.header("X-CSRF-Token").unwrap().to_owned();
        let etag = call("GET", &format!("{root}{entity}"), &[], "")
            .header("ETag")
            .unwrap()
            .to_owned();
        let r = call(
            "PATCH",
            &format!("{root}{entity}"),
            &[
                ("X-CSRF-Token".into(), token),
                ("If-Match".into(), etag),
                ("Content-Type".into(), "application/json".into()),
            ],
            body,
        );
        assert!(r.status < 300, "change elsewhere: {}", r.text());
    }
}

fn target() -> TabTarget {
    TabTarget {
        request_path: REQUEST_PATH.into(),
        response_id: None,
    }
}

fn rows(json: &Value) -> Vec<Value> {
    json.get("d")
        .map_or(&json["value"], |d| &d["results"])
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn tables(view: &ViewReply) -> Vec<Vec<Vec<String>>> {
    view.view
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Table { rows, .. } => Some(rows.clone()),
            _ => None,
        })
        .collect()
}

fn buttons(view: &ViewReply) -> Vec<String> {
    view.view
        .blocks
        .iter()
        .flat_map(|b| match b {
            Block::Actions { actions } => actions.iter().map(|a| a.id.clone()).collect(),
            _ => Vec::new(),
        })
        .collect()
}

// -------------------------------------------------------------------------------------------
// Models as strand A caches them (V4 only: they give the keys of collection entries and the
// draft actions).

fn prop(name: &str, ty: &str) -> Value {
    json!({ "name": name, "type": ty, "nullable": true })
}

fn rap_model(root: &str) -> Value {
    let ty = "com.example.zrt_salesorder.SalesOrderType";
    let op = |n: &str| json!({ "name": n, "kind": "action", "bound_to": ty, "parameters": [] });
    json!({
        "version": "v4",
        "service_root": root,
        "namespace": "com.example.zrt_salesorder",
        "entity_types": [{
            "name": ty,
            "key": ["SalesOrderUUID", "IsActiveEntity"],
            "properties": [
                prop("SalesOrderUUID", "Edm.Guid"), prop("SalesOrderID", "Edm.String"),
                prop("CustomerID", "Edm.String"), prop("Currency", "Edm.String"),
                prop("LastChangedAt", "Edm.DateTimeOffset"), prop("IsActiveEntity", "Edm.Boolean")
            ],
            "navigation": []
        }],
        "complex_types": [],
        "entity_sets": [{ "name": "SalesOrder", "entity_type": ty, "insertable": true, "updatable": true, "deletable": false, "draft": true }],
        "operations": [op("Edit"), op("Prepare"), op("Activate"), op("Discard")]
    })
}

fn cap_model(root: &str) -> Value {
    let op = |n: &str| json!({ "name": n, "kind": "action", "bound_to": "AdminService.Books", "parameters": [] });
    json!({
        "version": "v4",
        "service_root": root,
        "namespace": "AdminService",
        "entity_types": [
            {
                "name": "AdminService.Books",
                "key": ["ID", "IsActiveEntity"],
                "properties": [prop("ID", "Edm.Int32"), prop("title", "Edm.String"), prop("stock", "Edm.Int32"), prop("IsActiveEntity", "Edm.Boolean")],
                "navigation": []
            },
            {
                "name": "AdminService.Authors",
                "key": ["ID"],
                "properties": [prop("ID", "Edm.Int32"), prop("name", "Edm.String")],
                "navigation": []
            }
        ],
        "complex_types": [],
        "entity_sets": [
            { "name": "Books", "entity_type": "AdminService.Books", "insertable": true, "updatable": true, "deletable": true, "draft": true },
            { "name": "Authors", "entity_type": "AdminService.Authors", "insertable": true, "updatable": true, "deletable": true }
        ],
        "operations": [op("draftEdit"), op("draftPrepare"), op("draftActivate")]
    })
}

// -------------------------------------------------------------------------------------------
// The gate, per service

struct Case {
    root: &'static str,
    set: &'static str,
    creates: [&'static str; 2],
    /// An existing entity (relative to the root) and a change to it.
    entity: String,
    patch: [&'static str; 3],
    /// `(method, url, body, changeset)`; changeset 2 fails.
    batch: Vec<(&'static str, String, &'static str, &'static str)>,
    /// Whether the batch carries If-Match from remembered ETags.
    batch_if_match: bool,
    model: Option<fn(&str) -> Value>,
    verify: fn(&App),
}

fn gate(case: Case) {
    let app = App::new();
    if let Some(model) = case.model {
        app.seed_model(model(&app.url(case.root)));
    }
    let set = format!("{}{}", case.root, case.set);

    // 1. Read.
    let r = app.send("GET", &set, &[], None);
    assert_eq!(r.status(), 200, "read");
    assert!(!rows(&r.json()).is_empty(), "read returns entities");
    assert!(app.fetches().is_empty(), "reads need no CSRF token");

    // 2. Create with CSRF: the token is fetched once, then reused.
    let r = app.send("POST", &set, &[], Some(case.creates[0]));
    assert!(r.noted("CSRF token fetched"), "{:?}", r.all_notes());
    assert!(r.sent("X-CSRF-Token").is_some());
    assert_eq!(r.status(), 201, "create: {:?}", r.response.body.text);
    let r = app.send("POST", &set, &[], Some(case.creates[1]));
    assert_eq!(r.status(), 201, "second create: {:?}", r.response.body.text);
    assert_eq!(app.fetches().len(), 1, "the cached token is reused");
    assert_eq!(app.fetches()[0], app.url(case.root));

    // 3. Update with ETag: GET remembers it, PATCH gets If-Match; the new ETag of the
    // response is remembered too; a stale ETag gives 412.
    let entity = format!("{}{}", case.root, case.entity);
    let read = app.send("GET", &entity, &[], None);
    let etag = read.header("ETag").expect("ETag on read").to_owned();
    let r = app.send("PATCH", &entity, &[], Some(case.patch[0]));
    assert_eq!(r.sent("If-Match"), Some(etag.as_str()));
    assert!(r.noted("If-Match"), "{:?}", r.all_notes());
    assert!(
        matches!(r.status(), 200 | 204),
        "update: {:?}",
        r.response.body.text
    );
    let r = app.send("PATCH", &entity, &[], Some(case.patch[1]));
    assert!(
        matches!(r.status(), 200 | 204),
        "update with the new ETag: {:?}",
        r.response.body.text
    );
    assert!(
        r.sent("If-Match").is_some_and(|e| e != etag),
        "the ETag of the first update is used"
    );
    app.change_elsewhere(case.root, &case.entity, case.patch[2]);
    let r = app.send("PATCH", &entity, &[], Some(case.patch[1]));
    assert_eq!(r.status(), 412, "stale ETag");
    assert!(r.noted("412"), "{:?}", r.all_notes());

    // 4. $batch with two changesets, composed in the batch tab.
    let spec = app.spec("GET", &set);
    let mut state = None;
    for (method, url, body, changeset) in &case.batch {
        let reply = app.act(
            "batch",
            "add",
            &spec,
            state,
            &[
                ("method", method),
                ("url", url),
                ("body", body),
                ("changeset", changeset),
            ],
        );
        state = reply.state;
    }
    let built = app.act("batch", "build", &spec, state.clone(), &[]);
    let batch = built.update_request.expect("build rewrites the request");
    assert_eq!(batch.method, "POST");
    assert_eq!(batch.url, app.url(&format!("{}$batch", case.root)));
    let Body::Raw { data, .. } = &batch.body else {
        panic!("raw body")
    };
    assert_eq!(
        data.matches("Content-Type: multipart/mixed; boundary=changeset_")
            .count(),
        2
    );
    assert_eq!(data.contains("If-Match: W/"), case.batch_if_match, "{data}");
    let r = app.send_spec(&batch);
    assert!(
        r.sent("X-CSRF-Token").is_some(),
        "the batch POST carries the token"
    );
    assert!(
        matches!(r.status(), 200 | 202),
        "batch: {:?}",
        r.response.body.text
    );
    assert!(r.noted("changeset 1: 2 ok"), "{:?}", r.all_notes());
    assert!(
        r.noted("changeset 2: failed with 404, rolled back"),
        "{:?}",
        r.all_notes()
    );
    (case.verify)(&app);
    // The tab shows the parts' results of the last send.
    let view = app.render("batch", &batch, state);
    let last = tables(&view).pop().unwrap();
    assert_eq!(last.len(), case.batch.len() - 1, "{last:?}");
    assert!(last.iter().any(|row| row[3] == "404"));
}

#[test]
fn v2_gateway_read_create_update_batch() {
    gate(Case {
        root: GW,
        set: "SalesOrderSet",
        creates: [
            r#"{"CustomerID":"0000100001","Currency":"EUR","Note":"via plugin"}"#,
            r#"{"CustomerID":"0000100002","Currency":"NOK"}"#,
        ],
        entity: "SalesOrderSet('0500000002')".into(),
        patch: [
            r#"{"Note":"changed"}"#,
            r#"{"Note":"again"}"#,
            r#"{"Note":"elsewhere"}"#,
        ],
        batch: vec![
            ("GET", "SalesOrderSet('0500000001')".into(), "", "0"),
            (
                "POST",
                "SalesOrderSet".into(),
                r#"{"CustomerID":"0000100003","Currency":"GBP"}"#,
                "1",
            ),
            (
                "POST",
                "$2/ToItems".into(),
                r#"{"ProductID":"RT-7","Quantity":"2.000","QuantityUnit":"EA"}"#,
                "1",
            ),
            (
                "POST",
                "SalesOrderSet".into(),
                r#"{"CustomerID":"0000100001","Currency":"EUR"}"#,
                "2",
            ),
            (
                "PATCH",
                "SalesOrderSet('0999999999')".into(),
                r#"{"Note":"x"}"#,
                "2",
            ),
        ],
        batch_if_match: false,
        model: None,
        verify: |app| {
            let all = app
                .check(&format!("{GW}SalesOrderSet?$inlinecount=allpages"))
                .json();
            assert_eq!(all["d"]["__count"], "6", "3 + 2 creates + changeset 1");
            let items = app
                .check(&format!("{GW}SalesOrderSet('0500000006')/ToItems"))
                .json();
            assert_eq!(
                rows(&items)[0]["ProductID"],
                "RT-7",
                "$2 refers to the order of the same changeset"
            );
            assert_eq!(
                app.check(&format!("{GW}SalesOrderSet('0500000007')"))
                    .status,
                404
            );
        },
    });
}

#[test]
fn v2_cap_read_create_update_batch() {
    gate(Case {
        root: CAP2,
        set: "Books",
        creates: [
            r#"{"ID":300,"title":"Plugin Book","stock":1}"#,
            r#"{"ID":303,"title":"Second"}"#,
        ],
        entity: "Books(207)".into(),
        patch: [r#"{"stock":5}"#, r#"{"stock":6}"#, r#"{"stock":7}"#],
        batch: vec![
            (
                "POST",
                "Books".into(),
                r#"{"ID":301,"title":"Batch Book"}"#,
                "1",
            ),
            ("PATCH", "Books(201)".into(), r#"{"stock":9}"#, "1"),
            (
                "POST",
                "Books".into(),
                r#"{"ID":302,"title":"Rolled back"}"#,
                "2",
            ),
            ("PATCH", "Books(999)".into(), r#"{"stock":1}"#, "2"),
        ],
        batch_if_match: true,
        model: None,
        verify: |app| {
            assert_eq!(app.check(&format!("{CAP2}Books(301)")).status, 200);
            assert_eq!(
                app.check(&format!("{CAP2}Books(201)")).json()["d"]["stock"],
                9
            );
            assert_eq!(app.check(&format!("{CAP2}Books(302)")).status, 404);
        },
    });
}

#[test]
fn v4_rap_read_create_update_batch() {
    gate(Case {
        root: RAP,
        set: "SalesOrder",
        creates: [
            r#"{"CustomerID":"0000100002","Currency":"NOK"}"#,
            r#"{"CustomerID":"0000100003","Currency":"GBP","IsActiveEntity":true}"#,
        ],
        entity: format!("SalesOrder(SalesOrderUUID={UUID2},IsActiveEntity=true)"),
        patch: [r#"{"Currency":"EUR"}"#, r#"{"Currency":"USD"}"#, r#"{"Currency":"CHF"}"#],
        batch: vec![
            ("POST", "SalesOrder".into(), r#"{"CustomerID":"0000188888","Currency":"EUR","IsActiveEntity":true}"#, "1"),
            ("PATCH", format!("SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=true)"), r#"{"Currency":"USD"}"#, "1"),
            ("POST", "SalesOrder".into(), r#"{"CustomerID":"0000199999","Currency":"EUR"}"#, "2"),
            ("PATCH", "SalesOrder(SalesOrderUUID=00000000-0000-0000-0000-000000000000,IsActiveEntity=true)".into(), "{}", "2"),
        ],
        // If-Match is required on S/4: it comes from the ETags of the collection read.
        batch_if_match: true,
        model: Some(rap_model),
        verify: |app| {
            let count = |customer: &str| {
                app.check(&format!("{RAP}SalesOrder/$count?$filter=CustomerID eq '{customer}'")).text()
            };
            assert_eq!(count("0000188888"), "1");
            assert_eq!(count("0000199999"), "0", "changeset 2 rolled back");
            let so = app.check(&format!("{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=true)")).json();
            assert_eq!(so["Currency"], "USD");
        },
    });
}

#[test]
fn v4_cap_read_create_update_batch() {
    gate(Case {
        root: CAP4,
        set: "Books",
        creates: [
            r#"{"ID":300,"title":"Plugin Book"}"#,
            r#"{"ID":303,"title":"Direct","IsActiveEntity":true}"#,
        ],
        entity: "Books(ID=207,IsActiveEntity=true)".into(),
        patch: [r#"{"stock":5}"#, r#"{"stock":6}"#, r#"{"stock":7}"#],
        batch: vec![
            (
                "POST",
                "Authors".into(),
                r#"{"ID":500,"name":"Batch Author"}"#,
                "1",
            ),
            (
                "PATCH",
                "Books(ID=201,IsActiveEntity=true)".into(),
                r#"{"stock":9}"#,
                "1",
            ),
            (
                "POST",
                "Authors".into(),
                r#"{"ID":501,"name":"Rolled back"}"#,
                "2",
            ),
            ("PATCH", "Authors(9999)".into(), r#"{"name":"x"}"#, "2"),
        ],
        batch_if_match: true,
        model: Some(cap_model),
        verify: |app| {
            assert_eq!(app.check(&format!("{CAP4}Authors(500)")).status, 200);
            assert_eq!(app.check(&format!("{CAP4}Books(201)")).json()["stock"], 9);
            assert_eq!(app.check(&format!("{CAP4}Authors(501)")).status, 404);
        },
    });
}

// -------------------------------------------------------------------------------------------
// CSRF details

#[test]
fn expired_token_is_dropped_and_fetched_again() {
    let app = App::new();
    let set = format!("{GW}SalesOrderSet?sap-client=100");
    let body = r#"{"CustomerID":"0000100001","Currency":"EUR"}"#;
    assert_eq!(app.send("POST", &set, &[], Some(body)).status(), 201);
    assert_eq!(
        app.fetches(),
        vec![app.url(&format!("{GW}?sap-client=100"))],
        "the fetch keeps sap-client"
    );
    // The session ends (cookie gone): the cached token is refused once.
    app.jar.borrow_mut().clear();
    let r = app.send("POST", &set, &[], Some(body));
    assert_eq!(r.status(), 403);
    assert!(
        r.noted("CSRF token expired, send again"),
        "{:?}",
        r.all_notes()
    );
    let r = app.send("POST", &set, &[], Some(body));
    assert!(r.noted("CSRF token fetched"));
    assert_eq!(r.status(), 201);
    // A token the user set is left alone.
    let r = app.send("POST", &set, &[("X-CSRF-Token", "mine")], Some(body));
    assert_eq!(r.sent("X-CSRF-Token"), Some("mine"));
    assert_eq!(r.status(), 403);
}

#[test]
fn hand_written_batch_gets_crlf_and_a_token() {
    let app = App::new();
    let body = "--b\nContent-Type: application/http\n\nGET Books(201) HTTP/1.1\nAccept: application/json\n\n\n--b--\n";
    let mut request = app.request("POST", &format!("{CAP4}$batch"), &[], None);
    request
        .headers
        .push(("Content-Type".into(), "multipart/mixed; boundary=b".into()));
    request.body = Some(rt_plugin_sdk::text_body(
        Some("multipart/mixed; boundary=b"),
        body,
    ));
    let r = app.send_request(request);
    assert!(
        r.request
            .body
            .as_ref()
            .unwrap()
            .text
            .as_deref()
            .unwrap()
            .contains("\r\n")
    );
    assert_eq!(r.status(), 202);
    assert!(r.noted("$batch: read: 200"), "{:?}", r.all_notes());
}

#[test]
fn json_batch_gets_a_token_and_a_summary() {
    let app = App::new();
    let body = r#"{"requests":[
        {"id":"r1","method":"GET","url":"Books(201)"},
        {"id":"c1","atomicityGroup":"g1","method":"POST","url":"Books","body":{"ID":902,"title":"Pair"}}
    ]}"#;
    let r = app.send("POST", &format!("{CAP4}$batch"), &[], Some(body));
    assert!(r.noted("CSRF token fetched"));
    assert_eq!(r.status(), 200, "{:?}", r.response.body.text);
    assert!(
        r.noted("$batch (JSON): r1 200, c1 201"),
        "{:?}",
        r.all_notes()
    );
}

// -------------------------------------------------------------------------------------------
// Drafts

#[test]
fn draft_tab_rap_edit_and_activate() {
    let app = App::new();
    app.seed_model(rap_model(&app.url(RAP)));
    let active = format!("{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=true)");
    let etag = app
        .send("GET", &active, &[], None)
        .header("ETag")
        .unwrap()
        .to_owned();

    let spec = app.spec("GET", &active);
    let view = app.render("draft", &spec, None);
    assert_eq!(buttons(&view), vec!["edit", "open-draft"]);
    let edit = app
        .act("draft", "edit", &spec, None, &[])
        .update_request
        .unwrap();
    assert_eq!(
        edit.url,
        app.url(&format!("{active}/com.example.zrt_salesorder.Edit"))
    );
    let r = app.send_spec(&edit);
    assert_eq!(
        r.sent("If-Match"),
        Some(etag.as_str()),
        "Edit on S/4 needs the active ETag"
    );
    assert_eq!(r.status(), 200, "{:?}", r.response.body.text);
    assert_eq!(r.json()["IsActiveEntity"], false);

    // After Edit the tab offers the draft's actions.
    assert_eq!(
        buttons(&app.render("draft", &edit, None)),
        vec!["prepare", "activate", "discard", "open-active"]
    );
    let draft = format!("{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=false)");
    assert!(
        app.send("PATCH", &draft, &[], Some(r#"{"Currency":"JPY"}"#))
            .status()
            < 300
    );
    let prepare = app
        .act("draft", "prepare", &edit, None, &[])
        .update_request
        .unwrap();
    assert_eq!(app.send_spec(&prepare).status(), 200);
    let activate = app
        .act("draft", "activate", &edit, None, &[])
        .update_request
        .unwrap();
    assert_eq!(
        activate.url,
        app.url(&format!("{draft}/com.example.zrt_salesorder.Activate"))
    );
    let r = app.send_spec(&activate);
    assert_eq!(r.status(), 200, "{:?}", r.response.body.text);
    assert_eq!(r.json()["IsActiveEntity"], true);
    assert_eq!(r.json()["Currency"], "JPY");

    // Discard: Edit again, then the RAP Discard action.
    let r = app.send("GET", &active, &[], None);
    assert_eq!(r.status(), 200);
    assert_eq!(app.send_spec(&edit).status(), 200);
    let discard = app
        .act("draft", "discard", &edit, None, &[])
        .update_request
        .unwrap();
    assert_eq!(discard.method, "POST");
    assert_eq!(app.send_spec(&discard).status(), 204);
    assert_eq!(app.check(&draft).status, 404);
}

#[test]
fn draft_tab_cap_edit_activate_discard() {
    let app = App::new();
    app.seed_model(cap_model(&app.url(CAP4)));
    let bare = format!("{CAP4}Books(251)");
    let spec = app.spec("GET", &bare);
    let edit = app
        .act("draft", "edit", &spec, None, &[])
        .update_request
        .unwrap();
    assert_eq!(
        edit.url,
        app.url(&format!(
            "{CAP4}Books(ID=251,IsActiveEntity=true)/AdminService.draftEdit"
        ))
    );
    let Body::Raw { data, .. } = &edit.body else {
        panic!("raw body")
    };
    assert_eq!(data, r#"{"PreserveChanges":true}"#);
    let r = app.send_spec(&edit);
    assert_eq!(r.status(), 201, "{:?}", r.response.body.text);
    let draft = format!("{CAP4}Books(ID=251,IsActiveEntity=false)");
    assert!(
        app.send(
            "PATCH",
            &draft,
            &[],
            Some(r#"{"title":"Salt and Iron, revised"}"#)
        )
        .status()
            < 300
    );
    let activate = app
        .act("draft", "activate", &edit, None, &[])
        .update_request
        .unwrap();
    let r = app.send_spec(&activate);
    assert_eq!(r.json()["title"], "Salt and Iron, revised");

    assert_eq!(app.send_spec(&edit).status(), 201);
    let discard = app
        .act("draft", "discard", &edit, None, &[])
        .update_request
        .unwrap();
    assert_eq!(
        (discard.method.as_str(), discard.url.clone()),
        ("DELETE", app.url(&draft))
    );
    assert_eq!(app.send_spec(&discard).status(), 204);
    assert_eq!(
        app.check(&format!("{CAP4}Books(251)")).json()["HasDraftEntity"],
        false
    );

    // Not draft-enabled, no model: the tab says so instead of offering actions.
    let authors = app.spec("GET", &format!("{CAP4}Authors(150)"));
    assert!(buttons(&app.render("draft", &authors, None)).is_empty());
    let other = app.spec("GET", &format!("{CAP2}Books(201)"));
    assert!(buttons(&app.render("draft", &other, None)).is_empty());
}
