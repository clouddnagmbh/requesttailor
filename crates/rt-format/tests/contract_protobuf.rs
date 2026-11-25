// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Breaking changes of Protobuf files (wave 18, strand H): every rule of the contract, breaking
//! and not breaking, the synthetic Orders protocol in `testdata/contract/protobuf`, the requests
//! a change affects, and hostile input.

use std::path::PathBuf;

use rt_format::FileSet;
use rt_format::contract::{
    ContractError, SchemaKind, SchemaSources, SpecChange, SpecChangeKind, SpecChanges,
    schema_changes,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn sources(proto: &str) -> SchemaSources {
    let mut s = SchemaSources::new();
    s.insert("a.proto".into(), proto.as_bytes().to_vec());
    s
}

fn empty_collection() -> FileSet {
    FileSet {
        files: [(
            "opencollection.yml".to_owned(),
            b"opencollection: 1.0.0\ninfo:\n  name: T\n".to_vec(),
        )]
        .into(),
    }
}

fn diff(old: &str, new: &str) -> SpecChanges {
    schema_changes(
        SchemaKind::Protobuf,
        Some(&sources(old)),
        Some(&sources(new)),
        &empty_collection(),
    )
    .unwrap()
}

fn at<'a>(c: &'a SpecChanges, op: &str) -> Vec<&'a SpecChange> {
    c.changes
        .iter()
        .filter(|x| x.operation.as_deref() == Some(op))
        .collect()
}

fn one<'a>(c: &'a SpecChanges, op: &str) -> &'a SpecChange {
    let found = at(c, op);
    assert_eq!(found.len(), 1, "changes at {op}: {:#?}", c.changes);
    found[0]
}

fn breaking_ops(c: &SpecChanges) -> Vec<&str> {
    c.breaking()
        .map(|x| x.operation.as_deref().unwrap_or(""))
        .collect()
}

const BASE: &str = r#"syntax = "proto3";
package t;

service Svc {
  rpc Get(Req) returns (Msg);
  rpc Stream(Req) returns (stream Msg);
}

message Req { string id = 1; }

message Msg {
  string name = 1;
  int32 count = 2;
  repeated string tags = 3;
  Sub sub = 4;
  oneof pick { string a = 5; string b = 6; }
  map<string, string> labels = 7;
  Kind kind = 8;
  message Sub { string x = 1; }
}

enum Kind { KIND_UNSPECIFIED = 0; ONE = 1; TWO = 2; }
"#;

fn with(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "{from:?} not in the base");
    BASE.replace(from, to)
}

// ---- breaking rules -------------------------------------------------------------------

#[test]
fn a_changed_field_number_is_breaking() {
    let c = diff(BASE, &with("string name = 1;", "string name = 9;"));
    let f = one(&c, "t.Msg.name");
    assert_eq!(f.kind, SpecChangeKind::NumberChanged);
    assert!(f.breaking);
    assert!(
        f.message.contains('1') && f.message.contains('9'),
        "{}",
        f.message
    );
}

#[test]
fn a_reused_field_number_is_breaking() {
    // `count` (int32) is gone, number 2 now holds a message: readers misparse old data.
    let c = diff(BASE, &with("int32 count = 2;", "Sub other = 2;"));
    assert!(
        c.breaking()
            .any(|x| x.kind == SpecChangeKind::NumberChanged),
        "{:#?}",
        c.changes
    );
    assert!(at(&c, "t.Msg.other").iter().any(|x| x.breaking));
}

#[test]
fn a_removed_field_is_breaking_and_says_whether_the_number_is_reserved() {
    let c = diff(BASE, &with("  int32 count = 2;\n", ""));
    let f = one(&c, "t.Msg.count");
    assert_eq!(f.kind, SpecChangeKind::FieldRemoved);
    assert!(f.breaking);
    assert!(f.message.contains("not reserved"), "{}", f.message);
    let reserved = with(
        "  int32 count = 2;\n",
        "  reserved 2;\n  reserved \"count\";\n",
    );
    let c = diff(BASE, &reserved);
    let f = one(&c, "t.Msg.count");
    assert!(
        f.breaking,
        "removing a field breaks readers even with reserved"
    );
    assert!(!f.message.contains("not reserved"), "{}", f.message);
    assert!(f.message.contains("reserved"), "{}", f.message);
}

