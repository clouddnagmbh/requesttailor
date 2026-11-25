// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Breaking changes of a GraphQL schema (wave 18, strand H): every rule of the contract, breaking
//! and not breaking, the synthetic shop schema in `testdata/contract/graphql`, the requests a
//! change affects, and hostile input.

use std::path::PathBuf;

use rt_format::FileSet;
use rt_format::contract::{
    ContractError, SchemaKind, SchemaSources, SpecChange, SpecChangeKind, SpecChanges,
    schema_changes,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn sources(sdl: &str) -> SchemaSources {
    let mut s = SchemaSources::new();
    s.insert("schema.graphql".into(), sdl.as_bytes().to_vec());
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
        SchemaKind::Graphql,
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

/// The one change at `op`; fails if there are none or several.
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

const BASE: &str = "type Query { user(id: ID!, limit: Int): User  users: [User!]! }\n\
    type User implements Node { id: ID! name: String! age: Int tags: [String!] }\n\
    interface Node { id: ID! }\n\
    enum Role { ADMIN USER GUEST }\n\
    input Filter { name: String role: Role }\n\
    union Hit = User | Group\n\
    type Group { id: ID! }\n\
    directive @cache(ttl: Int) on FIELD_DEFINITION\n\
    directive @log on FIELD_DEFINITION\n";

// ---- breaking rules -------------------------------------------------------------------

#[test]
fn a_removed_type_is_breaking() {
    let c = diff(
        BASE,
        &BASE
            .replace("type Group { id: ID! }\n", "")
            .replace(" | Group", ""),
    );
    let t = one(&c, "Group");
    assert_eq!(t.kind, SpecChangeKind::TypeRemoved);
    assert!(t.breaking);
}

#[test]
fn a_removed_field_is_breaking() {
    let c = diff(BASE, &BASE.replace(" age: Int", ""));
    let f = one(&c, "User.age");
    assert_eq!(f.kind, SpecChangeKind::FieldRemoved);
    assert!(f.breaking);
}

#[test]
fn a_removed_argument_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("user(id: ID!, limit: Int)", "user(id: ID!)"),
    );
    let a = one(&c, "Query.user(limit:)");
    assert_eq!(a.kind, SpecChangeKind::ParameterRemoved);
    assert!(a.breaking);
}

#[test]
fn a_removed_enum_value_is_breaking() {
    let c = diff(BASE, &BASE.replace("ADMIN USER GUEST", "ADMIN USER"));
    let v = one(&c, "Role.GUEST");
    assert_eq!(v.kind, SpecChangeKind::EnumValueRemoved);
    assert!(v.breaking);
}

#[test]
fn a_changed_field_type_is_breaking() {
    let c = diff(BASE, &BASE.replace("age: Int", "age: String"));
    let f = one(&c, "User.age");
    assert_eq!(f.kind, SpecChangeKind::TypeChanged);
    assert!(f.breaking);
    assert!(
        f.message.contains("Int") && f.message.contains("String"),
        "{}",
        f.message
    );
}

#[test]
fn a_changed_list_element_type_is_breaking() {
    let c = diff(BASE, &BASE.replace("users: [User!]!", "users: [Group!]!"));
    assert_eq!(one(&c, "Query.users").kind, SpecChangeKind::TypeChanged);
    assert!(one(&c, "Query.users").breaking);
}

#[test]
fn a_list_becoming_a_single_value_is_breaking() {
    let c = diff(BASE, &BASE.replace("users: [User!]!", "users: User!"));
    assert!(one(&c, "Query.users").breaking);
}

#[test]
fn a_changed_argument_type_is_breaking() {
    let c = diff(BASE, &BASE.replace("user(id: ID!,", "user(id: String!,"));
    let a = one(&c, "Query.user(id:)");
    assert_eq!(a.kind, SpecChangeKind::TypeChanged);
    assert!(a.breaking);
}

