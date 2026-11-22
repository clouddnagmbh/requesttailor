// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The contract check (wave 12, strand F): the spec a collection names, requests against the
//! spec, breaking changes between spec versions and the changelog. Fixtures in
//! `testdata/contract` (three versions of a synthetic Orders API and a collection).

use std::path::PathBuf;

use rt_format::FileSet;
use rt_format::contract::{
    ContractError, ContractReport, FindingKind, SPEC_CANDIDATES, SpecChange, SpecChangeKind,
    SpecChanges, changelog, changes, check, configured_spec, spec_info,
};
use rt_format::diff::CollectionDiff;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/contract")
}

fn spec(name: &str) -> Vec<u8> {
    std::fs::read(root().join("specs").join(name)).unwrap()
}

fn collection() -> FileSet {
    FileSet::from_dir(&root().join("collection")).unwrap()
}

fn set_file(files: &mut FileSet, path: &str, text: &str) {
    files
        .files
        .insert(path.to_owned(), text.as_bytes().to_vec());
}

fn request(method: &str, url: &str, extra: &str) -> String {
    format!(
        "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n{extra}"
    )
}

fn findings(report: &ContractReport, kind: FindingKind) -> Vec<&str> {
    report
        .findings
        .iter()
        .filter(|f| f.kind == kind)
        .map(|f| f.request.as_str())
        .collect()
}

fn diff_of(old: &str, new: &str) -> SpecChanges {
    changes(Some(&spec(old)), Some(&spec(new)), &collection()).unwrap()
}

fn find(c: &SpecChanges, kind: SpecChangeKind) -> Vec<&SpecChange> {
    c.changes.iter().filter(|x| x.kind == kind).collect()
}

// ---- Where the spec is ----------------------------------------------------------------

#[test]
fn configured_spec_comes_from_the_extension() {
    assert_eq!(
        configured_spec(&collection()).as_deref(),
        Some("/specs/v1.yaml")
    );
}

#[test]
fn without_the_extension_nothing_is_configured() {
    let mut files = collection();
    set_file(
        &mut files,
        "opencollection.yml",
        "opencollection: 1.0.0\ninfo:\n  name: Orders\n",
    );
    assert_eq!(configured_spec(&files), None);
    assert_eq!(SPEC_CANDIDATES[0], "openapi.yaml");
}

#[test]
fn spec_info_reads_yaml_and_json() {
    assert_eq!(
        spec_info(&spec("v1.yaml")).unwrap(),
        ("Orders API".into(), "1.0.0".into())
    );
    assert_eq!(
        spec_info(&spec("v3.yaml")).unwrap(),
        ("Orders API".into(), "2.0.0".into())
    );
    assert_eq!(
        spec_info(&spec("mini.json")).unwrap(),
        ("Mini".into(), "0.1.0".into())
    );
    assert!(spec_info(b"not: [openapi").is_err());
    assert!(spec_info(b"hello: world").is_err());
}

// ---- check ------------------------------------------------------------------------------

#[test]
fn a_matching_collection_has_no_findings_and_covers_everything() {
    let report = check(&collection(), &spec("v1.yaml")).unwrap();
    assert_eq!(report.findings, vec![], "{report:#?}");
    assert_eq!(report.operations, 5);
    assert_eq!(report.covered, 5);
    assert!(report.uncovered.is_empty());
}