#[test]
fn an_incompatible_type_change_is_breaking() {
    for (from, to) in [
        ("int32 count = 2;", "string count = 2;"),
        ("int32 count = 2;", "sint32 count = 2;"),
        ("int32 count = 2;", "fixed32 count = 2;"),
        ("string name = 1;", "int32 name = 1;"),
        ("Sub sub = 4;", "string sub = 4;"),
        ("Kind kind = 8;", "string kind = 8;"),
        (
            "map<string, string> labels = 7;",
            "map<string, int32> labels = 7;",
        ),
    ] {
        let c = diff(BASE, &with(from, to));
        let name = to.split_whitespace().nth_back(2).unwrap_or("?");
        let f = c
            .changes
            .iter()
            .find(|x| x.kind == SpecChangeKind::TypeChanged);
        let f = f.unwrap_or_else(|| panic!("{from} -> {to}: {:#?}", c.changes));
        assert!(f.breaking, "{from} -> {to} ({name})");
    }
}

#[test]
fn a_message_type_swapped_for_another_is_breaking() {
    let c = diff(BASE, &with("Sub sub = 4;", "Req sub = 4;"));
    assert!(one(&c, "t.Msg.sub").breaking);
}

#[test]
fn repeated_and_optional_switches_are_breaking() {
    let c = diff(BASE, &with("repeated string tags = 3;", "string tags = 3;"));
    let f = one(&c, "t.Msg.tags");
    assert_eq!(f.kind, SpecChangeKind::NullabilityChanged);
    assert!(f.breaking);
    let c = diff(BASE, &with("string name = 1;", "repeated string name = 1;"));
    assert!(one(&c, "t.Msg.name").breaking);
    let c = diff(BASE, &with("int32 count = 2;", "optional int32 count = 2;"));
    let f = one(&c, "t.Msg.count");
    assert_eq!(f.kind, SpecChangeKind::NullabilityChanged);
    assert!(f.breaking);
}

#[test]
fn removed_messages_enums_and_values_are_breaking() {
    let c = diff(
        BASE,
        &with("message Req { string id = 1; }\n", "")
            .replace("(Req)", "(Msg)")
            .replace("(Req)", "(Msg)"),
    );
    assert_eq!(one(&c, "t.Req").kind, SpecChangeKind::TypeRemoved);
    assert!(one(&c, "t.Req").breaking);
    let c = diff(
        BASE,
        &with(
            "enum Kind { KIND_UNSPECIFIED = 0; ONE = 1; TWO = 2; }\n",
            "",
        )
        .replace("  Kind kind = 8;\n", ""),
    );
    assert!(one(&c, "t.Kind").breaking);
    let c = diff(BASE, &with("TWO = 2;", ""));
    let v = one(&c, "t.Kind.TWO");
    assert_eq!(v.kind, SpecChangeKind::EnumValueRemoved);
    assert!(v.breaking);
}

#[test]
fn a_removed_nested_message_is_reported_once_under_its_name() {
    let c = diff(
        BASE,
        &with("  message Sub { string x = 1; }\n", "").replace("  Sub sub = 4;\n", ""),
    );
    assert!(one(&c, "t.Msg.Sub").breaking);
    assert!(one(&c, "t.Msg.sub").breaking);
}

#[test]
fn a_removed_rpc_or_service_is_breaking() {
    let c = diff(BASE, &with("  rpc Stream(Req) returns (stream Msg);\n", ""));
    let r = one(&c, "/t.Svc/Stream");
    assert_eq!(r.kind, SpecChangeKind::OperationRemoved);
    assert!(r.breaking);
    let gone = with(
        "service Svc {\n  rpc Get(Req) returns (Msg);\n  rpc Stream(Req) returns (stream Msg);\n}\n",
        "",
    );
    let c = diff(BASE, &gone);
    let s = one(&c, "t.Svc");
    assert_eq!(s.kind, SpecChangeKind::TypeRemoved);
    assert!(s.breaking);
}

#[test]
fn a_changed_streaming_kind_is_breaking() {
    let c = diff(BASE, &with("rpc Get(Req)", "rpc Get(stream Req)"));
    let r = one(&c, "/t.Svc/Get");
    assert_eq!(r.kind, SpecChangeKind::StreamingChanged);
    assert!(r.breaking);
    let c = diff(BASE, &with("returns (stream Msg)", "returns (Msg)"));
    assert!(one(&c, "/t.Svc/Stream").breaking);
}

