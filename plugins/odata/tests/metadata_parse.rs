// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `metadata::parse` on the four synthetic services of `testdata` (see SOURCES.md): entity
//! sets, keys, navigation, capabilities, drafts, annotations and operations, V2 and V4.

use rt_plugin_odata::edm::{Model, OperationKind, Property, Version};
use rt_plugin_odata::metadata::parse;

const S4_V2: &str = include_str!("../testdata/s4/ZRT_SALES_SRV.edmx");
const S4_V4: &str = include_str!("../testdata/s4/zrt_salesorder_0001.edmx");
const CAP_V4: &str = include_str!("../testdata/cap-bookshop/admin-v4.edmx");
const CAP_V2: &str = include_str!("../testdata/cap-bookshop/admin-v2.edmx");

fn sets(m: &Model) -> Vec<(&str, &str, bool, bool, bool, bool)> {
    m.entity_sets
        .iter()
        .map(|s| {
            (
                s.name.as_str(),
                s.entity_type.as_str(),
                s.insertable,
                s.updatable,
                s.deletable,
                s.draft,
            )
        })
        .collect()
}

fn property<'m>(m: &'m Model, ty: &str, name: &str) -> &'m Property {
    m.entity_type(ty)
        .unwrap_or_else(|| panic!("type {ty}"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{ty}/{name}"))
}

fn annotation<'p>(p: &'p Property, name: &str) -> Option<&'p str> {
    p.annotations
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn navigation(m: &Model, ty: &str) -> Vec<(String, String, bool)> {
    m.entity_type(ty)
        .unwrap()
        .navigation
        .iter()
        .map(|n| (n.name.clone(), n.target.clone(), n.many))
        .collect()
}

/// Name, kind, bound to, parameter names, return type.
type OperationRow<'m> = (
    &'m str,
    OperationKind,
    Option<&'m str>,
    Vec<&'m str>,
    Option<&'m str>,
);

fn operations(m: &Model) -> Vec<OperationRow<'_>> {
    m.operations
        .iter()
        .map(|o| {
            (
                o.name.as_str(),
                o.kind,
                o.bound_to.as_deref(),
                o.parameters.iter().map(|p| p.name.as_str()).collect(),
                o.returns.as_deref(),
            )
        })
        .collect()
}