#[test]
fn an_output_field_turning_nullable_is_breaking() {
    let c = diff(BASE, &BASE.replace("name: String!", "name: String"));
    let f = one(&c, "User.name");
    assert_eq!(f.kind, SpecChangeKind::NullabilityChanged);
    assert!(f.breaking);
}

#[test]
fn an_inner_non_null_lost_in_a_list_is_breaking() {
    let c = diff(BASE, &BASE.replace("users: [User!]!", "users: [User]!"));
    assert_eq!(
        one(&c, "Query.users").kind,
        SpecChangeKind::NullabilityChanged
    );
    assert!(one(&c, "Query.users").breaking);
}

#[test]
fn an_input_argument_turning_non_null_is_breaking() {
    let c = diff(BASE, &BASE.replace("limit: Int)", "limit: Int!)"));
    let a = one(&c, "Query.user(limit:)");
    assert_eq!(a.kind, SpecChangeKind::NullabilityChanged);
    assert!(a.breaking);
}

#[test]
fn an_input_field_turning_non_null_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("name: String role", "name: String! role"),
    );
    let f = one(&c, "Filter.name");
    assert_eq!(f.kind, SpecChangeKind::NullabilityChanged);
    assert!(f.breaking);
}

#[test]
fn a_new_required_argument_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("limit: Int)", "limit: Int, region: String!)"),
    );
    let a = one(&c, "Query.user(region:)");
    assert_eq!(a.kind, SpecChangeKind::RequiredAdded);
    assert!(a.breaking);
}

#[test]
fn a_new_required_input_field_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("role: Role }", "role: Role currency: String! }"),
    );
    let f = one(&c, "Filter.currency");
    assert_eq!(f.kind, SpecChangeKind::RequiredAdded);
    assert!(f.breaking);
}

#[test]
fn a_removed_input_field_is_breaking() {
    let c = diff(BASE, &BASE.replace(" role: Role }", " }"));
    let f = one(&c, "Filter.role");
    assert_eq!(f.kind, SpecChangeKind::FieldRemoved);
    assert!(f.breaking);
}

#[test]
fn a_removed_union_member_is_breaking() {
    let c = diff(BASE, &BASE.replace("User | Group", "User"));
    let m = one(&c, "Hit");
    assert!(m.breaking);
    assert!(m.message.contains("Group"), "{}", m.message);
}

#[test]
fn a_removed_interface_is_breaking() {
    let c = diff(BASE, &BASE.replace("implements Node ", ""));
    let m = one(&c, "User");
    assert!(m.breaking);
    assert!(m.message.contains("Node"), "{}", m.message);
}

#[test]
fn a_removed_directive_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("directive @log on FIELD_DEFINITION\n", ""),
    );
    let d = one(&c, "@log");
    assert_eq!(d.kind, SpecChangeKind::TypeRemoved);
    assert!(d.breaking);
}

#[test]
fn a_changed_type_kind_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("type Group { id: ID! }", "interface Group { id: ID! }"),
    );
    let t = one(&c, "Group");
    assert_eq!(t.kind, SpecChangeKind::TypeChanged);
    assert!(t.breaking);
}

#[test]
fn a_new_required_argument_of_a_directive_is_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("@cache(ttl: Int)", "@cache(ttl: Int, scope: String!)"),
    );
    assert!(one(&c, "@cache(scope:)").breaking);
}

// ---- rules that do not break ----------------------------------------------------------

#[test]
fn an_output_field_turning_non_null_is_not_breaking() {
    let c = diff(BASE, &BASE.replace("age: Int", "age: Int!"));
    let f = one(&c, "User.age");
    assert_eq!(f.kind, SpecChangeKind::NullabilityChanged);
    assert!(!f.breaking);
}

#[test]
fn an_input_argument_turning_nullable_is_not_breaking() {
    let c = diff(BASE, &BASE.replace("user(id: ID!,", "user(id: ID,"));
    let a = one(&c, "Query.user(id:)");
    assert_eq!(a.kind, SpecChangeKind::NullabilityChanged);
    assert!(!a.breaking);
}

