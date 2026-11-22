// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The query builder's text rules: system query options read and written (round trips,
//! percent-encoding, templates, V2 versus V4), the URL rewrite, and EDM literals per version.

use rt_plugin_odata::edm::Version::{V2, V4};
use rt_plugin_odata::query::Query;
use rt_plugin_odata::query::literal::{Condition, Operator, condition, filter, literal, quote};
use rt_plugin_odata::query::url::{params_from_url, with_query};
use rt_plugin_sdk::model::{Param, ParamKind};

fn round_trip(text: &str, version: rt_plugin_odata::edm::Version) {
    let q = Query::parse(text);
    assert_eq!(q.to_query_string(version), text, "{q:?}");
    assert_eq!(Query::parse(&q.to_query_string(version)), q);
}

#[test]
fn system_options_round_trip() {
    round_trip(
        "$filter=Price%20gt%2010M%20and%20Currency%20eq%20'EUR'&$select=ID,Name&$expand=ToItems&$orderby=CreatedAt%20desc,ID&$top=10&$skip=20&$inlinecount=allpages&sap-client=100",
        V2,
    );
    round_trip(
        "$filter=contains(title,'Wuthering')&$select=ID,title&$expand=author($select=name;$expand=books($top=1)),genre&$orderby=title&$top=5&$count=true",
        V4,
    );
}

#[test]
fn parse_fills_the_fields() {
    let q = Query::parse(
        "$filter=Name%20eq%20'O''Neil'+and+x&$select=a,%20b&$expand=author($select=name,dateOfBirth),books&$orderby=a%20desc,b%20asc,c&$top=3&$skip=0&$count=true&$format=json&custom=a%2Fb",
    );
    assert_eq!(q.filter.as_deref(), Some("Name eq 'O''Neil' and x"));
    assert_eq!(q.select, vec!["a", "b"]);
    assert_eq!(
        q.expand,
        vec!["author($select=name,dateOfBirth)", "books"],
        "nested options stay raw, commas inside them do not split"
    );
    assert_eq!(
        q.orderby,
        vec![("a".into(), true), ("b".into(), false), ("c".into(), false)]
    );
    assert_eq!((q.top, q.skip, q.count), (Some(3), Some(0), true));
    // Unknown parameters keep their exact text and order.
    assert_eq!(
        q.other,
        vec![
            ("$format".into(), "json".into()),
            ("custom".into(), "a%2Fb".into())
        ]
    );
}

#[test]
fn count_per_version() {
    let v2 = Query::parse("$inlinecount=allpages");
    let v4 = Query::parse("$count=true");
    assert!(v2.count && v4.count);
    assert_eq!(v2.to_query_string(V4), "$count=true");
    assert_eq!(v4.to_query_string(V2), "$inlinecount=allpages");
    // Values that are not "count" stay as written.
    let q = Query::parse("$count=false&$inlinecount=none");
    assert!(!q.count);
    assert_eq!(q.to_query_string(V4), "$count=false&$inlinecount=none");
}

#[test]
fn templates_survive() {
    let text = "$filter=Name%20eq%20'{{name}}'&$top={{top}}&$skip={{skip}}&sap-client={{client}}";
    let q = Query::parse(text);
    assert_eq!(q.filter.as_deref(), Some("Name eq '{{name}}'"));
    assert_eq!(q.top, None);
    assert_eq!(
        q.other,
        vec![
            ("$top".into(), "{{top}}".into()),
            ("$skip".into(), "{{skip}}".into()),
            ("sap-client".into(), "{{client}}".into()),
        ]
    );
    assert_eq!(q.to_query_string(V2), text);
    // A template with characters that would be encoded elsewhere stays as written.
    let q = Query::parse("$filter={{my filter&x}}");
    assert_eq!(q.filter.as_deref(), Some("{{my filter&x}}"));
    assert_eq!(q.to_query_string(V4), "$filter={{my filter&x}}");
}

#[test]
fn encoding_of_written_values() {
    let q = Query {
        filter: Some("Name eq 'Müller & Söhne' and Note eq '#1+2' and p eq '100%'".into()),
        orderby: vec![("Name".into(), true)],
        ..Query::default()
    };
    assert_eq!(
        q.to_query_string(V4),
        "$filter=Name%20eq%20'M%C3%BCller%20%26%20S%C3%B6hne'%20and%20Note%20eq%20'%231%2B2'%20and%20p%20eq%20'100%25'&$orderby=Name%20desc"
    );
    assert_eq!(Query::parse(&q.to_query_string(V4)), q);
}

