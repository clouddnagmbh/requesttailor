// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OData test server against its own description: reads, CSRF, ETags, `$batch`, drafts,
//! auth and `sap-client`, for the four services.

use rt_odata_testserver::client::{self, CookieJar, Response, TestServer};
use rt_odata_testserver::services::SERVICES;
use serde_json::{Value, json};

const GW: &str = "/sap/opu/odata/sap/ZRT_SALES_SRV/";
const RAP: &str = "/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";
const CAP4: &str = "/odata/v4/admin/";
const CAP2: &str = "/odata/v2/admin/";
const UUID1: &str = "0b7e2f0a-6c1d-4e5f-9a10-000000000001";

/// One user against one server: Basic auth on S/4 paths, a cookie jar, a CSRF token.
struct User {
    server: TestServer,
    jar: CookieJar,
    token: Option<String>,
}

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

impl User {
    fn new() -> Self {
        Self {
            server: TestServer::start(),
            jar: CookieJar::default(),
            token: None,
        }
    }

    fn raw(&mut self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Response {
        let mut headers = h(headers);
        if path.starts_with("/sap/") && !headers.iter().any(|(n, _)| n == "Authorization") {
            headers.push(("Authorization".into(), client::s4_basic_auth()));
        }
        if let Some(c) = self.jar.header() {
            headers.push(("Cookie".into(), c));
        }
        let r = client::send(method, &self.server.url(path), &headers, body.as_bytes()).unwrap();
        self.jar.store(&r.headers);
        r
    }

    fn get(&mut self, path: &str) -> Response {
        self.raw("GET", path, &[("Accept", "application/json")], "")
    }

    fn fetch(&mut self, root: &str) -> String {
        let r = self.raw("GET", root, &[("X-CSRF-Token", "Fetch")], "");
        assert_eq!(r.status, 200, "{}", r.text());
        let token = r.header("X-CSRF-Token").unwrap().to_owned();
        self.token = Some(token.clone());
        token
    }

    /// A modifying request with the fetched token.
    fn write(
        &mut self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Response {
        let token = self.token.clone().expect("fetch first");
        let mut all = vec![("X-CSRF-Token", token.as_str())];
        if !headers.iter().any(|(n, _)| *n == "Content-Type") {
            all.push(("Content-Type", "application/json"));
        }
        all.extend_from_slice(headers);
        self.raw(method, path, &all, body)
    }
}

#[test]
fn table_agrees_with_the_edmx_files() {
    for def in SERVICES {
        for set in def.sets {
            assert!(
                def.edmx
                    .contains(&format!("EntitySet Name=\"{}\"", set.name)),
                "{} in {}",
                set.name,
                def.root
            );
            if let Some((prop, _)) = set.etag {
                assert!(def.edmx.contains(&format!("Property Name=\"{prop}\"")));
            }
            for nav in set.navs {
                assert!(
                    def.edmx
                        .contains(&format!("NavigationProperty Name=\"{}\"", nav.name))
                );
            }
        }
    }
    let roots: Vec<_> = rt_odata_testserver::SERVICES
        .iter()
        .map(|(r, _)| *r)
        .collect();
    assert_eq!(roots, SERVICES.iter().map(|s| s.root).collect::<Vec<_>>());
}

#[test]
fn s4_needs_basic_auth_and_client_100() {
    let mut u = User::new();
    let r = u.raw(
        "GET",
        &format!("{GW}SalesOrderSet"),
        &[("Authorization", "Basic eDp5")],
        "",
    );
    assert_eq!(r.status, 401);
    assert!(r.header("WWW-Authenticate").unwrap().starts_with("Basic"));
    let r = u.get(&format!("{GW}SalesOrderSet?sap-client=200&sap-language=DE"));
    assert_eq!(r.status, 401);
    let e = r.json();
    assert_eq!(e["error"]["message"]["lang"], "de");
    assert!(
        e["error"]["message"]["value"]
            .as_str()
            .unwrap()
            .contains("client 200")
    );
    assert_eq!(
        u.get(&format!("{GW}SalesOrderSet?sap-client=100")).status,
        200
    );
    // CAP has no auth.
    assert_eq!(u.get(&format!("{CAP4}Books")).status, 200);
    let m = u.get(&format!("{RAP}$metadata"));
    assert_eq!(m.header("Content-Type"), Some("application/xml"));
    assert!(m.text().contains("Common.DraftRoot"));
}

#[test]
fn v2_reads() {
    let mut u = User::new();
    let r = u.get(&format!(
        "{GW}SalesOrderSet?$inlinecount=allpages&$top=2&$orderby=GrossAmount desc"
    ));
    assert_eq!(r.status, 200);
    assert_eq!(r.header("DataServiceVersion"), Some("2.0"));
    let d = &r.json()["d"];
    assert_eq!(d["__count"], "3");
    let rows = d["results"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // Decimal strings compare as strings in $orderby; both are the two largest anyway.
    let meta = &rows[0]["__metadata"];
    assert_eq!(meta["type"], "ZRT_SALES_SRV.SalesOrder");
    assert!(
        meta["uri"]
            .as_str()
            .unwrap()
            .contains("/SalesOrderSet('05000000")
    );
    assert!(
        meta["etag"]
            .as_str()
            .unwrap()
            .starts_with("W/\"datetimeoffset'")
    );
    assert!(rows[0]["ToItems"]["__deferred"]["uri"].is_string());

    let r = u.get(&format!("{GW}SalesOrderSet('0500000002')"));
    assert_eq!(r.json()["d"]["Note"], "Rush order");
    assert_eq!(
        r.header("ETag"),
        r.json()["d"]["__metadata"]["etag"].as_str()
    );

    let r = u.get(&format!(
        "{GW}SalesOrderItemSet(SalesOrderID='0500000001',ItemPosition='000020')"
    ));
    assert_eq!(r.json()["d"]["ProductID"], "RT-1001");
    let r = u.get(&format!(
        "{GW}SalesOrderSet('0500000001')/ToItems?$select=ItemPosition"
    ));
    let items = r.json()["d"]["results"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    assert!(items[0].get("ProductID").is_none());
    let r = u.get(&format!("{GW}SalesOrderSet('0500000001')/ToCustomer"));
    assert_eq!(r.json()["d"]["City"], "Linz");
    let r = u.get(&format!(
        "{GW}SalesOrderSet?$filter=Currency%20eq%20'EUR'%20or%20Currency%20eq%20'GBP'"
    ));
    assert_eq!(r.json()["d"]["results"].as_array().unwrap().len(), 2);
    let r = u.get(&format!("{GW}SalesOrderSet/$count"));
    assert_eq!(r.text(), "3");
    let r = u.get(&format!("{GW}SalesOrderSet('0500000001')?$expand=ToItems"));
    assert_eq!(
        r.json()["d"]["ToItems"]["results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let r = u.get(&format!("{GW}SalesOrderSet('9999999999')"));
    assert_eq!(r.status, 404);
    assert!(r.json()["error"]["message"]["value"].is_string());

    let r = u.get(&format!(
        "{CAP2}Books?$filter=stock gt 100&$orderby=ID desc"
    ));
    let ids: Vec<Value> = r.json()["d"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["ID"].clone())
        .collect();
    assert_eq!(ids, vec![json!(252), json!(251)]);
    let r = u.get(&format!("{CAP2}Books(201)"));
    assert_eq!(
        r.json()["d"]["__metadata"]["etag"],
        "W/\"2026-01-15T09:30:00.000Z\""
    );
}

#[test]
fn v4_reads() {
    let mut u = User::new();
    let r = u.get(&format!(
        "{CAP4}Books?$count=true&$orderby=price desc&$top=2&$select=title"
    ));
    assert_eq!(r.header("OData-Version"), Some("4.0"));
    let b = r.json();
    assert_eq!(b["@odata.context"], "$metadata#Books");
    assert_eq!(b["@odata.count"], 5);
    let rows = b["value"].as_array().unwrap();
    assert_eq!(rows[0]["title"], "Paper Cranes");
    assert_eq!(rows[0]["ID"], 271, "keys stay in V4 selects");
    assert!(rows[0].get("stock").is_none());
    assert!(rows[0]["@odata.etag"].is_string());

    let named = u
        .get(&format!("{CAP4}Books(ID=201,IsActiveEntity=true)"))
        .json();
    let bare = u.get(&format!("{CAP4}Books(201)")).json();
    assert_eq!(named, bare);
    assert_eq!(named["@odata.context"], "$metadata#Books/$entity");
    assert_eq!(named["IsActiveEntity"], true);
    let r = u.get(&format!("{CAP4}Authors(150)/books?$skip=1"));
    assert_eq!(r.json()["value"].as_array().unwrap().len(), 1);
    let r = u.get(&format!("{CAP4}Books(ID=201,IsActiveEntity=true)/author"));
    assert_eq!(r.json()["name"], "Ada Pennington");
    assert_eq!(
        u.get(&format!(
            "{CAP4}Books/$count?$filter=currency_code eq 'EUR'"
        ))
        .text(),
        "2"
    );

    let r = u.get(&format!(
        "{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=true)?$expand=_Item"
    ));
    let so = r.json();
    assert_eq!(so["SalesOrderID"], "0500000001");
    assert_eq!(so["_Item"].as_array().unwrap().len(), 1);
    assert_eq!(r.header("ETag"), Some("W/\"2026-03-02T08:00:00.0000000Z\""));
    let r = u.get(RAP);
    assert_eq!(r.json()["value"][0]["name"], "SalesOrder");
}

#[test]
fn csrf_token_is_bound_to_the_session() {
    for root in [GW, CAP4] {
        let mut u = User::new();
        let set = if root == GW {
            "SalesOrderSet"
        } else {
            "Authors"
        };
        let body = if root == GW {
            r#"{"CustomerID":"0000100001","Currency":"EUR"}"#
        } else {
            r#"{"ID":900,"name":"New Author"}"#
        };
        let path = format!("{root}{set}");
        let r = u.raw("POST", &path, &[("Content-Type", "application/json")], body);
        assert_eq!(r.status, 403);
        assert_eq!(r.header("X-CSRF-Token"), Some("Required"));
        let token = u.fetch(root);
        assert!(u.jar.header().is_some(), "the fetch sets a session cookie");
        // The same session gets the same token again.
        assert_eq!(u.fetch(root), token);
        // Token without the session cookie: refused.
        let jar = std::mem::take(&mut u.jar);
        assert_eq!(u.write("POST", &path, &[], body).status, 403);
        u.jar = jar;
        let r = u.write("POST", &path, &[], body);
        assert_eq!(r.status, 201, "{}", r.text());
        assert!(r.header("Location").unwrap().contains(set));
    }
}

#[test]
fn etags_on_s4_and_cap() {
    let mut u = User::new();
    u.fetch(GW);
    let path = format!("{GW}SalesOrderSet('0500000001')");
    let etag = u.get(&path).header("ETag").unwrap().to_owned();
    let r = u.write("MERGE", &path, &[], r#"{"Note":"x"}"#);
    assert_eq!(r.status, 428);
    let r = u.write(
        "PATCH",
        &path,
        &[("If-Match", "W/\"old\"")],
        r#"{"Note":"x"}"#,
    );
    assert_eq!(r.status, 412);
    let r = u.write(
        "PATCH",
        &path,
        &[("If-Match", &etag)],
        r#"{"Note":"changed"}"#,
    );
    assert_eq!(r.status, 204);
    let new = r.header("ETag").unwrap().to_owned();
    assert_ne!(new, etag);
    assert_eq!(u.get(&path).json()["d"]["Note"], "changed");
    // Tunnelled MERGE with the old ETag fails, with the new one passes.
    let r = u.write(
        "POST",
        &path,
        &[("X-HTTP-Method", "MERGE"), ("If-Match", &etag)],
        r#"{"Note":"y"}"#,
    );
    assert_eq!(r.status, 412);
    let r = u.write(
        "POST",
        &path,
        &[("X-HTTP-Method", "MERGE"), ("If-Match", &new)],
        r#"{"Note":"y"}"#,
    );
    assert_eq!(r.status, 204);
    // Not deletable.
    assert_eq!(
        u.write("DELETE", &path, &[("If-Match", "*")], "").status,
        405
    );

    u.fetch(CAP4);
    let path = format!("{CAP4}Books(ID=207,IsActiveEntity=true)");
    let r = u.write("PATCH", &path, &[], r#"{"stock":1}"#);
    assert_eq!(r.status, 200, "If-Match is optional on CAP");
    assert_eq!(r.json()["stock"], 1);
    let r = u.write(
        "PATCH",
        &path,
        &[("If-Match", "W/\"old\"")],
        r#"{"stock":2}"#,
    );
    assert_eq!(r.status, 412);
    assert_eq!(r.json()["error"]["code"], "412");
    let r = u.write("DELETE", &path, &[], "");
    assert_eq!(r.status, 204);
    assert_eq!(u.get(&path).status, 404);
}

fn http_part(
    method: &str,
    url: &str,
    content_id: Option<u32>,
    headers: &str,
    body: &str,
) -> String {
    let id = content_id
        .map(|i| format!("Content-ID: {i}\r\n"))
        .unwrap_or_default();
    format!(
        "Content-Type: application/http\r\nContent-Transfer-Encoding: binary\r\n{id}\r\n{method} {url} HTTP/1.1\r\nContent-Type: application/json\r\n{headers}\r\n{body}\r\n"
    )
}

#[test]
fn batch_changesets_are_all_or_nothing() {
    let mut u = User::new();
    u.fetch(GW);
    let body = [
        "--batch_1\r\n".to_owned(),
        http_part("GET", "SalesOrderSet('0500000001')", None, "", ""),
        "--batch_1\r\nContent-Type: multipart/mixed; boundary=cs_1\r\n\r\n--cs_1\r\n".to_owned(),
        http_part(
            "POST",
            "SalesOrderSet",
            Some(1),
            "",
            r#"{"CustomerID":"0000100002","Currency":"NOK"}"#,
        ),
        "--cs_1\r\n".to_owned(),
        http_part(
            "POST",
            "$1/ToItems",
            Some(2),
            "",
            r#"{"ProductID":"RT-9","Quantity":"1.000"}"#,
        ),
        "--cs_1--\r\n\r\n".to_owned(),
        "--batch_1\r\nContent-Type: multipart/mixed; boundary=cs_2\r\n\r\n--cs_2\r\n".to_owned(),
        http_part(
            "POST",
            "SalesOrderSet",
            Some(3),
            "",
            r#"{"CustomerID":"0000100003","Currency":"GBP"}"#,
        ),
        "--cs_2\r\n".to_owned(),
        http_part(
            "PATCH",
            "SalesOrderSet('0500000002')",
            Some(4),
            "If-Match: W/\"stale\"\r\n",
            r#"{"Note":"no"}"#,
        ),
        "--cs_2--\r\n\r\n--batch_1--\r\n".to_owned(),
    ]
    .concat();
    let r = u.write(
        "POST",
        &format!("{GW}$batch"),
        &[("Content-Type", "multipart/mixed; boundary=batch_1")],
        &body,
    );
    assert_eq!(r.status, 202, "{}", r.text());
    let text = r.text();
    assert!(
        r.header("Content-Type")
            .unwrap()
            .starts_with("multipart/mixed; boundary=batch_rt_")
    );
    assert!(text.contains("HTTP/1.1 200 OK"));
    assert_eq!(text.matches("HTTP/1.1 201 Created").count(), 2, "{text}");
    assert!(text.contains("HTTP/1.1 412 Precondition Failed"));
    // The first changeset is applied, the second rolled back.
    let r = u.get(&format!("{GW}SalesOrderSet?$inlinecount=allpages"));
    assert_eq!(r.json()["d"]["__count"], "4");
    let r = u.get(&format!("{GW}SalesOrderSet('0500000004')/ToItems"));
    let items = r.json()["d"]["results"].as_array().unwrap().clone();
    assert_eq!(items[0]["ItemPosition"], "000010");
    assert_eq!(items[0]["SalesOrderID"], "0500000004");
    assert_eq!(
        u.get(&format!("{GW}SalesOrderSet('0500000005')")).status,
        404
    );
    // Writes outside a changeset are refused in V2; LF line endings are accepted.
    let lf = "--b\nContent-Type: application/http\n\nDELETE SalesOrderItemSet(SalesOrderID='0500000001',ItemPosition='000010') HTTP/1.1\n\n\n--b--\n";
    let r = u.write(
        "POST",
        &format!("{GW}$batch"),
        &[("Content-Type", "multipart/mixed; boundary=b")],
        lf,
    );
    assert!(
        r.text().contains("HTTP/1.1 400 Bad Request"),
        "{}",
        r.text()
    );
}

#[test]
fn drafts_rap_and_cap() {
    let mut u = User::new();
    u.fetch(RAP);
    let active = format!("{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=true)");
    let draft = format!("{RAP}SalesOrder(SalesOrderUUID={UUID1},IsActiveEntity=false)");
    let ns = "com.example.zrt_salesorder";
    let r = u.write(
        "POST",
        &format!("{active}/{ns}.Edit"),
        &[],
        r#"{"PreserveChanges":true}"#,
    );
    assert_eq!(r.status, 428, "Edit needs If-Match on S/4");
    let etag = u.get(&active).header("ETag").unwrap().to_owned();
    let r = u.write(
        "POST",
        &format!("{active}/{ns}.Edit"),
        &[("If-Match", &etag)],
        r#"{"PreserveChanges":true}"#,
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["IsActiveEntity"], false);
    assert_eq!(r.json()["HasActiveEntity"], true);
    assert_eq!(u.get(&active).json()["HasDraftEntity"], true);
    let r = u.write(
        "POST",
        &format!("{active}/{ns}.Edit"),
        &[("If-Match", &etag)],
        r#"{"PreserveChanges":true}"#,
    );
    assert_eq!(r.status, 409);
    // Drafts take writes without If-Match.
    assert_eq!(
        u.write("PATCH", &draft, &[], r#"{"CustomerID":"0000100003"}"#)
            .status,
        200
    );
    assert_eq!(
        u.write("POST", &format!("{draft}/{ns}.Prepare"), &[], "{}")
            .status,
        200
    );
    let r = u.write("POST", &format!("{draft}/{ns}.Activate"), &[], "{}");
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["IsActiveEntity"], true);
    assert_eq!(r.json()["CustomerID"], "0000100003");
    assert_ne!(r.header("ETag"), Some(etag.as_str()));
    assert_eq!(u.get(&draft).status, 404);
    // Create makes a draft; discard removes it.
    let r = u.write(
        "POST",
        &format!("{RAP}SalesOrder"),
        &[],
        r#"{"CustomerID":"0000100001","Currency":"EUR"}"#,
    );
    assert_eq!(r.status, 201);
    assert_eq!(r.json()["IsActiveEntity"], false);
    let location = r.header("Location").unwrap().to_owned();
    assert!(location.ends_with("IsActiveEntity=false)"));
    let path = location
        .split_once(u.server.addr.to_string().as_str())
        .unwrap()
        .1
        .to_owned();
    assert_eq!(
        u.write("POST", &format!("{path}/{ns}.Discard"), &[], "{}")
            .status,
        204
    );
    assert_eq!(u.get(&path).status, 404);

    u.fetch(CAP4);
    let active = format!("{CAP4}Books(ID=251,IsActiveEntity=true)");
    let draft = format!("{CAP4}Books(ID=251,IsActiveEntity=false)");
    let r = u.write(
        "POST",
        &format!("{active}/AdminService.draftEdit"),
        &[],
        r#"{"PreserveChanges":true}"#,
    );
    assert_eq!(r.status, 201, "{}", r.text());
    assert_eq!(
        u.write(
            "PATCH",
            &draft,
            &[],
            r#"{"title":"Salt and Iron (2nd ed.)"}"#
        )
        .status,
        200
    );
    assert_eq!(
        u.write(
            "POST",
            &format!("{draft}/AdminService.draftPrepare"),
            &[],
            "{}"
        )
        .status,
        200
    );
    let r = u.write(
        "POST",
        &format!("{draft}/AdminService.draftActivate"),
        &[],
        "{}",
    );
    assert_eq!(r.json()["title"], "Salt and Iron (2nd ed.)");
    assert_eq!(
        u.get(&format!("{CAP4}Books?$count=true")).json()["@odata.count"],
        5
    );
    // Discard on CAP: DELETE on the draft.
    u.write(
        "POST",
        &format!("{active}/AdminService.draftEdit"),
        &[],
        "{}",
    );
    assert_eq!(u.write("DELETE", &draft, &[], "").status, 204);
    assert_eq!(u.get(&active).json()["HasDraftEntity"], false);
    // Direct create of an active instance (like CAP's draft bypass).
    let r = u.write(
        "POST",
        &format!("{CAP4}Books"),
        &[],
        r#"{"title":"Direct","IsActiveEntity":true}"#,
    );
    assert_eq!(r.json()["IsActiveEntity"], true);
    assert_eq!(r.json()["ID"], 272);
}
