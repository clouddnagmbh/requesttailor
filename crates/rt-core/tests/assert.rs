// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Declarative assertions and variable actions (wave 7, strand C): every expression kind and
//! every operator of Bruno's set, Bruno's value rules, errors, and `set-variable` actions.
//! The corpus' assertions run in `crates/rt-format/tests/assert_corpus.rs` (they need the
//! reader); the same evaluation in the browser is covered by the conformance suite.

use rt_core::assert::{apply_actions, evaluate};
use rt_core::model::{Assertion, ScriptPhase, VarAction, VarScope};
use rt_core::script::{AssertionResult, ScriptResponse, VarChange};
use rt_core::vars::{Scope, VarLayer, VarSource};

fn response() -> ScriptResponse {
    ScriptResponse {
        status: 201,
        headers: vec![
            (
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ),
            ("Set-Cookie".into(), "a=1".into()),
            ("set-cookie".into(), "b=2".into()),
            ("X-Request-Id".into(), "req-42".into()),
        ],
        body: br#"{
            "id": 7,
            "name": "Ada Lovelace",
            "ok": true,
            "none": null,
            "ratio": 0.5,
            "zero": 0,
            "empty": "",
            "tags": ["a", "b", "c"],
            "items": [{"id": 1, "name": "first"}, {"id": 2, "name": "second"}],
            "nested": {"a b": {"c": "deep"}, "length": 99},
            "blank": {},
            "nothing": [],
            "token": "tok-123",
            "big": 12345678901234567890
        }"#
        .to_vec(),
        time_ms: 120,
        url: "http://127.0.0.1/x".into(),
    }
}

fn scope() -> Scope {
    Scope {
        layers: vec![
            VarLayer {
                source: VarSource::Environment {
                    name: "local".into(),
                },
                vars: vec![
                    ("expectedName".into(), "Ada Lovelace".into(), false),
                    ("expectedId".into(), "7".into(), false),
                    ("plainAssert".into(), "foo".into(), false),
                    ("flag".into(), "true".into(), false),
                    ("apiKey".into(), "s3cr3t".into(), true),
                ],
            },
            VarLayer {
                source: VarSource::Runtime,
                vars: vec![("chained".into(), "{{expectedId}}".into(), false)],
            },
        ],
    }
}

fn a(expression: &str, operator: &str, value: &str) -> Assertion {
    Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.into(),
        description: None,
        enabled: true,
    }
}

fn one(expression: &str, operator: &str, value: &str) -> AssertionResult {
    let mut results = evaluate(&[a(expression, operator, value)], &response(), &scope());
    assert_eq!(results.len(), 1);
    results.remove(0)
}

#[track_caller]
fn pass(expression: &str, operator: &str, value: &str) {
    let r = one(expression, operator, value);
    assert!(
        r.passed,
        "expected {expression} {operator} {value} to pass: {r:?}"
    );
    assert_eq!(r.error, None, "{r:?}");
}

#[track_caller]
fn fail(expression: &str, operator: &str, value: &str) {
    let r = one(expression, operator, value);
    assert!(
        !r.passed,
        "expected {expression} {operator} {value} to fail: {r:?}"
    );
}

#[track_caller]
fn error(expression: &str, operator: &str, value: &str, contains: &str) -> AssertionResult {
    let r = one(expression, operator, value);
    assert!(!r.passed, "{r:?}");
    let e = r.error.clone().unwrap_or_default();
    assert!(
        e.contains(contains),
        "error {e:?} should contain {contains:?}"
    );
    r
}

// ---- expressions ----

#[test]
fn status_status_text_and_response_time() {
    pass("res.status", "eq", "201");
    let r = one("res.status", "eq", "201");
    assert_eq!(r.actual.as_deref(), Some("201"));
    assert_eq!(r.expected, "201");
    pass("res.statusText", "eq", "Created");
    pass("res.responseTime", "eq", "120");
    pass("res.responseTime", "lt", "1000");
}

#[test]
fn headers_are_case_insensitive_and_joined() {
    pass("res.headers.content-type", "contains", "application/json");
    pass("res.headers.CONTENT-TYPE", "startsWith", "application/json");
    pass("res.headers[\"x-request-id\"]", "eq", "req-42");
    pass("res.headers['X-Request-Id']", "eq", "req-42");
    pass("res.headers.set-cookie", "eq", "a=1, b=2");
    pass("res.headers.x-missing", "isUndefined", "");
    pass("res.headers", "isJson", "");
    pass("res.headers.x-request-id.length", "eq", "6");
}

