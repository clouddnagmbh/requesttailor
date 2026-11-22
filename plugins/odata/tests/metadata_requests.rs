// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `metadata::requests_for`: a folder per entity set with list, read, create, update and
//! delete as the set allows, one request per operation, URLs from `{{baseUrl}}`.

use rt_plugin_odata::metadata::{parse, requests_for};
use rt_plugin_sdk::api::NewRequest;
use rt_plugin_sdk::model::{Body, ParamKind, RawKind};

const S4_V2: &str = include_str!("../testdata/s4/ZRT_SALES_SRV.edmx");
const S4_V4: &str = include_str!("../testdata/s4/zrt_salesorder_0001.edmx");
const CAP_V4: &str = include_str!("../testdata/cap-bookshop/admin-v4.edmx");
const CAP_V2: &str = include_str!("../testdata/cap-bookshop/admin-v2.edmx");

fn summary(requests: &[NewRequest]) -> Vec<(String, String, String, String)> {
    requests
        .iter()
        .map(|r| {
            (
                r.folder.clone(),
                r.name.clone(),
                r.spec.method.clone(),
                r.spec.url.clone(),
            )
        })
        .collect()
}

fn find<'r>(requests: &'r [NewRequest], name: &str) -> &'r NewRequest {
    requests
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no request {name}"))
}

fn body_json(r: &NewRequest) -> serde_json::Value {
    match &r.spec.body {
        Body::Raw {
            kind: RawKind::Json,
            data,
        } => serde_json::from_str(data).expect("bodies are JSON"),
        other => panic!("{}: JSON body expected, got {other:?}", r.name),
    }
}

/// Top-level member names of a JSON body, in the order written.
fn body_keys(r: &NewRequest) -> Vec<String> {
    let Body::Raw { data, .. } = &r.spec.body else {
        panic!("no body");
    };
    data.lines()
        .filter(|l| l.starts_with("  \"") && !l.starts_with("   "))
        .map(|l| l.trim().split('"').nth(1).unwrap().to_owned())
        .collect()
}

fn row(folder: &str, name: &str, method: &str, url: &str) -> (String, String, String, String) {
    (folder.into(), name.into(), method.into(), url.into())
}

#[test]
fn s4_gateway_v2() {
    let m = parse(S4_V2, "https://h/sap/opu/odata/sap/ZRT_SALES_SRV/").unwrap();
    let r = requests_for(&m);
    let so = "SalesOrderSet";
    let it = "SalesOrderItemSet";
    let cu = "CustomerSet";
    assert_eq!(
        summary(&r),
        vec![
            row(
                so,
                "List SalesOrderSet",
                "GET",
                "{{baseUrl}}SalesOrderSet?$top=20&$format=json"
            ),
            row(
                so,
                "Read SalesOrderSet",
                "GET",
                "{{baseUrl}}SalesOrderSet('1')?$format=json"
            ),
            row(
                so,
                "Create SalesOrderSet",
                "POST",
                "{{baseUrl}}SalesOrderSet"
            ),
            row(
                so,
                "Update SalesOrderSet",
                "PATCH",
                "{{baseUrl}}SalesOrderSet('1')"
            ),
            row(
                it,
                "List SalesOrderItemSet",
                "GET",
                "{{baseUrl}}SalesOrderItemSet?$top=20&$format=json"
            ),
            row(
                it,
                "Read SalesOrderItemSet",
                "GET",
                "{{baseUrl}}SalesOrderItemSet(SalesOrderID='1',ItemPosition='1')?$format=json"
            ),
            row(
                it,
                "Create SalesOrderItemSet",
                "POST",
                "{{baseUrl}}SalesOrderItemSet"
            ),
            row(
                it,
                "Update SalesOrderItemSet",
                "PATCH",
                "{{baseUrl}}SalesOrderItemSet(SalesOrderID='1',ItemPosition='1')"
            ),
            row(
                it,
                "Delete SalesOrderItemSet",
                "DELETE",
                "{{baseUrl}}SalesOrderItemSet(SalesOrderID='1',ItemPosition='1')"
            ),
            row(
                cu,
                "List CustomerSet",
                "GET",
                "{{baseUrl}}CustomerSet?$top=20&$format=json"
            ),
            row(
                cu,
                "Read CustomerSet",
                "GET",
                "{{baseUrl}}CustomerSet('1')?$format=json"
            ),
            row(
                "",
                "ConfirmSalesOrder",
                "POST",
                "{{baseUrl}}ConfirmSalesOrder?SalesOrderID='1'"
            ),
            row(
                "",
                "GetOpenOrderCount",
                "GET",
                "{{baseUrl}}GetOpenOrderCount?CustomerID='1'&$format=json"
            ),
        ]
    );

    // Query parameters mirror the URL; V2 writes ask for JSON.
    let list = find(&r, "List SalesOrderSet");
    let params: Vec<_> = list
        .spec
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str(), p.kind))
        .collect();
    assert_eq!(
        params,
        vec![
            ("$top", "20", ParamKind::Query),
            ("$format", "json", ParamKind::Query)
        ]
    );
    assert!(list.spec.headers.is_empty());
    assert_eq!(list.spec.body, Body::None);
    let create = find(&r, "Create SalesOrderSet");
    assert_eq!(create.spec.headers[0].name, "Accept");
    assert_eq!(create.spec.headers[0].value, "application/json");

    // Bodies: only what may be written, in property order; V2 decimals as strings.
    assert_eq!(
        body_keys(create),
        vec!["CustomerID", "Currency", "LifecycleStatus", "Note"]
    );
    assert_eq!(
        body_keys(find(&r, "Update SalesOrderSet")),
        vec!["CustomerID", "Currency", "Note"]
    );
    let item = body_json(find(&r, "Create SalesOrderItemSet"));
    assert_eq!(item["Quantity"], "0");
    assert_eq!(item["SalesOrderID"], "");
    assert!(item.get("NetAmount").is_none());

    // Names and order: `seq` counts per folder; the request names are the spec names.
    let seqs: Vec<_> = r
        .iter()
        .filter(|x| x.folder == it)
        .map(|x| x.spec.seq)
        .collect();
    assert_eq!(
        seqs,
        vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0)]
    );
    assert!(r.iter().all(|x| x.name == x.spec.name));
}