#[test]
fn empty_and_odd_input() {
    assert_eq!(Query::parse(""), Query::default());
    assert_eq!(Query::parse("&&"), Query::default());
    let q = Query::parse("flag&$top=x&$filter=");
    assert_eq!(
        q.other,
        vec![
            ("flag".into(), "".into()),
            ("$top".into(), "x".into()),
            ("$filter".into(), "".into())
        ]
    );
    assert_eq!(q.to_query_string(V4), "flag&$top=x&$filter");
    // Encoded option names are recognised.
    assert_eq!(Query::parse("%24top=2").top, Some(2));
}

#[test]
fn rewrite_keeps_base_fragment_order_and_unchanged_text() {
    let url = "{{baseUrl}}/SalesOrderSet?sap-client=100&$top=5&$filter=A+eq+1#frag";
    let mut q = Query::parse("sap-client=100&$top=5&$filter=A+eq+1");
    q.top = Some(10);
    q.select = vec!["A".into()];
    assert_eq!(
        with_query(url, &q, V2),
        "{{baseUrl}}/SalesOrderSet?sap-client=100&$top=10&$filter=A+eq+1&$select=A#frag",
        "params keep their place, unchanged ones their text (+), new ones go last"
    );
    q.filter = None;
    q.top = None;
    q.select.clear();
    assert_eq!(
        with_query(url, &q, V2),
        "{{baseUrl}}/SalesOrderSet?sap-client=100#frag"
    );
    q.other.clear();
    assert_eq!(with_query(url, &q, V2), "{{baseUrl}}/SalesOrderSet#frag");
    // Count switches spelling with the version.
    let q = Query {
        count: true,
        ..Query::default()
    };
    assert_eq!(
        with_query("http://h/s/X?$count=true", &q, V2),
        "http://h/s/X?$inlinecount=allpages"
    );
    assert_eq!(
        with_query("http://h/s/X?$count=true", &q, V4),
        "http://h/s/X?$count=true"
    );
}

#[test]
fn query_rows_follow_the_url() {
    let row = |name: &str, value: &str, enabled| Param {
        name: name.into(),
        value: value.into(),
        kind: ParamKind::Query,
        enabled,
    };
    let path = Param {
        name: "id".into(),
        value: "1".into(),
        kind: ParamKind::Path,
        enabled: true,
    };
    let old = vec![row("$top", "5", true), row("off", "1", false), path.clone()];
    let rows = params_from_url("http://h/s/X?$top=10&$filter=a%20eq%201", &old);
    assert_eq!(
        rows,
        vec![
            row("$top", "10", true),
            row("off", "1", false),
            row("$filter", "a%20eq%201", true),
            path
        ]
    );
}

// -------------------------------------------------------------------------------------------
// Literals.

#[test]
fn string_literals() {
    assert_eq!(quote("O'Neil"), "'O''Neil'");
    assert_eq!(literal("Edm.String", "O'Neil", V2).unwrap(), "'O''Neil'");
    assert_eq!(literal("Edm.String", "a b", V4).unwrap(), "'a b'");
    assert_eq!(literal("Edm.String", "{{it's}}", V4).unwrap(), "'{{it's}}'");
    assert_eq!(literal("Edm.String", "null", V4).unwrap(), "null");
}

#[test]
fn guid_literals() {
    let g = "0f8fad5b-d9cb-469f-a165-70867728950e";
    assert_eq!(literal("Edm.Guid", g, V2).unwrap(), format!("guid'{g}'"));
    assert_eq!(literal("Edm.Guid", g, V4).unwrap(), g);
    assert!(literal("Edm.Guid", "nope", V4).is_err());
}