#[test]
fn body_paths() {
    pass("res.body", "isJson", "");
    pass("res.body.id", "eq", "7");
    pass("res.body.name", "eq", "Ada Lovelace");
    pass("res.body.items[0].id", "eq", "1");
    pass("res.body.items[1].name", "eq", "second");
    pass("res.body.items.length", "eq", "2");
    pass("res.body.tags.length", "eq", "3");
    pass("res.body.name.length", "eq", "12");
    pass("res.body[\"nested\"][\"a b\"].c", "eq", "deep");
    pass("res.body.nested['a b'].c", "eq", "deep");
    // An object's own `length` key wins.
    pass("res.body.nested.length", "eq", "99");
    pass("res.body.tags[2]", "eq", "c");
    let r = one("res.body.items[0]", "isJson", "");
    assert_eq!(r.actual.as_deref(), Some(r#"{"id":1,"name":"first"}"#));
}

#[test]
fn text_bodies_are_text() {
    let resp = ScriptResponse {
        status: 200,
        body: b"hello world".to_vec(),
        ..Default::default()
    };
    let results = evaluate(
        &[
            a("res.body", "eq", "hello world"),
            a("res.body", "isString", ""),
            a("res.body.length", "eq", "11"),
            a("res.body.id", "eq", "1"),
        ],
        &resp,
        &Scope::default(),
    );
    assert!(
        results[0].passed && results[1].passed && results[2].passed,
        "{results:?}"
    );
    assert!(!results[3].passed);
    assert!(
        results[3]
            .error
            .as_deref()
            .unwrap()
            .contains("did not resolve")
    );
}

#[test]
fn bare_names_are_variables() {
    pass("plainAssert", "eq", "foo");
    pass("expectedId", "eq", "7");
    pass("flag", "eq", "true");
    pass("flag", "isBoolean", "");
    // Placeholders inside a value resolve.
    pass("chained", "eq", "7");
    pass("noSuchVariable", "isUndefined", "");
    error("noSuchVariable", "eq", "1", "did not resolve");
}

#[test]
fn variables_in_expected_values() {
    pass("res.body.name", "eq", "{{expectedName}}");
    pass("res.body.id", "eq", "{{expectedId}}");
    let r = one("res.body.name", "eq", "{{expectedName}}");
    assert_eq!(r.expected, "Ada Lovelace");
    // An unresolved placeholder stays as written.
    let r = one("res.body.name", "eq", "{{nope}}");
    assert_eq!(r.expected, "{{nope}}");
    assert!(!r.passed);
}

#[test]
fn unknown_expressions_and_bad_paths_fail_with_an_error() {
    error("res.foo", "eq", "1", "res.foo");
    error("res.body.items[x]", "eq", "1", "not an index");
    error("res.body.items[0", "eq", "1", "missing");
    error("res.body.nope", "eq", "1", "did not resolve");
    error("res.body.nope.deeper", "eq", "1", "did not resolve");
    error("", "eq", "1", "empty");
    let r = error("res.body.nope", "eq", "1", "res.body.nope");
    assert_eq!(r.actual, None);
}

// ---- operators ----

#[test]
fn eq_and_neq_are_strict_like_bruno() {
    pass("res.status", "eq", "201");
    // A quoted value is text, the status a number.
    fail("res.status", "eq", "\"201\"");
    pass("res.body.token", "eq", "\"tok-123\"");
    pass("res.body.token", "eq", "'tok-123'");
    pass("res.body.ok", "eq", "true");
    fail("res.body.ok", "eq", "\"true\"");
    pass("res.body.none", "eq", "null");
    pass("res.body.ratio", "eq", "0.5");
    pass("res.body.ratio", "eq", "5e-1");
    pass("res.body.empty", "eq", "");
    // Objects never equal a literal.
    fail("res.body.blank", "eq", "{}");
    pass("res.status", "neq", "200");
    fail("res.status", "neq", "201");
    pass("res.headers.x-request-id", "neq", "other");
    // Numbers beyond Number.MAX_SAFE_INTEGER stay text, as in Bruno.
    fail("res.body.big", "eq", "12345678901234567890");
    // Empty operator is eq.
    pass("res.status", "", "201");
}

#[test]
fn numeric_comparisons() {
    pass("res.status", "gt", "200");
    fail("res.status", "gt", "201");
    pass("res.status", "gte", "201");
    pass("res.status", "lt", "300");
    fail("res.status", "lt", "201");
    pass("res.status", "lte", "201");
    pass("res.body.ratio", "lt", "1");
    error("res.body.name", "gt", "1", "needs a number");
    error("res.status", "gt", "abc", "needs a number");
    pass("res.status", "between", "200, 299");
    pass("res.status", "between", "[201,201]");
    fail("res.status", "between", "300, 399");
    error("res.status", "between", "200", "two numbers");
    error("res.body.name", "between", "1, 2", "needs a number");
}

#[test]
fn in_and_not_in() {
    pass("res.status", "in", "200, 201, 204");
    pass("res.status", "in", "[200,201]");
    fail("res.status", "in", "200, 204");
    pass("res.status", "notIn", "400, 500");
    fail("res.status", "notIn", "201");
    pass("res.body.token", "in", "a, tok-123");
    pass("res.body.ok", "in", "false, true");
}

#[test]
fn contains_and_not_contains() {
    pass("res.body.name", "contains", "Love");
    fail("res.body.name", "contains", "love");
    pass("res.body.name", "notContains", "Babbage");
    pass("res.body.tags", "contains", "b");
    pass("res.body.tags", "notContains", "z");
    error("res.body.id", "contains", "7", "text or an array");
    error("res.body.nope", "contains", "x", "did not resolve");
}

#[test]
fn length() {
    pass("res.body.tags", "length", "3");
    fail("res.body.tags", "length", "2");
    pass("res.body.name", "length", "12");
    pass("res.body.empty", "length", "0");
    error("res.body.blank", "length", "0", "text or an array");
    error("res.body.tags", "length", "three", "needs a number");
}

#[test]
fn matches_and_not_matches() {
    pass("res.body.token", "matches", "^tok-[0-9]+$");
    pass("res.body.token", "matches", "/^tok-\\d+$/");
    fail("res.body.token", "matches", "^abc");
    pass("res.body.token", "notMatches", "^abc");
    // Numbers are read as text.
    pass("res.status", "matches", "^2\\d\\d$");
    error(
        "res.body.token",
        "matches",
        "(",
        "invalid regular expression",
    );
    error("res.body.nope", "matches", "x", "did not resolve");
}

#[test]
fn starts_and_ends_with() {
    pass("res.body.name", "startsWith", "Ada");
    fail("res.body.name", "startsWith", "Lovelace");
    pass("res.body.name", "endsWith", "Lovelace");
    pass("res.status", "startsWith", "2");
    error("res.body.nope", "endsWith", "x", "did not resolve");
}

#[test]
fn emptiness() {
    pass("res.body.empty", "isEmpty", "");
    pass("res.body.blank", "isEmpty", "");
    pass("res.body.nothing", "isEmpty", "");
    fail("res.body.tags", "isEmpty", "");
    pass("res.body.tags", "isNotEmpty", "");
    pass("res.body.name", "isNotEmpty", "");
    fail("res.body.empty", "isNotEmpty", "");
    error(
        "res.body.id",
        "isEmpty",
        "",
        "needs a text, an array or an object",
    );
}

#[test]
fn null_undefined_defined() {
    pass("res.body.none", "isNull", "");
    fail("res.body.id", "isNull", "");
    pass("res.body.nope", "isUndefined", "");
    fail("res.body.none", "isUndefined", "");
    pass("res.body.none", "isDefined", "");
    let r = one("res.body.nope", "isDefined", "");
    assert!(!r.passed);
}

#[test]
fn truthiness() {
    for truthy in [
        "res.body.id",
        "res.body.ok",
        "res.body.name",
        "res.body.blank",
        "res.body.nothing",
    ] {
        pass(truthy, "isTruthy", "");
        fail(truthy, "isFalsy", "");
    }
    for falsy in [
        "res.body.zero",
        "res.body.empty",
        "res.body.none",
        "res.body.nope",
    ] {
        pass(falsy, "isFalsy", "");
    }
    fail("res.body.zero", "isTruthy", "");
}

#[test]
fn types() {
    pass("res.body", "isJson", "");
    fail("res.body.tags", "isJson", "");
    fail("res.body.name", "isJson", "");
    pass("res.body.id", "isNumber", "");
    pass("res.status", "isNumber", "");
    fail("res.body.token", "isNumber", "");
    pass("res.body.token", "isString", "");
    pass("res.headers.x-request-id", "isString", "");
    pass("res.body.ok", "isBoolean", "");
    fail("res.body.id", "isBoolean", "");
    pass("res.body.tags", "isArray", "");
    fail("res.body.blank", "isArray", "");
}

#[test]
fn unary_operators_ignore_the_value() {
    let r = one("res.body.ok", "isTruthy", "whatever {{expectedId}}");
    assert!(r.passed);
    assert_eq!(r.expected, "");
}

#[test]
fn unknown_operators_fail_and_are_reported() {
    let r = error(
        "res.status",
        "approximately",
        "200",
        "unknown operator `approximately`",
    );
    assert_eq!(r.operator, "approximately");
    assert_eq!(r.actual.as_deref(), Some("201"));
}

#[test]
fn disabled_assertions_are_skipped_and_order_is_kept() {
    let mut off = a("res.status", "eq", "500");
    off.enabled = false;
    let results = evaluate(
        &[
            a("res.status", "eq", "201"),
            off,
            a("res.body.id", "eq", "8"),
        ],
        &response(),
        &scope(),
    );
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].expression, "res.status");
    assert!(results[0].passed);
    assert_eq!(results[1].expression, "res.body.id");
    assert!(!results[1].passed);
    assert_eq!(results[1].error, None, "a plain mismatch has no error");
}