#[test]
fn a_changed_rpc_message_is_breaking() {
    let c = diff(
        BASE,
        &with("rpc Get(Req) returns (Msg);", "rpc Get(Msg) returns (Msg);"),
    );
    let r = one(&c, "/t.Svc/Get");
    assert_eq!(r.kind, SpecChangeKind::TypeChanged);
    assert!(r.breaking);
}

#[test]
fn a_changed_package_is_breaking_once_and_does_not_remove_everything() {
    let c = diff(BASE, &with("package t;", "package t2;"));
    let p = one(&c, "package t");
    assert!(p.breaking);
    assert!(p.message.contains("t2"), "{}", p.message);
    assert_eq!(c.changes.len(), 1, "{:#?}", c.changes);
    // and a change inside is still found against the new package
    let c = diff(
        BASE,
        &with("package t;", "package t2;").replace("  int32 count = 2;\n", ""),
    );
    assert!(one(&c, "t2.Msg.count").breaking);
    assert_eq!(c.breaking().count(), 2);
}

#[test]
fn a_renamed_service_is_a_removed_one() {
    let c = diff(BASE, &with("service Svc", "service Svc2"));
    assert!(one(&c, "t.Svc").breaking);
    assert!(!one(&c, "t.Svc2").breaking);
}

#[test]
fn a_oneof_member_moved_is_breaking() {
    let moved = with(
        "oneof pick { string a = 5; string b = 6; }",
        "string a = 5; oneof pick { string b = 6; }",
    );
    let c = diff(BASE, &moved);
    let f = one(&c, "t.Msg.a");
    assert!(f.breaking);
    assert!(f.message.contains("oneof"), "{}", f.message);
    let other = with(
        "oneof pick { string a = 5; string b = 6; }",
        "oneof pick { string a = 5; } oneof second { string b = 6; }",
    );
    assert!(one(&diff(BASE, &other), "t.Msg.b").breaking);
}

// ---- rules that do not break ----------------------------------------------------------

#[test]
fn wire_compatible_type_changes_are_a_hint() {
    for (from, to) in [
        ("int32 count = 2;", "int64 count = 2;"),
        ("int32 count = 2;", "uint32 count = 2;"),
        ("int32 count = 2;", "bool count = 2;"),
        ("string name = 1;", "bytes name = 1;"),
    ] {
        let c = diff(BASE, &with(from, to));
        let f = c
            .changes
            .iter()
            .find(|x| x.kind == SpecChangeKind::TypeChanged)
            .unwrap_or_else(|| panic!("{from} -> {to}"));
        assert!(!f.breaking, "{from} -> {to}");
        assert!(f.message.contains("wire"), "{}", f.message);
    }
    let c = diff(
        &with("int32 count = 2;", "sint32 count = 2;"),
        &with("int32 count = 2;", "sint64 count = 2;"),
    );
    assert!(!one(&c, "t.Msg.count").breaking);
    let c = diff(
        &with("int32 count = 2;", "fixed32 count = 2;"),
        &with("int32 count = 2;", "sfixed32 count = 2;"),
    );
    assert!(!one(&c, "t.Msg.count").breaking);
}

#[test]
fn a_renamed_field_is_a_hint_about_json() {
    let c = diff(BASE, &with("string name = 1;", "string title = 1;"));
    let f = one(&c, "t.Msg.title");
    assert!(!f.breaking);
    assert!(f.message.contains("JSON"), "{}", f.message);
    assert!(c.breaking().next().is_none(), "{:#?}", c.changes);
}

#[test]
fn additions_are_not_breaking() {
    let new = with(
        "  int32 count = 2;\n",
        "  int32 count = 2;\n  string extra = 20;\n",
    )
    .replace("TWO = 2;", "TWO = 2; THREE = 3;")
    .replace(
        "  rpc Get(Req) returns (Msg);",
        "  rpc Get(Req) returns (Msg);\n  rpc Put(Msg) returns (Req);",
    ) + "message Added { string x = 1; }\nservice Other { rpc Do(Req) returns (Req); }\nenum NewEnum { N_UNSPECIFIED = 0; }\n";
    let c = diff(BASE, &new);
    assert!(c.breaking().next().is_none(), "{:#?}", c.changes);
    assert_eq!(one(&c, "t.Msg.extra").kind, SpecChangeKind::FieldAdded);
    assert_eq!(one(&c, "/t.Svc/Put").kind, SpecChangeKind::OperationAdded);
    assert!(!one(&c, "t.Kind.THREE").breaking);
    assert!(!one(&c, "t.Added").breaking);
    assert!(!one(&c, "t.Other").breaking);
    assert!(!one(&c, "t.NewEnum").breaking);
}