#[test]
fn additions_are_not_breaking() {
    let new = BASE
        .replace("age: Int", "age: Int email: String")
        .replace("ADMIN USER GUEST", "ADMIN USER GUEST OWNER")
        .replace("limit: Int)", "limit: Int, sort: String)")
        .replace("role: Role }", "role: Role since: String }")
        .replace("User | Group", "User | Group | Query")
        + "type Extra { id: ID! }\ndirective @trace on FIELD_DEFINITION\n";
    let c = diff(BASE, &new);
    assert!(c.breaking().next().is_none(), "{:#?}", c.changes);
    assert_eq!(one(&c, "User.email").kind, SpecChangeKind::FieldAdded);
    assert_eq!(
        one(&c, "Query.user(sort:)").kind,
        SpecChangeKind::ParameterAdded
    );
    assert_eq!(one(&c, "Filter.since").kind, SpecChangeKind::FieldAdded);
    assert!(!one(&c, "Role.OWNER").breaking);
    assert!(!one(&c, "Extra").breaking);
    assert!(!one(&c, "@trace").breaking);
    assert!(!one(&c, "Hit").breaking);
}

#[test]
fn a_new_argument_with_a_default_is_not_breaking() {
    let c = diff(
        BASE,
        &BASE.replace("limit: Int)", "limit: Int, region: String! = \"eu\")"),
    );
    assert!(!one(&c, "Query.user(region:)").breaking);
}

#[test]
fn descriptions_and_comments_are_not_breaking() {
    let c = diff(
        BASE,
        &(BASE
            .replace("type Group", "\"A group.\"\ntype Group")
            .replace("name: String! age", "\"The name.\" name: String! age")
            + "# a comment\n"),
    );
    assert!(c.breaking().next().is_none());
    assert!(!c.changes.is_empty(), "a changed description is listed");
    assert!(c.changes.iter().all(|x| x.kind == SpecChangeKind::Other));
}

#[test]
fn commas_and_layout_are_no_change() {
    let c = diff(
        BASE,
        &BASE
            .replace(' ', "  ")
            .replace("{ ", "{\n  ")
            .replace("}\n", ",\n}\n"),
    );
    assert!(c.changes.is_empty(), "{:#?}", c.changes);
}

#[test]
fn deprecating_a_field_is_a_hint() {
    let c = diff(
        BASE,
        &BASE.replace("age: Int", "age: Int @deprecated(reason: \"no\")"),
    );
    let f = one(&c, "User.age");
    assert!(!f.breaking);
    assert!(f.message.contains("deprecated"), "{}", f.message);
    let e = diff(BASE, &BASE.replace("GUEST", "GUEST @deprecated"));
    assert!(!one(&e, "Role.GUEST").breaking);
}

#[test]
fn the_order_of_definitions_is_no_change() {
    let at_directives = BASE.find("directive").unwrap();
    let reordered = format!("{}{}", &BASE[at_directives..], &BASE[..at_directives]);
    assert!(diff(BASE, &reordered).changes.is_empty());
}

#[test]
fn extensions_are_merged_into_their_type() {
    let split = "type Query { a: Int }\nextend type Query { b: Int }\n";
    let whole = "type Query { a: Int b: Int }\n";
    assert!(diff(whole, split).changes.is_empty());
    let c = diff(split, "type Query { a: Int }\n");
    assert!(one(&c, "Query.b").breaking);
}

#[test]
fn several_files_make_one_schema() {
    let mut old = SchemaSources::new();
    old.insert("a.graphql".into(), b"type Query { a: Int }".to_vec());
    old.insert("b.graphql".into(), b"type Other { id: ID }".to_vec());
    let mut new = SchemaSources::new();
    new.insert(
        "a.graphql".into(),
        b"type Query { a: Int }\ntype Other { id: ID }".to_vec(),
    );
    let c = schema_changes(
        SchemaKind::Graphql,
        Some(&old),
        Some(&new),
        &empty_collection(),
    )
    .unwrap();
    assert!(c.changes.is_empty(), "{:#?}", c.changes);
}