#[test]
fn every_bruno_operator_is_known() {
    let unary = [
        "isEmpty",
        "isNotEmpty",
        "isNull",
        "isUndefined",
        "isDefined",
        "isTruthy",
        "isFalsy",
        "isJson",
        "isNumber",
        "isString",
        "isBoolean",
        "isArray",
    ];
    let binary = [
        "eq",
        "neq",
        "gt",
        "gte",
        "lt",
        "lte",
        "in",
        "notIn",
        "contains",
        "notContains",
        "length",
        "matches",
        "notMatches",
        "startsWith",
        "endsWith",
        "between",
    ];
    for op in unary.iter().chain(&binary) {
        let r = one("res.body.tags", op, "1, 2");
        let e = r.error.unwrap_or_default();
        assert!(!e.contains("unknown operator"), "{op}: {e}");
    }
}

// ---- actions ----

fn action(expression: &str, method: &str, variable: &str, scope: VarScope) -> VarAction {
    VarAction {
        phase: ScriptPhase::AfterResponse,
        expression: expression.into(),
        method: method.into(),
        variable: variable.into(),
        scope,
        enabled: true,
    }
}

fn change(name: &str, value: &str, scope: VarScope) -> VarChange {
    VarChange {
        name: name.into(),
        value: Some(value.into()),
        scope,
    }
}