#[test]
fn reserving_a_number_is_not_breaking() {
    let c = diff(
        BASE,
        &with(
            "  int32 count = 2;\n  repeated",
            "  int32 count = 2;\n  reserved 15, 20 to 25;\n  repeated",
        ),
    );
    assert!(c.breaking().next().is_none(), "{:#?}", c.changes);
}

#[test]
fn comments_and_layout_are_no_change() {
    let c = diff(
        BASE,
        &format!("// a comment\n{}\n/* more */\n", BASE.replace("  ", "\t")),
    );
    assert!(c.changes.is_empty(), "{:#?}", c.changes);
}

#[test]
fn a_missing_side_counts_as_empty() {
    let added = schema_changes(
        SchemaKind::Protobuf,
        None,
        Some(&sources(BASE)),
        &empty_collection(),
    )
    .unwrap();
    assert!(!added.changes.is_empty() && added.breaking().next().is_none());
    let removed = schema_changes(
        SchemaKind::Protobuf,
        Some(&sources(BASE)),
        None,
        &empty_collection(),
    )
    .unwrap();
    assert!(removed.breaking().next().is_some());
    assert!(
        schema_changes(SchemaKind::Protobuf, None, None, &empty_collection())
            .unwrap()
            .changes
            .is_empty()
    );
}

#[test]
fn well_known_types_are_not_part_of_the_schema() {
    let proto = "syntax = \"proto3\";\npackage w;\nimport \"google/protobuf/timestamp.proto\";\nmessage M { google.protobuf.Timestamp at = 1; }\n";
    assert!(diff(proto, proto).changes.is_empty());
}

// ---- the synthetic Orders protocol ----------------------------------------------------

fn orders(version: u8) -> SchemaSources {
    let dir = fixtures().join("protobuf").join(format!("v{version}"));
    let mut s = SchemaSources::new();
    for name in ["orders.proto", "common/money.proto"] {
        s.insert(
            format!("protos/{name}"),
            std::fs::read(dir.join(name)).unwrap(),
        );
    }
    s
}

fn collection() -> FileSet {
    FileSet::from_dir(&fixtures().join("schemas-collection")).unwrap()
}

fn orders_changes(from: u8, to: u8) -> SpecChanges {
    schema_changes(
        SchemaKind::Protobuf,
        Some(&orders(from)),
        Some(&orders(to)),
        &collection(),
    )
    .unwrap()
}

#[test]
fn orders_v1_to_v2_breaks_nothing() {
    let c = orders_changes(1, 2);
    assert!(
        c.breaking().next().is_none(),
        "{:#?}",
        c.breaking().collect::<Vec<_>>()
    );
    assert_eq!(
        one(&c, "/shop.orders.Orders/Refund").kind,
        SpecChangeKind::OperationAdded
    );
    assert_eq!(
        one(&c, "shop.orders.ListOrdersRequest.page_token").kind,
        SpecChangeKind::FieldAdded
    );
    assert_eq!(
        one(&c, "shop.orders.Order.extra_tags").kind,
        SpecChangeKind::FieldAdded
    );
    assert_eq!(
        one(&c, "shop.orders.Order.priority").kind,
        SpecChangeKind::TypeChanged
    );
    assert!(
        at(&c, "shop.orders.Order.description")
            .iter()
            .all(|x| !x.breaking)
    );
    assert!(at(&c, "shop.orders.Order.Status.REFUNDED").len() == 1);
    assert!(at(&c, "shop.orders.Audit").len() == 1);
}

#[test]
fn orders_v1_to_v3_breaks_in_every_way_the_rules_name() {
    let c = orders_changes(1, 3);
    let mut got = breaking_ops(&c);
    got.sort_unstable();
    let mut want = vec![
        "/shop.orders.Orders/ListOrders",
        "/shop.orders.Orders/Watch",
        "shop.orders.Admin",
        "shop.orders.ListOrdersRequest.customer",
        "shop.orders.ListOrdersRequest.offset",
        "shop.orders.Order.Status.PAID",
        "shop.orders.Order.id",
        "shop.orders.Order.legacy",
        "shop.orders.Order.phone",
        "shop.orders.Order.tags",
        "shop.orders.Order.total",
        "shop.orders.PurgeRequest",
        "shop.orders.PurgeResult",
    ];
    want.sort_unstable();
    assert_eq!(got, want, "{:#?}", c.changes);
    assert_eq!(
        one(&c, "shop.orders.Order.id").kind,
        SpecChangeKind::NumberChanged
    );
    assert_eq!(
        one(&c, "/shop.orders.Orders/ListOrders").kind,
        SpecChangeKind::StreamingChanged
    );
    assert_eq!(
        one(&c, "/shop.orders.Orders/Watch").kind,
        SpecChangeKind::OperationRemoved
    );
    assert_eq!(c.from_version, None);
}