#[test]
fn block_strings_and_default_values_parse() {
    let sdl = "\"\"\"\nA \\\"\"\" doc\n\"\"\"\ntype Query { f(a: [Int!] = [1, 2], b: Filter = {name: \"x\"}, c: Role = ADMIN): Int }\n\
        input Filter { name: String }\nenum Role { ADMIN }\n";
    assert!(diff(sdl, sdl).changes.is_empty());
}

#[test]
fn a_missing_side_counts_as_an_empty_schema() {
    let added = schema_changes(
        SchemaKind::Graphql,
        None,
        Some(&sources(BASE)),
        &empty_collection(),
    )
    .unwrap();
    assert!(!added.changes.is_empty());
    assert!(added.breaking().next().is_none());
    let removed = schema_changes(
        SchemaKind::Graphql,
        Some(&sources(BASE)),
        None,
        &empty_collection(),
    )
    .unwrap();
    assert!(removed.breaking().next().is_some());
    let none = schema_changes(SchemaKind::Graphql, None, None, &empty_collection()).unwrap();
    assert!(none.changes.is_empty());
}

// ---- the synthetic shop schema --------------------------------------------------------

fn shop(version: u8) -> SchemaSources {
    let name = format!("shop-v{version}.graphql");
    let bytes = std::fs::read(fixtures().join("graphql").join(name)).unwrap();
    let mut s = SchemaSources::new();
    s.insert("api/shop.graphql".into(), bytes);
    s
}

fn collection() -> FileSet {
    FileSet::from_dir(&fixtures().join("schemas-collection")).unwrap()
}

fn shop_changes(from: u8, to: u8) -> SpecChanges {
    schema_changes(
        SchemaKind::Graphql,
        Some(&shop(from)),
        Some(&shop(to)),
        &collection(),
    )
    .unwrap()
}

#[test]
fn shop_v1_to_v2_breaks_nothing() {
    let c = shop_changes(1, 2);
    assert!(
        c.breaking().next().is_none(),
        "{:#?}",
        c.breaking().collect::<Vec<_>>()
    );
    assert!(at(&c, "Product.sku").len() == 1);
    assert!(at(&c, "Order.createdAt").len() == 1);
    assert!(at(&c, "Query.products(sort:)").len() == 1);
    assert!(at(&c, "OrderStatus.REFUNDED").len() == 1);
    assert!(at(&c, "Review").len() == 1);
    assert!(
        at(&c, "Order.total").iter().all(|x| !x.breaking),
        "nullable to non-null is fine"
    );
    assert!(
        at(&c, "Query.legacyStats")
            .iter()
            .any(|x| x.message.contains("deprecated"))
    );
    assert!(at(&c, "Query.search(text:)").iter().all(|x| !x.breaking));
}

#[test]
fn shop_v1_to_v3_breaks_in_every_way_the_rules_name() {
    let c = shop_changes(1, 3);
    let mut got = breaking_ops(&c);
    got.sort_unstable();
    let mut want = vec![
        "@auth",
        "Category",
        "Mutation.cancelOrder(reason:)",
        "NewOrder.coupon",
        "NewOrder.currency",
        "OrderStatus.CANCELLED",
        "Product",
        "Product.category",
        "Product.legacyCode",
        "Product.price",
        "Product.tags",
        "Query.legacyStats",
        "Query.order(id:)",
        "Query.products(category:)",
        "Query.products(region:)",
        "SearchResult",
    ];
    want.sort_unstable();
    assert_eq!(got, want, "{:#?}", c.changes);
    assert!(c.changes.iter().all(|x| !x.message.is_empty()));
    assert_eq!(c.from_version, None);
    assert_eq!(c.to_version, None);
}