#[test]
fn s4_rap_v4_drafts_and_bound_actions() {
    let m = parse(
        S4_V4,
        "https://h/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/",
    )
    .unwrap();
    let r = requests_for(&m);
    let key = "(SalesOrderUUID=00000000-0000-0000-0000-000000000000,IsActiveEntity=true)";
    let draft_key = "(SalesOrderUUID=00000000-0000-0000-0000-000000000000,IsActiveEntity=false)";
    let so = "SalesOrder";
    let ns = "com.example.zrt_salesorder";
    let mut expected = vec![
        row(
            so,
            "List SalesOrder",
            "GET",
            "{{baseUrl}}SalesOrder?$top=20",
        ),
        row(
            so,
            "Read SalesOrder",
            "GET",
            &format!("{{{{baseUrl}}}}SalesOrder{key}"),
        ),
        row(so, "Create SalesOrder", "POST", "{{baseUrl}}SalesOrder"),
        // Drafts change only drafts.
        row(
            so,
            "Update SalesOrder",
            "PATCH",
            &format!("{{{{baseUrl}}}}SalesOrder{draft_key}"),
        ),
    ];
    let items = summary(&r)
        .into_iter()
        .filter(|x| x.0 == "SalesOrderItem")
        .count();
    assert_eq!(items, 5);
    for action in ["Edit", "Prepare", "Activate", "Discard", "confirm"] {
        expected.push(row(
            so,
            action,
            "POST",
            &format!("{{{{baseUrl}}}}SalesOrder{key}/{ns}.{action}"),
        ));
    }
    let got: Vec<_> = summary(&r).into_iter().filter(|x| x.0 == so).collect();
    assert_eq!(got, expected);

    // No draft administration fields, no computed fields in bodies.
    assert_eq!(
        body_keys(find(&r, "Create SalesOrder")),
        vec![
            "SalesOrderUUID",
            "CustomerID",
            "GrossAmount",
            "Currency",
            "OverallStatus"
        ]
    );
    assert_eq!(
        body_keys(find(&r, "Update SalesOrder")),
        vec!["CustomerID", "GrossAmount", "Currency", "OverallStatus"]
    );
    assert_eq!(body_json(find(&r, "Create SalesOrder"))["GrossAmount"], 0);
    assert_eq!(
        body_json(find(&r, "Edit")),
        serde_json::json!({ "PreserveChanges": false })
    );
    assert_eq!(body_json(find(&r, "Prepare")), serde_json::json!({}));
    // V4 requests carry no extra headers.
    assert!(r.iter().all(|x| x.spec.headers.is_empty()));
}

#[test]
fn cap_bookshop_v4_and_v2_functions() {
    let m = parse(CAP_V4, "http://localhost:4004/odata/v4/admin/").unwrap();
    let r = requests_for(&m);
    let f = find(&r, "booksInStock");
    assert_eq!((f.folder.as_str(), f.spec.method.as_str()), ("", "GET"));
    assert_eq!(f.spec.url, "{{baseUrl}}booksInStock(min=1)");
    let edit = find(&r, "draftEdit");
    assert_eq!(edit.folder, "Books");
    assert_eq!(
        edit.spec.url,
        "{{baseUrl}}Books(ID=1,IsActiveEntity=true)/AdminService.draftEdit"
    );
    let create = body_json(find(&r, "Create Books"));
    assert_eq!(
        create,
        serde_json::json!({
            "ID": 0, "title": "", "descr": "", "author_ID": 0, "genre_ID": 0,
            "stock": 0, "price": 0, "currency_code": ""
        })
    );
    let guid = find(&r, "Read DraftAdministrativeData");
    assert_eq!(
        guid.spec.url,
        "{{baseUrl}}DraftAdministrativeData(00000000-0000-0000-0000-000000000000)"
    );
    assert_eq!(r.len(), 4 * 5 + 4);

    let m = parse(CAP_V2, "http://localhost:4004/odata/v2/admin/").unwrap();
    let r = requests_for(&m);
    let f = find(&r, "booksInStock");
    assert_eq!(f.spec.url, "{{baseUrl}}booksInStock?min=1&$format=json");
    assert_eq!(
        find(&r, "Read Books").spec.url,
        "{{baseUrl}}Books(1)?$format=json"
    );
    assert_eq!(r.len(), 3 * 5 + 1);
}

#[test]
fn complex_properties_get_nested_bodies() {
    let m = parse(S4_V2, "https://h/s/").unwrap();
    let mut m = m;
    // CustomerSet is read-only in the service; open it up to see the body.
    m.entity_sets[2].insertable = true;
    let r = requests_for(&m);
    assert_eq!(
        body_json(find(&r, "Create CustomerSet")),
        serde_json::json!({
            "CustomerID": "", "Name": "", "City": "", "Country": "",
            "Address": { "Street": "", "PostalCode": "" }
        })
    );
}