#[test]
fn requests_that_call_a_changed_method_or_type_are_named() {
    let c = orders_changes(1, 3);
    let reqs = |op: &str| -> Vec<String> {
        let mut r: Vec<String> = at(&c, op).iter().flat_map(|x| x.requests.clone()).collect();
        r.sort();
        r
    };
    assert_eq!(
        reqs("/shop.orders.Orders/ListOrders"),
        ["grpc/list-orders.yml"]
    );
    assert_eq!(reqs("/shop.orders.Orders/Watch"), ["grpc/watch.yml"]);
    assert_eq!(reqs("shop.orders.Admin"), ["grpc/purge.yml"]);
    assert_eq!(reqs("shop.orders.PurgeRequest"), ["grpc/purge.yml"]);
    assert_eq!(
        reqs("shop.orders.ListOrdersRequest.customer"),
        ["grpc/list-orders.yml"]
    );
    // Order is the answer of GetOrder, ListOrders and Watch (and Cancel, which has no request)
    assert_eq!(
        reqs("shop.orders.Order.legacy"),
        [
            "grpc/get-order.yml",
            "grpc/list-orders.yml",
            "grpc/watch.yml"
        ]
    );
    // an enum nested in Order is reached through Order
    assert!(reqs("shop.orders.Order.Status.PAID").contains(&"grpc/get-order.yml".to_owned()));
    // nothing calls Upload
    assert!(!reqs("shop.orders.Order.legacy").contains(&"grpc/upload.yml".to_owned()));
}

#[test]
fn imports_are_found_by_the_directory_of_the_importing_file_and_the_roots_they_imply() {
    let mut s = SchemaSources::new();
    s.insert(
        "include/shop/money.proto".into(),
        b"syntax = \"proto3\";\npackage m;\nmessage Money { int64 units = 1; }\n".to_vec(),
    );
    s.insert("api/orders.proto".into(), b"syntax = \"proto3\";\npackage o;\nimport \"shop/money.proto\";\nmessage Order { m.Money total = 1; }\n".to_vec());
    let c = schema_changes(
        SchemaKind::Protobuf,
        Some(&s),
        Some(&s),
        &empty_collection(),
    )
    .unwrap();
    assert!(c.changes.is_empty());
}

// ---- hostile input --------------------------------------------------------------------

fn error(proto: &[u8]) -> String {
    let mut s = SchemaSources::new();
    s.insert("a.proto".into(), proto.to_vec());
    match schema_changes(
        SchemaKind::Protobuf,
        Some(&s),
        Some(&s),
        &empty_collection(),
    ) {
        Err(ContractError::Spec(m)) => m,
        other => panic!("expected a spec error, got {other:?}"),
    }
}

#[test]
fn a_proto_that_does_not_compile_says_where() {
    let m = error(b"syntax = \"proto3\";\nmessage M {\n  string a = ;\n}\n");
    assert!(m.contains("a.proto"), "{m}");
    assert!(error(b"syntax = \"proto3\";\nimport \"missing.proto\";\n").contains("missing.proto"));
    assert!(error(&[0xff, 0xfe]).contains("a.proto"));
}

#[test]
fn pathological_protos_are_refused_quickly() {
    let started = web_time::Instant::now();
    let nested = format!(
        "syntax = \"proto3\";\n{}{}",
        "message M {".repeat(5000),
        "}".repeat(5000)
    );
    let mut n = SchemaSources::new();
    n.insert("a.proto".into(), nested.into_bytes());
    let _ = schema_changes(
        SchemaKind::Protobuf,
        Some(&n),
        Some(&n),
        &empty_collection(),
    );
    assert!(error(&vec![b' '; 17 * 1024 * 1024]).contains("large"));
    assert!(started.elapsed().as_secs() < 10);
}