#[test]
fn date_and_time_literals() {
    assert_eq!(
        literal("Edm.DateTime", "2026-01-01", V2).unwrap(),
        "datetime'2026-01-01T00:00:00'"
    );
    assert_eq!(
        literal("Edm.DateTime", "2026-01-01T08:30", V2).unwrap(),
        "datetime'2026-01-01T08:30:00'"
    );
    assert_eq!(
        literal("Edm.DateTime", "2026-01-01T08:30:00Z", V2).unwrap(),
        "datetime'2026-01-01T08:30:00'",
        "V2 DateTime has no offset"
    );
    assert_eq!(
        literal("Edm.DateTimeOffset", "2026-01-01T08:30:00+01:00", V2).unwrap(),
        "datetimeoffset'2026-01-01T08:30:00+01:00'"
    );
    assert_eq!(
        literal("Edm.DateTimeOffset", "2026-01-01", V4).unwrap(),
        "2026-01-01T00:00:00Z"
    );
    assert_eq!(
        literal("Edm.DateTimeOffset", "2026-01-01T08:30:00.123Z", V4).unwrap(),
        "2026-01-01T08:30:00.123Z"
    );
    assert_eq!(literal("Edm.Date", "2026-01-31", V4).unwrap(), "2026-01-31");
    assert_eq!(
        literal("Edm.Date", "2026-01-31", V2).unwrap(),
        "datetime'2026-01-31T00:00:00'"
    );
    assert_eq!(
        literal("Edm.Time", "12:30", V2).unwrap(),
        "time'PT12H30M00S'"
    );
    assert_eq!(literal("Edm.Time", "PT1H", V2).unwrap(), "time'PT1H'");
    assert_eq!(literal("Edm.TimeOfDay", "12:30", V4).unwrap(), "12:30:00");
    assert!(literal("Edm.DateTime", "01.01.2026", V2).is_err());
    assert!(literal("Edm.Date", "2026-1-1", V4).is_err());
    assert!(literal("Edm.DateTimeOffset", "2026-01-01T08:30:00+1", V4).is_err());
    // Templates are not checked, but get the type's syntax.
    assert_eq!(
        literal("Edm.DateTime", "{{from}}", V2).unwrap(),
        "datetime'{{from}}'"
    );
}

#[test]
fn number_and_boolean_literals() {
    assert_eq!(literal("Edm.Decimal", "12.5", V2).unwrap(), "12.5M");
    assert_eq!(literal("Edm.Decimal", "-3", V4).unwrap(), "-3");
    assert!(literal("Edm.Decimal", "12,5", V2).is_err());
    assert_eq!(literal("Edm.Int32", "42", V2).unwrap(), "42");
    assert!(literal("Edm.Int32", "4.2", V2).is_err());
    assert_eq!(literal("Edm.Int64", "42", V2).unwrap(), "42L");
    assert_eq!(literal("Edm.Int64", "42", V4).unwrap(), "42");
    assert_eq!(literal("Edm.Double", "1.5E3", V2).unwrap(), "1.5E3d");
    assert_eq!(literal("Edm.Single", "1.5", V2).unwrap(), "1.5f");
    assert_eq!(literal("Edm.Double", "1.5", V4).unwrap(), "1.5");
    assert_eq!(literal("Edm.Boolean", "TRUE", V2).unwrap(), "true");
    assert!(literal("Edm.Boolean", "yes", V4).is_err());
    assert_eq!(
        literal("Edm.Decimal", "{{amount}}", V2).unwrap(),
        "{{amount}}M"
    );
    assert_eq!(literal("Edm.Int32", "null", V2).unwrap(), "null");
    // V4 enum types.
    assert_eq!(literal("Shop.Color", "Red", V4).unwrap(), "Shop.Color'Red'");
}

#[test]
fn conditions_per_version() {
    let c = |property: &str, edm_type: &str, operator, value: &str| Condition {
        property: property.into(),
        edm_type: edm_type.into(),
        operator,
        value: value.into(),
    };
    let name = c("CustomerName", "Edm.String", Operator::Contains, "Acme");
    assert_eq!(
        condition(&name, V2).unwrap(),
        "substringof('Acme',CustomerName)"
    );
    assert_eq!(
        condition(&name, V4).unwrap(),
        "contains(CustomerName,'Acme')"
    );
    let starts = c("title", "Edm.String", Operator::StartsWith, "The");
    assert_eq!(condition(&starts, V4).unwrap(), "startswith(title,'The')");
    let rows = vec![
        c("GrossAmount", "Edm.Decimal", Operator::Ge, "100"),
        c("CreatedAt", "Edm.DateTime", Operator::Lt, "2026-02-01"),
        c("Note", "Edm.String", Operator::Ne, "null"),
    ];
    assert_eq!(
        filter(&rows, false, V2).unwrap(),
        "GrossAmount ge 100M and CreatedAt lt datetime'2026-02-01T00:00:00' and Note ne null"
    );
    assert_eq!(filter(&rows[..1], true, V4).unwrap(), "GrossAmount ge 100");
    let bad = c("GrossAmount", "Edm.Decimal", Operator::Eq, "lots");
    let e = filter(&[bad], false, V2).unwrap_err();
    assert!(
        e.starts_with("GrossAmount: 'lots' is not a decimal number"),
        "{e}"
    );
}