#[test]
fn requests_that_use_a_changed_part_are_named() {
    let c = shop_changes(1, 3);
    let reqs = |op: &str| -> Vec<String> {
        let mut r: Vec<String> = at(&c, op).iter().flat_map(|x| x.requests.clone()).collect();
        r.sort();
        r
    };
    assert_eq!(reqs("Product.legacyCode"), ["graphql/products.yml"]);
    assert_eq!(reqs("Product.price"), ["graphql/products.yml"]);
    assert_eq!(reqs("Query.legacyStats"), ["graphql/stats.yml"]);
    assert_eq!(reqs("Query.order(id:)"), ["graphql/order.yml"]);
    assert_eq!(
        reqs("Query.products(category:)"),
        Vec::<String>::new(),
        "no request sets it"
    );
    assert_eq!(
        reqs("Query.products(region:)"),
        ["graphql/products.yml"],
        "every request of the field"
    );
    assert_eq!(reqs("NewOrder.currency"), ["graphql/place-order.yml"]);
    assert_eq!(reqs("Category"), ["graphql/search.yml"]);
    assert_eq!(reqs("SearchResult"), ["graphql/search.yml"]);
    // a field nobody selects names no request
    assert_eq!(reqs("Product.tags"), Vec::<String>::new());
    // a changed type names the requests that select anything of it
    assert!(reqs("Product").contains(&"graphql/products.yml".to_owned()));
}

#[test]
fn a_query_that_does_not_parse_matches_by_name() {
    let mut files = collection();
    files.files.insert(
        "graphql/broken.yml".into(),
        "info:\n  name: Broken\n  type: graphql\n  seq: 9\n\ngraphql:\n  method: POST\n  url: \"{{baseUrl}}/graphql\"\n  body:\n    query: |-\n      query { products { legacyCode \n    variables: |-\n      {}\n  auth: inherit\n"
            .as_bytes()
            .to_vec(),
    );
    let c = schema_changes(SchemaKind::Graphql, Some(&shop(1)), Some(&shop(3)), &files).unwrap();
    let reqs: Vec<&String> = at(&c, "Product.legacyCode")
        .iter()
        .flat_map(|x| &x.requests)
        .collect();
    assert!(
        reqs.iter().any(|r| r.as_str() == "graphql/broken.yml"),
        "{reqs:?}"
    );
}

// ---- hostile input --------------------------------------------------------------------

fn error(sdl: &[u8]) -> String {
    let mut s = SchemaSources::new();
    s.insert("schema.graphql".into(), sdl.to_vec());
    match schema_changes(SchemaKind::Graphql, Some(&s), Some(&s), &empty_collection()) {
        Err(ContractError::Spec(m)) => m,
        other => panic!("expected a spec error, got {other:?}"),
    }
}

#[test]
fn a_schema_that_does_not_parse_says_where() {
    let m = error(b"type Query {\n  a: Int\n  b\n}\n");
    assert!(m.contains("schema.graphql") && m.contains("line 4"), "{m}");
    assert!(error(b"type { a: Int }").contains("line 1"));
    assert!(error(b"type Query { a: \"x\" }").contains("schema.graphql"));
    assert!(error(b"type Query { a: Int } }").contains("schema.graphql"));
    assert!(error(b"\"\"\"never closed").contains("schema.graphql"));
    assert!(error(&[0xff, 0xfe, 0x00]).contains("schema.graphql"));
}

#[test]
fn pathological_schemas_are_refused_quickly() {
    let started = web_time::Instant::now();
    let deep = format!(
        "type Query {{ a: {}Int{} }}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let m = error(deep.as_bytes());
    assert!(m.contains("deep") || m.contains("nest"), "{m}");
    let values = format!(
        "type Query {{ a(b: Int = {}1{}): Int }}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    error(values.as_bytes());
    let huge = vec![b'#'; 17 * 1024 * 1024];
    assert!(error(&huge).contains("large"));
    assert!(started.elapsed().as_secs() < 5);
}