#[test]
fn a_request_without_an_operation_is_found() {
    let mut files = collection();
    set_file(
        &mut files,
        "customers.yml",
        &request("GET", "{{baseUrl}}/customers", ""),
    );
    // The path exists, the method does not.
    set_file(
        &mut files,
        "orders/put.yml",
        &request("PUT", "{{baseUrl}}/orders", ""),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    let mut found = findings(&report, FindingKind::NoOperation);
    found.sort();
    assert_eq!(found, vec!["customers.yml", "orders/put.yml"]);
    assert!(report.findings.iter().all(|f| f.operation.is_none()));
    let put = report
        .findings
        .iter()
        .find(|f| f.request == "orders/put.yml")
        .unwrap();
    assert!(
        put.message.contains("GET") && put.message.contains("POST"),
        "{}",
        put.message
    );
}

#[test]
fn an_unknown_query_parameter_is_found() {
    let mut files = collection();
    set_file(
        &mut files,
        "orders/list-orders.yml",
        &request(
            "GET",
            "{{baseUrl}}/orders?limit=10&sort=asc",
            "  headers:\n    - name: X-Tenant\n      value: acme\n    - name: Accept\n      value: application/json\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(report.findings.len(), 1, "{report:#?}");
    let f = &report.findings[0];
    assert_eq!(f.kind, FindingKind::UnknownParameter);
    assert_eq!(f.request, "orders/list-orders.yml");
    assert_eq!(f.operation.as_deref(), Some("GET /orders"));
    assert!(f.message.contains("sort"));
}

#[test]
fn a_missing_required_parameter_is_found() {
    let mut files = collection();
    set_file(
        &mut files,
        "orders/list-orders.yml",
        &request("GET", "{{baseUrl}}/orders?limit=10", ""),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(
        findings(&report, FindingKind::MissingParameter),
        vec!["orders/list-orders.yml"]
    );
    assert!(report.findings[0].message.contains("X-Tenant"));
    assert_eq!(report.findings.len(), 1);
}

#[test]
fn a_disabled_header_does_not_count() {
    let mut files = collection();
    set_file(
        &mut files,
        "orders/list-orders.yml",
        &request(
            "GET",
            "{{baseUrl}}/orders",
            "  headers:\n    - name: X-Tenant\n      value: acme\n      disabled: true\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(
        findings(&report, FindingKind::MissingParameter),
        vec!["orders/list-orders.yml"]
    );
}

#[test]
fn a_body_that_does_not_fit_the_schema_is_found() {
    let body = |data: &str| {
        request(
            "POST",
            "{{baseUrl}}/orders",
            &format!("  body:\n    type: json\n    data: |-\n      {data}\n"),
        )
    };
    let cases = [
        // customer is required and missing
        (r#"{"items": []}"#, "customer"),
        // qty must be an integer
        (
            r#"{"customer": "a", "items": [{"sku": "x", "qty": "two"}]}"#,
            "qty",
        ),
        // customer must be a string
        (r#"{"customer": 7, "items": []}"#, "customer"),
    ];
    for (data, needle) in cases {
        let mut files = collection();
        set_file(&mut files, "orders/create-order.yml", &body(data));
        let report = check(&files, &spec("v1.yaml")).unwrap();
        assert_eq!(
            findings(&report, FindingKind::BodyMismatch),
            vec!["orders/create-order.yml"],
            "{data}: {report:#?}"
        );
        assert!(
            report.findings[0].message.contains(needle),
            "{data}: {}",
            report.findings[0].message
        );
    }
    // No body where one is required, and a body where none is expected.
    let mut files = collection();
    set_file(
        &mut files,
        "orders/create-order.yml",
        &request("POST", "{{baseUrl}}/orders", ""),
    );
    set_file(
        &mut files,
        "health.yml",
        &request(
            "GET",
            "{{baseUrl}}/health",
            "  body:\n    type: json\n    data: \"{}\"\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    let mut found = findings(&report, FindingKind::BodyMismatch);
    found.sort();
    assert_eq!(found, vec!["health.yml", "orders/create-order.yml"]);
    // A media type the operation does not accept.
    let mut files = collection();
    set_file(
        &mut files,
        "orders/create-order.yml",
        &request(
            "POST",
            "{{baseUrl}}/orders",
            "  body:\n    type: text\n    data: hi\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(
        findings(&report, FindingKind::BodyMismatch),
        vec!["orders/create-order.yml"]
    );
}

#[test]
fn templates_in_a_body_are_not_judged() {
    let mut files = collection();
    set_file(
        &mut files,
        "orders/create-order.yml",
        &request(
            "POST",
            "{{baseUrl}}/orders",
            "  body:\n    type: json\n    data: |-\n      {\"customer\": \"{{customer}}\", \"items\": [{\"sku\": \"x\", \"qty\": {{qty}}}]}\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(report.findings, vec![], "{report:#?}");
    // A template standing for a whole value fits any type.
    set_file(
        &mut files,
        "orders/create-order.yml",
        &request(
            "POST",
            "{{baseUrl}}/orders",
            "  body:\n    type: json\n    data: |-\n      {\"customer\": \"a\", \"items\": [{\"sku\": \"x\", \"qty\": \"{{qty}}\"}]}\n",
        ),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(report.findings, vec![], "{report:#?}");
}

#[test]
fn uncovered_operations_are_listed() {
    let mut files = collection();
    files.files.remove("orders/delete-order.yml");
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(report.uncovered, vec!["DELETE /orders/{id}"]);
    assert_eq!((report.operations, report.covered), (5, 4));
    // Against v2 the new operation is uncovered, too.
    let report = check(&collection(), &spec("v2.yaml")).unwrap();
    assert_eq!(report.uncovered, vec!["GET /orders/{id}/items"]);
}

#[test]
fn the_server_base_path_and_a_version_variable_are_stripped() {
    let mut files = collection();
    set_file(
        &mut files,
        "a.yml",
        &request("GET", "https://api.example.test/v1/health", ""),
    );
    set_file(
        &mut files,
        "b.yml",
        &request("GET", "{{baseUrl}}/{{apiVersion}}/health", ""),
    );
    set_file(
        &mut files,
        "c.yml",
        &request("GET", "{{baseUrl}}/v1/health?x=1#top", ""),
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    let unknown = findings(&report, FindingKind::UnknownParameter);
    assert_eq!(unknown, vec!["c.yml"], "{report:#?}");
    assert!(
        findings(&report, FindingKind::NoOperation).is_empty(),
        "{report:#?}"
    );
}

#[test]
fn other_protocols_are_not_checked() {
    let mut files = collection();
    set_file(
        &mut files,
        "stream.yml",
        "info:\n  name: S\n  type: websocket\n  seq: 9\n\nwebsocket:\n  url: \"{{baseUrl}}/nothing\"\n",
    );
    let report = check(&files, &spec("v1.yaml")).unwrap();
    assert_eq!(report.findings, vec![]);
}

#[test]
fn a_spec_that_is_not_openapi_is_refused() {
    let err = check(&collection(), b"hello: world").unwrap_err();
    assert!(matches!(err, ContractError::Spec(_)), "{err:?}");
}

#[test]
fn json_specs_and_refs_work() {
    let mut files = FileSet::default();
    set_file(
        &mut files,
        "opencollection.yml",
        "opencollection: 1.0.0\ninfo:\n  name: M\n",
    );
    set_file(
        &mut files,
        "ping.yml",
        &request("GET", "{{baseUrl}}/ping", ""),
    );
    let report = check(&files, &spec("mini.json")).unwrap();
    assert_eq!((report.operations, report.covered), (1, 1));
}

// ---- changes ------------------------------------------------------------------------------

#[test]
fn v1_to_v2_changes_nothing_that_breaks() {
    let c = diff_of("v1.yaml", "v2.yaml");
    assert_eq!(c.from_version.as_deref(), Some("1.0.0"));
    assert_eq!(c.to_version.as_deref(), Some("1.1.0"));
    assert_eq!(c.breaking().count(), 0, "{c:#?}");
    let added = find(&c, SpecChangeKind::OperationAdded);
    assert_eq!(added.len(), 1);
    assert_eq!(
        added[0].operation.as_deref(),
        Some("GET /orders/{id}/items")
    );
    let p = find(&c, SpecChangeKind::ParameterAdded);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].operation.as_deref(), Some("GET /orders"));
    assert!(p[0].location.contains("offset"), "{}", p[0].location);
    // `currency` on the order (response) and `coupon` on the new order (request).
    let fields: Vec<&str> = find(&c, SpecChangeKind::FieldAdded)
        .iter()
        .map(|x| x.location.as_str())
        .collect();
    assert!(fields.iter().any(|l| l.contains("currency")), "{fields:?}");
    assert!(fields.iter().any(|l| l.contains("coupon")), "{fields:?}");
    assert_eq!(find(&c, SpecChangeKind::ResponseChanged).len(), 1);
    let other = find(&c, SpecChangeKind::Other);
    assert_eq!(other.len(), 1, "{other:#?}");
    assert_eq!(other[0].operation.as_deref(), Some("GET /orders/{id}"));
}

#[test]
fn v2_to_v3_breaks_in_every_way() {
    let c = diff_of("v2.yaml", "v3.yaml");
    assert_eq!(c.to_version.as_deref(), Some("2.0.0"));
    let b = |kind| -> Vec<&SpecChange> {
        c.changes
            .iter()
            .filter(|x| x.kind == kind && x.breaking)
            .collect()
    };
    let removed = b(SpecChangeKind::OperationRemoved);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].operation.as_deref(), Some("DELETE /orders/{id}"));
    assert_eq!(removed[0].requests, vec!["orders/delete-order.yml"]);

    let required = b(SpecChangeKind::RequiredAdded);
    assert_eq!(required.len(), 2, "{required:#?}");
    assert!(
        required
            .iter()
            .any(|x| x.operation.as_deref() == Some("GET /orders") && x.location.contains("region"))
    );
    assert!(
        required
            .iter()
            .any(|x| x.operation.as_deref() == Some("POST /orders")
                && x.location.contains("currency"))
    );
    let list = required
        .iter()
        .find(|x| x.location.contains("region"))
        .unwrap();
    assert_eq!(list.requests, vec!["orders/list-orders.yml"]);

    let param = b(SpecChangeKind::ParameterRemoved);
    assert_eq!(param.len(), 1);
    assert!(param[0].location.contains("status"));

    let ty = b(SpecChangeKind::TypeChanged);
    assert_eq!(ty.len(), 1, "{ty:#?}");
    assert!(ty[0].location.contains("limit"));
    assert!(ty[0].message.contains("integer") && ty[0].message.contains("string"));
    // The message names the parameter (the changelog shows only the message).
    assert!(ty[0].message.contains("`limit`"), "{}", ty[0].message);

    let field = b(SpecChangeKind::FieldRemoved);
    assert!(
        field.iter().any(|x| x.location.contains("total")),
        "{field:#?}"
    );
    // The order is returned by several operations; each is listed with its requests.
    assert!(field.iter().any(|x| x.location.contains("total")
        && x.operation.as_deref() == Some("GET /orders/{id}")
        && x.requests == vec!["orders/get-order.yml"]));

    let resp = b(SpecChangeKind::ResponseChanged);
    assert_eq!(resp.len(), 1, "{resp:#?}");
    assert_eq!(resp[0].operation.as_deref(), Some("GET /health"));
    assert_eq!(resp[0].requests, vec!["health.yml"]);

    // The added 204, the cancelled status, the reworded summary: not breaking.
    assert!(
        c.changes
            .iter()
            .any(|x| x.kind == SpecChangeKind::ResponseChanged && !x.breaking)
    );
    assert!(
        c.changes
            .iter()
            .any(|x| x.kind == SpecChangeKind::Other && !x.breaking)
    );
    assert_eq!(
        c.changes.iter().filter(|x| x.breaking).count(),
        removed.len() + required.len() + param.len() + ty.len() + field.len() + resp.len()
    );
}

#[test]
fn nullable_in_30_and_a_null_type_in_31_are_the_same() {
    let c = diff_of("v1.yaml", "v3.yaml");
    assert!(
        !c.changes
            .iter()
            .any(|x| x.location.contains("note") && x.kind == SpecChangeKind::TypeChanged),
        "{c:#?}"
    );
}

#[test]
fn every_kind_shows_up_once_over_the_versions() {
    let mut seen = std::collections::BTreeSet::new();
    for (a, b) in [("v1.yaml", "v2.yaml"), ("v2.yaml", "v3.yaml")] {
        for x in diff_of(a, b).changes {
            seen.insert(format!("{:?}", x.kind));
        }
    }
    for kind in [
        "OperationAdded",
        "OperationRemoved",
        "ParameterAdded",
        "ParameterRemoved",
        "RequiredAdded",
        "FieldRemoved",
        "FieldAdded",
        "TypeChanged",
        "ResponseChanged",
        "Other",
    ] {
        assert!(seen.contains(kind), "{kind} missing in {seen:?}");
    }
}

#[test]
fn a_spec_that_appears_or_disappears_adds_or_removes_every_operation() {
    let files = collection();
    let c = changes(None, Some(&spec("v1.yaml")), &files).unwrap();
    assert_eq!(c.from_version, None);
    assert_eq!(c.changes.len(), 5);
    assert!(
        c.changes
            .iter()
            .all(|x| x.kind == SpecChangeKind::OperationAdded && !x.breaking)
    );
    let c = changes(Some(&spec("v1.yaml")), None, &files).unwrap();
    assert_eq!(c.to_version, None);
    assert!(
        c.changes
            .iter()
            .all(|x| x.kind == SpecChangeKind::OperationRemoved && x.breaking)
    );
    let c = changes(None, None, &files).unwrap();
    assert!(c.changes.is_empty());
}

#[test]
fn identical_specs_have_no_changes() {
    assert!(diff_of("v2.yaml", "v2.yaml").changes.is_empty());
}

// ---- changelog ------------------------------------------------------------------------------

#[test]
fn the_changelog_lists_breaking_changes_first() {
    let c = diff_of("v2.yaml", "v3.yaml");
    let text = changelog("Orders API 1.1.0 → 2.0.0", &c, &CollectionDiff::default());
    assert!(text.starts_with("# Orders API 1.1.0 → 2.0.0\n"), "{text}");
    let breaking = text.find("## Breaking changes").expect(&text);
    let other = text.find("## Other changes").expect(&text);
    assert!(breaking < other);
    assert!(text.contains("DELETE /orders/{id}"));
    assert!(text.contains("orders/delete-order.yml"));
}

#[test]
fn the_changelog_says_so_when_nothing_breaks() {
    let c = diff_of("v1.yaml", "v2.yaml");
    let text = changelog("x", &c, &CollectionDiff::default());
    assert!(text.contains("## Breaking changes\n\nNone."), "{text}");
}