#[test]
fn s4_gateway_v2() {
    let root = "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/";
    let m = parse(S4_V2, root).unwrap();
    assert_eq!(m.version, Version::V2);
    assert_eq!(m.service_root, root);
    assert_eq!(m.namespace, "ZRT_SALES_SRV");
    assert_eq!(
        sets(&m),
        vec![
            (
                "SalesOrderSet",
                "ZRT_SALES_SRV.SalesOrder",
                true,
                true,
                false,
                false
            ),
            (
                "SalesOrderItemSet",
                "ZRT_SALES_SRV.SalesOrderItem",
                true,
                true,
                true,
                false
            ),
            (
                "CustomerSet",
                "ZRT_SALES_SRV.Customer",
                false,
                false,
                false,
                false
            ),
        ]
    );
    assert_eq!(m.entity_types.len(), 3);
    let item = m.entity_type("ZRT_SALES_SRV.SalesOrderItem").unwrap();
    assert_eq!(item.key, vec!["SalesOrderID", "ItemPosition"]);
    assert!(!item.media);

    // Associations give target and multiplicity.
    assert_eq!(
        navigation(&m, "ZRT_SALES_SRV.SalesOrder"),
        vec![
            (
                "ToItems".into(),
                "ZRT_SALES_SRV.SalesOrderItem".into(),
                true
            ),
            ("ToCustomer".into(), "ZRT_SALES_SRV.Customer".into(), false),
        ]
    );
    assert_eq!(
        navigation(&m, "ZRT_SALES_SRV.SalesOrderItem"),
        vec![(
            "ToSalesOrder".into(),
            "ZRT_SALES_SRV.SalesOrder".into(),
            false
        )]
    );

    // Properties: facets and sap: annotations.
    let id = property(&m, "ZRT_SALES_SRV.SalesOrder", "SalesOrderID");
    assert_eq!((id.r#type.as_str(), id.nullable), ("Edm.String", false));
    assert_eq!(id.max_length, Some(10));
    assert_eq!(
        id.annotations,
        vec![
            ("label".into(), "Sales Order".into()),
            ("creatable".into(), "false".into()),
            ("updatable".into(), "false".into()),
        ]
    );
    let amount = property(&m, "ZRT_SALES_SRV.SalesOrder", "GrossAmount");
    assert_eq!(
        (amount.precision, amount.scale.as_deref()),
        (Some(15), Some("2"))
    );
    assert_eq!(annotation(amount, "unit"), Some("Currency"));
    let note = property(&m, "ZRT_SALES_SRV.SalesOrder", "Note");
    assert_eq!(annotation(note, "filterable"), Some("false"));
    assert!(note.nullable);
    let changed = property(&m, "ZRT_SALES_SRV.SalesOrder", "ChangedAt");
    assert_eq!(annotation(changed, "etag"), Some("true"));
    let address = property(&m, "ZRT_SALES_SRV.Customer", "Address");
    assert_eq!(address.r#type, "ZRT_SALES_SRV.Address");

    let complex: Vec<_> = m.complex_types.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        complex,
        vec!["ZRT_SALES_SRV.Address", "ZRT_SALES_SRV.ConfirmationResult"]
    );
    assert_eq!(m.complex_types[0].properties.len(), 2);

    // Function imports: kind by m:HttpMethod.
    assert_eq!(
        operations(&m),
        vec![
            (
                "ConfirmSalesOrder",
                OperationKind::Action,
                None,
                vec!["SalesOrderID"],
                Some("ZRT_SALES_SRV.ConfirmationResult")
            ),
            (
                "GetOpenOrderCount",
                OperationKind::Function,
                None,
                vec!["CustomerID"],
                Some("Edm.Int32")
            ),
        ]
    );
}

#[test]
fn s4_rap_v4_with_draft() {
    let root = "https://s4.example.com/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";
    let m = parse(S4_V4, root).unwrap();
    assert_eq!(m.version, Version::V4);
    assert_eq!(m.namespace, "com.example.zrt_salesorder");
    // Capabilities.DeleteRestrictions and Common.DraftRoot through the `SAP__self` alias.
    assert_eq!(
        sets(&m),
        vec![
            (
                "SalesOrder",
                "com.example.zrt_salesorder.SalesOrderType",
                true,
                true,
                false,
                true
            ),
            (
                "SalesOrderItem",
                "com.example.zrt_salesorder.SalesOrderItemType",
                true,
                true,
                true,
                false
            ),
        ]
    );
    let order = m
        .entity_type("com.example.zrt_salesorder.SalesOrderType")
        .unwrap();
    assert_eq!(order.key, vec!["SalesOrderUUID", "IsActiveEntity"]);
    assert_eq!(order.properties.len(), 10);
    assert_eq!(
        navigation(&m, "com.example.zrt_salesorder.SalesOrderType"),
        vec![
            (
                "_Item".into(),
                "com.example.zrt_salesorder.SalesOrderItemType".into(),
                true
            ),
            (
                "SiblingEntity".into(),
                "com.example.zrt_salesorder.SalesOrderType".into(),
                false
            ),
        ]
    );
    let id = property(
        &m,
        "com.example.zrt_salesorder.SalesOrderType",
        "SalesOrderID",
    );
    assert_eq!(
        id.annotations,
        vec![
            ("label".into(), "Sales Order".into()),
            ("computed".into(), "true".into()),
        ]
    );
    let changed = property(
        &m,
        "com.example.zrt_salesorder.SalesOrderType",
        "LastChangedAt",
    );
    assert_eq!(annotation(changed, "computed"), Some("true"));
    assert_eq!(annotation(changed, "etag"), Some("true"));
    let customer = property(
        &m,
        "com.example.zrt_salesorder.SalesOrderType",
        "CustomerID",
    );
    assert_eq!(annotation(customer, "label"), Some("Customer"));
    assert_eq!(annotation(customer, "computed"), None);

    // Bound actions: binding parameter removed, `bound_to` its type.
    let bound = Some("com.example.zrt_salesorder.SalesOrderType");
    let ret = Some("com.example.zrt_salesorder.SalesOrderType");
    assert_eq!(
        operations(&m),
        vec![
            (
                "Edit",
                OperationKind::Action,
                bound,
                vec!["PreserveChanges"],
                ret
            ),
            ("Prepare", OperationKind::Action, bound, vec![], ret),
            ("Activate", OperationKind::Action, bound, vec![], ret),
            ("Discard", OperationKind::Action, bound, vec![], None),
            ("confirm", OperationKind::Action, bound, vec![], ret),
        ]
    );
}

#[test]
fn cap_bookshop_v4() {
    let m = parse(CAP_V4, "http://localhost:4004/odata/v4/admin/").unwrap();
    assert_eq!(m.version, Version::V4);
    assert_eq!(m.namespace, "AdminService");
    let names: Vec<_> = sets(&m).into_iter().map(|s| (s.0, s.5)).collect();
    assert_eq!(
        names,
        vec![
            ("Books", true),
            ("Authors", false),
            ("Genres", false),
            ("DraftAdministrativeData", false)
        ]
    );
    let books = m.entity_type("AdminService.Books").unwrap();
    assert_eq!(books.key, vec!["ID", "IsActiveEntity"]);
    assert_eq!(
        navigation(&m, "AdminService.Authors"),
        vec![("books".into(), "AdminService.Books".into(), true)]
    );
    assert_eq!(
        navigation(&m, "AdminService.Books")[0],
        ("author".into(), "AdminService.Authors".into(), false)
    );
    assert_eq!(
        annotation(property(&m, "AdminService.Books", "title"), "label"),
        Some("Title")
    );
    assert_eq!(
        annotation(property(&m, "AdminService.Books", "modifiedAt"), "computed"),
        Some("true")
    );
    assert_eq!(
        annotation(property(&m, "AdminService.Books", "stock"), "computed"),
        None
    );
    let books_type = Some("AdminService.Books");
    assert_eq!(
        operations(&m),
        vec![
            (
                "draftPrepare",
                OperationKind::Action,
                books_type,
                vec!["SideEffectsQualifier"],
                books_type
            ),
            (
                "draftActivate",
                OperationKind::Action,
                books_type,
                vec![],
                books_type
            ),
            (
                "draftEdit",
                OperationKind::Action,
                books_type,
                vec!["PreserveChanges"],
                books_type
            ),
            (
                "booksInStock",
                OperationKind::Function,
                None,
                vec!["min"],
                Some("Collection(AdminService.Books)")
            ),
        ]
    );
}

#[test]
fn cap_bookshop_v2() {
    let m = parse(CAP_V2, "http://localhost:4004/odata/v2/admin").unwrap();
    assert_eq!(m.version, Version::V2);
    assert_eq!(m.service_root, "http://localhost:4004/odata/v2/admin/");
    assert_eq!(
        sets(&m),
        vec![
            ("Books", "AdminService.Books", true, true, true, false),
            ("Authors", "AdminService.Authors", true, true, true, false),
            ("Genres", "AdminService.Genres", true, true, true, false),
        ]
    );
    assert_eq!(m.entity_type("AdminService.Books").unwrap().key, vec!["ID"]);
    // Multiplicity `0..1` is to-one, `*` to-many.
    assert_eq!(
        navigation(&m, "AdminService.Books"),
        vec![("author".into(), "AdminService.Authors".into(), false)]
    );
    assert_eq!(
        navigation(&m, "AdminService.Authors"),
        vec![("books".into(), "AdminService.Books".into(), true)]
    );
    assert_eq!(
        annotation(property(&m, "AdminService.Books", "modifiedAt"), "etag"),
        Some("true")
    );
    assert_eq!(
        annotation(
            property(&m, "AdminService.Authors", "dateOfBirth"),
            "display-format"
        ),
        Some("Date")
    );
    assert_eq!(
        operations(&m),
        vec![(
            "booksInStock",
            OperationKind::Function,
            None,
            vec!["min"],
            Some("Collection(AdminService.Books)")
        )]
    );
}

#[test]
fn the_model_survives_json() {
    // The cache stores `serde_json::to_value(model)`; every field must come back.
    for (xml, root) in [
        (S4_V2, "https://h/a/"),
        (S4_V4, "https://h/b/"),
        (CAP_V4, "https://h/c/"),
        (CAP_V2, "https://h/d/"),
    ] {
        let m = parse(xml, root).unwrap();
        let back: Model = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }
}

#[test]
fn errors_name_the_problem() {
    assert!(parse("{\"$Version\":\"4.0\"}", "").is_err());
    let e = parse(
        "<edmx:Edmx xmlns:edmx='x' Version='4.0'><a></b></edmx:Edmx>",
        "",
    )
    .unwrap_err()
    .into_message();
    assert!(e.contains("XML line 1"), "{e}");
}