#[test]
fn set_variable_actions() {
    let mut disabled = action("res.body.id", "jsonq", "off", VarScope::Runtime);
    disabled.enabled = false;
    let mut before = action("res.body.id", "jsonq", "early", VarScope::Runtime);
    before.phase = ScriptPhase::BeforeRequest;
    let changes = apply_actions(
        &[
            action("res.body.token", "jsonq", "token", VarScope::Runtime),
            action("res.body.id", "jsonq", "id", VarScope::Environment),
            action("res.body.items[1]", "jsonq", "item", VarScope::Collection),
            action("res.body.ok", "jsonq", "ok", VarScope::Global),
            action("$.items[0].name", "jsonq", "first", VarScope::Runtime),
            action("items[1].id", "jsonq", "second", VarScope::Runtime),
            action(
                "res.headers.x-request-id",
                "jsonq",
                "fromHeaderExpr",
                VarScope::Runtime,
            ),
            action("X-REQUEST-ID", "header", "requestId", VarScope::Runtime),
            action("", "status", "status", VarScope::Runtime),
            disabled,
            before,
            action("res.body.nope", "jsonq", "missing", VarScope::Runtime),
            action(
                "Promise.resolve().then(() => 1)",
                "jsonq",
                "js",
                VarScope::Runtime,
            ),
            action("res.body.id", "xpath", "unsupported", VarScope::Runtime),
            action("x-none", "header", "noHeader", VarScope::Runtime),
            action("res.body.id", "jsonq", "  ", VarScope::Runtime),
        ],
        &response(),
    );
    assert_eq!(
        changes,
        vec![
            change("token", "tok-123", VarScope::Runtime),
            change("id", "7", VarScope::Environment),
            change("item", r#"{"id":2,"name":"second"}"#, VarScope::Collection),
            change("ok", "true", VarScope::Global),
            change("first", "first", VarScope::Runtime),
            change("second", "2", VarScope::Runtime),
            change("fromHeaderExpr", "req-42", VarScope::Runtime),
            change("requestId", "req-42", VarScope::Runtime),
            change("status", "201", VarScope::Runtime),
        ]
    );
}
