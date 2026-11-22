// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Declarative assertions and variable actions (wave 7, strand C), evaluated without
//! JavaScript so they behave the same everywhere and need no trust.
//!
//! # Expressions
//!
//! - `res.status` (number), `res.statusText` (the standard reason phrase of the status),
//!   `res.responseTime` (milliseconds, number).
//! - `res.headers` (an object, lowercase names), `res.headers.<name>` and
//!   `res.headers["a-b"]`: case-insensitive; several headers of one name are joined with
//!   `", "`.
//! - `res.body`: the JSON body, or the body as text when it is not JSON. Paths into it:
//!   `res.body.items[0].id`, `res.body["a b"]`, `res.body.items.length` (length of an array
//!   or a string, unless an object has a `length` key).
//! - Anything else is a variable name, resolved from `scope`. Its value is read like an
//!   expected value (below), so a variable `7` is the number 7, `true` a boolean.
//!
//! A path that does not resolve gives `undefined` (as in JavaScript); `isUndefined` and
//! `isDefined` test for it, every other operator fails with an error naming the expression.
//!
//! # Expected values (Bruno's rules)
//!
//! `{{variables}}` are resolved from `scope` first. Then: `true`/`false`, `null`,
//! `undefined`, numbers (up to `Number.MAX_SAFE_INTEGER`; larger ones stay text) and text in
//! `"…"` or `'…'` quotes are those values; anything else is text. `in`/`notIn` take a list
//! `a, b` or `[a, b]`, `between` takes `low, high` (inclusive), `matches`/`notMatches` a
//! regular expression `^a` or `/^a/`. Unary operators ignore the value.
//!
//! # Operators
//!
//! See [`crate::model::Assertion::operator`]. Comparisons are strict like Bruno's (chai):
//! `eq` compares numbers numerically and text as text, the number 200 is not the text
//! `"200"`, objects and arrays never equal a literal. `gt`/`gte`/`lt`/`lte`/`between` need
//! numbers; `contains` works on text (substring) and arrays (element); `length` on text and
//! arrays; `startsWith`/`endsWith`/`matches` read the value as text. An empty operator is
//! `eq`; an unknown one fails the assertion with an error.

use serde_json::{Map, Value};

use crate::model::{Assertion, ScriptPhase, VarAction};
use crate::script::{AssertionResult, ScriptResponse, VarChange};
use crate::vars::Scope;

/// Evaluates the enabled assertions against the response. `scope` resolves `{{variables}}` in
/// expected values and bare variable names as expressions. Disabled assertions are skipped
/// (not reported).
pub fn evaluate(
    assertions: &[Assertion],
    response: &ScriptResponse,
    scope: &Scope,
) -> Vec<AssertionResult> {
    let body = ParsedBody::new(&response.body);
    assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|a| evaluate_one(a, response, &body, scope))
        .collect()
}

/// The variable changes of the enabled `set-variable` actions, in order.
///
/// Methods: `jsonq` (the expression is a response expression like `res.body.token` or
/// `res.headers.x`, a JSONPath-like `$.token`, or a bare path `token` into the JSON body),
/// `header` (a header name, case-insensitive), `status` (the status code). The value is the
/// text of a string and the JSON of anything else. Other methods, actions that are disabled
/// or run before the request, actions without a variable name and expressions that do not
/// resolve change nothing.
pub fn apply_actions(actions: &[VarAction], response: &ScriptResponse) -> Vec<VarChange> {
    let body = ParsedBody::new(&response.body);
    actions
        .iter()
        .filter(|a| a.enabled && a.phase != ScriptPhase::BeforeRequest)
        .filter(|a| !a.variable.trim().is_empty())
        .filter_map(|a| {
            let value = match a.method.as_str() {
                "jsonq" => action_jsonq(&a.expression, response, &body)?,
                "header" => {
                    let name = a.expression.trim();
                    let name = name.strip_prefix("res.headers.").unwrap_or(name);
                    header(response, name)?
                }
                "status" => Value::from(response.status),
                _ => return None,
            };
            Some(VarChange {
                name: a.variable.trim().to_owned(),
                value: Some(text_of(&value)),
                scope: a.scope,
            })
        })
        .collect()
}

// ---- values ----

/// A value as JavaScript sees it: `undefined` or a JSON value.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Undefined,
    Json(Value),
}

impl Val {
    fn json(&self) -> Option<&Value> {
        match self {
            Val::Undefined => None,
            Val::Json(v) => Some(v),
        }
    }

    /// The value as text for the report; `None` for `undefined`.
    fn actual(&self) -> Option<String> {
        self.json().map(text_of)
    }

    /// `String(value)` in JavaScript, for operators that read text.
    fn as_text(&self) -> String {
        match self {
            Val::Undefined => "undefined".into(),
            Val::Json(Value::String(s)) => s.clone(),
            Val::Json(Value::Array(a)) => a
                .iter()
                .map(|v| match v {
                    Value::Null => String::new(),
                    other => Val::Json(other.clone()).as_text(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Val::Json(Value::Object(_)) => "[object Object]".into(),
            Val::Json(other) => other.to_string(),
        }
    }

    fn number(&self) -> Option<f64> {
        self.json().and_then(Value::as_f64)
    }

    fn truthy(&self) -> bool {
        match self {
            Val::Undefined | Val::Json(Value::Null) => false,
            Val::Json(Value::Bool(b)) => *b,
            Val::Json(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
            Val::Json(Value::String(s)) => !s.is_empty(),
            Val::Json(_) => true,
        }
    }

    /// A short description for error messages: `the number 3`, `the text "a"`.
    fn describe(&self) -> String {
        match self {
            Val::Undefined => "undefined".into(),
            Val::Json(Value::Null) => "null".into(),
            Val::Json(Value::Bool(b)) => format!("the boolean {b}"),
            Val::Json(Value::Number(n)) => format!("the number {n}"),
            Val::Json(Value::String(s)) => format!("the text {}", Value::String(s.clone())),
            Val::Json(Value::Array(_)) => "an array".into(),
            Val::Json(Value::Object(_)) => "an object".into(),
        }
    }
}

/// Strings as they are, everything else as JSON.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// JavaScript's `===` (objects and arrays are never equal: no reference is shared).
fn strict_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Undefined, Val::Undefined) => true,
        (Val::Json(Value::Number(x)), Val::Json(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Val::Json(Value::Array(_) | Value::Object(_)), _)
        | (_, Val::Json(Value::Array(_) | Value::Object(_))) => false,
        (Val::Json(x), Val::Json(y)) => x == y,
        _ => false,
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Bruno's `evaluateJsTemplateLiteral` without the template part: literals become values,
/// the rest stays text.
fn literal(raw: &str) -> Val {
    let s = raw.trim();
    match s {
        "" => return Val::Json(Value::String(raw.to_owned())),
        "true" => return Val::Json(Value::Bool(true)),
        "false" => return Val::Json(Value::Bool(false)),
        "null" => return Val::Json(Value::Null),
        "undefined" => return Val::Undefined,
        _ => {}
    }
    for quote in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(quote) && s.ends_with(quote) {
            return Val::Json(Value::String(s[1..s.len() - 1].to_owned()));
        }
    }
    if let Some(n) = js_number(s) {
        if n.abs() > MAX_SAFE_INTEGER {
            return Val::Json(Value::String(s.to_owned()));
        }
        return Val::Json(number_value(n));
    }
    Val::Json(Value::String(s.to_owned()))
}

/// `Number(s)` for decimal and hex literals; `None` where JavaScript gives `NaN` (or for
/// `Infinity`, which a response never contains).
fn js_number(s: &str) -> Option<f64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|n| n as f64);
    }
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    let valid = !digits.is_empty()
        && digits.chars().any(|c| c.is_ascii_digit())
        && digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if !valid {
        return None;
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Integers stay integers in JSON (`200`, not `200.0`).
fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

// ---- expressions ----

/// The body, parsed once per response.
struct ParsedBody(Value);

impl ParsedBody {
    fn new(bytes: &[u8]) -> Self {
        let value = serde_json::from_slice::<Value>(bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()));
        ParsedBody(value)
    }
}

#[derive(Debug, PartialEq)]
enum Segment {
    Key(String),
    Index(usize),
}

/// Parses `.a`, `["a b"]`, `['a']`, `[0]` segments. Keys after a dot run to the next `.` or
/// `[` (so `res.headers.content-type` works).
fn segments(mut rest: &str) -> Result<Vec<Segment>, String> {
    let mut out = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = after[..end].trim();
            if key.is_empty() {
                return Err("an empty name after `.`".into());
            }
            out.push(Segment::Key(key.to_owned()));
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let after = after.trim_start();
            let (segment, tail) =
                if let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let inner = &after[1..];
                    let close = inner
                        .find(quote)
                        .ok_or_else(|| format!("a missing closing {quote}"))?;
                    (Segment::Key(inner[..close].to_owned()), &inner[close + 1..])
                } else {
                    let close = after.find(']').ok_or("a missing `]`")?;
                    let index = after[..close].trim();
                    let index = index
                        .parse::<usize>()
                        .map_err(|_| format!("`[{index}]` is not an index or a quoted name"))?;
                    (Segment::Index(index), &after[close..])
                };
            rest = tail.trim_start().strip_prefix(']').ok_or("a missing `]`")?;
            out.push(segment);
        } else {
            return Err(format!("unexpected `{rest}`"));
        }
    }
    Ok(out)
}

/// Follows `path` into `value`; a missing step gives `undefined`.
fn walk(value: &Value, path: &[Segment]) -> Val {
    let mut current = value;
    for (i, segment) in path.iter().enumerate() {
        let next = match (current, segment) {
            (Value::Object(map), Segment::Key(k)) => map.get(k),
            (Value::Array(items), Segment::Index(n)) => items.get(*n),
            (Value::Array(items), Segment::Key(k)) if k == "length" => {
                return rest_of(Value::from(items.len()), &path[i + 1..]);
            }
            (Value::String(s), Segment::Key(k)) if k == "length" => {
                return rest_of(Value::from(s.encode_utf16().count()), &path[i + 1..]);
            }
            (Value::String(s), Segment::Index(n)) => {
                // JavaScript indexes UTF-16 code units; one character is close enough here.
                return match s.chars().nth(*n) {
                    Some(c) => rest_of(Value::String(c.to_string()), &path[i + 1..]),
                    None => Val::Undefined,
                };
            }
            _ => None,
        };
        match next {
            Some(v) => current = v,
            None => return Val::Undefined,
        }
    }
    Val::Json(current.clone())
}

fn rest_of(value: Value, path: &[Segment]) -> Val {
    walk(&value, path)
}

/// The value of header `name` (case-insensitive), several joined with `", "`.
fn header(response: &ScriptResponse, name: &str) -> Option<Value> {
    let values: Vec<&str> = response
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
        .collect();
    (!values.is_empty()).then(|| Value::String(values.join(", ")))
}

fn headers_object(response: &ScriptResponse) -> Value {
    let mut map = Map::new();
    for (name, _) in &response.headers {
        let key = name.to_ascii_lowercase();
        if !map.contains_key(&key) {
            let value = header(response, &key).unwrap_or(Value::Null);
            map.insert(key, value);
        }
    }
    Value::Object(map)
}

/// Evaluates a `res…` expression. `Ok(None)`: not a response expression.
fn response_expression(
    expression: &str,
    response: &ScriptResponse,
    body: &ParsedBody,
) -> Option<Result<Val, String>> {
    let rest = expression.strip_prefix("res")?;
    if !(rest.starts_with('.') || rest.starts_with('[')) {
        return None;
    }
    Some(segments(rest).and_then(|path| {
        let Some((Segment::Key(first), tail)) = path.split_first() else {
            return Err("`res` needs a property such as `res.status`".into());
        };
        let root = match first.as_str() {
            "status" => Value::from(response.status),
            "statusText" => Value::String(reason_phrase(response.status).into()),
            "responseTime" => Value::from(response.time_ms),
            "headers" => {
                // Header names are case-insensitive.
                return Ok(match tail.split_first() {
                    Some((Segment::Key(name), more)) => match header(response, name) {
                        Some(v) => walk(&v, more),
                        None => Val::Undefined,
                    },
                    Some((Segment::Index(_), _)) => Val::Undefined,
                    None => Val::Json(headers_object(response)),
                });
            }
            "body" => return Ok(walk(&body.0, tail)),
            other => {
                return Err(format!(
                    "`res.{other}` is not known (use res.status, res.statusText, \
                     res.responseTime, res.headers or res.body)"
                ));
            }
        };
        Ok(walk(&root, tail))
    }))
}

/// The left side of an assertion.
fn lhs(
    expression: &str,
    response: &ScriptResponse,
    body: &ParsedBody,
    scope: &Scope,
) -> Result<Val, String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err("the expression is empty".into());
    }
    if let Some(result) = response_expression(expression, response, body) {
        return result;
    }
    Ok(match scope.get(expression) {
        Some((value, _, _)) => literal(&scope.interpolate(value).value),
        None => Val::Undefined,
    })
}

fn action_jsonq(expression: &str, response: &ScriptResponse, body: &ParsedBody) -> Option<Value> {
    let expression = expression.trim();
    let value = if let Some(result) = response_expression(expression, response, body) {
        result.ok()?
    } else if let Some(rest) = expression.strip_prefix('$') {
        walk(&body.0, &segments(rest).ok()?)
    } else {
        let path = if expression.starts_with('[') {
            expression.to_owned()
        } else {
            format!(".{expression}")
        };
        walk(&body.0, &segments(&path).ok()?)
    };
    value.json().cloned()
}

// ---- operators ----

const UNARY: &[&str] = &[
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

const BINARY: &[&str] = &[
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

/// The parsed right side.
enum Rhs {
    None,
    One(Val),
    List(Vec<Val>),
    Pattern(String),
}

fn rhs(operator: &str, expected: &str) -> Rhs {
    match operator {
        op if UNARY.contains(&op) => Rhs::None,
        "in" | "notIn" | "between" => {
            let s = expected.trim();
            let s = s
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(s);
            Rhs::List(s.split(',').map(literal).collect())
        }
        "matches" | "notMatches" => {
            let s = expected.trim();
            let s = if s.len() >= 2 && s.starts_with('/') && s.ends_with('/') {
                &s[1..s.len() - 1]
            } else {
                s
            };
            Rhs::Pattern(s.to_owned())
        }
        _ => Rhs::One(literal(expected)),
    }
}

/// `Ok(passed)` or `Err(why it could not be evaluated)`.
fn apply(operator: &str, actual: &Val, rhs: &Rhs) -> Result<bool, String> {
    let one = || match rhs {
        Rhs::One(v) => v.clone(),
        _ => Val::Undefined,
    };
    let numbers = |name: &str| -> Result<(f64, f64), String> {
        let expected = one();
        let a = actual
            .number()
            .ok_or_else(|| format!("{name} needs a number, got {}", actual.describe()))?;
        let b = expected.number().ok_or_else(|| {
            format!(
                "{name} needs a number to compare with, got {}",
                expected.describe()
            )
        })?;
        Ok((a, b))
    };
    Ok(match operator {
        "eq" => strict_eq(actual, &one()),
        "neq" => !strict_eq(actual, &one()),
        "gt" => numbers("gt").map(|(a, b)| a > b)?,
        "gte" => numbers("gte").map(|(a, b)| a >= b)?,
        "lt" => numbers("lt").map(|(a, b)| a < b)?,
        "lte" => numbers("lte").map(|(a, b)| a <= b)?,
        "in" | "notIn" => {
            let Rhs::List(list) = rhs else { unreachable!() };
            let found = list.iter().any(|v| strict_eq(actual, v));
            if operator == "in" { found } else { !found }
        }
        "contains" | "notContains" => {
            let needle = one();
            let found = match actual {
                Val::Json(Value::String(s)) => s.contains(&needle.as_text()),
                Val::Json(Value::Array(items)) => items
                    .iter()
                    .any(|item| strict_eq(&Val::Json(item.clone()), &needle)),
                other => {
                    return Err(format!(
                        "{operator} needs a text or an array, got {}",
                        other.describe()
                    ));
                }
            };
            if operator == "contains" {
                found
            } else {
                !found
            }
        }
        "length" => {
            let expected = one();
            let want = expected
                .number()
                .ok_or_else(|| format!("length needs a number, got {}", expected.describe()))?;
            let len = match actual {
                Val::Json(Value::String(s)) => s.encode_utf16().count(),
                Val::Json(Value::Array(items)) => items.len(),
                other => {
                    return Err(format!(
                        "length needs a text or an array, got {}",
                        other.describe()
                    ));
                }
            };
            len as f64 == want
        }
        "matches" | "notMatches" => {
            let Rhs::Pattern(pattern) = rhs else {
                unreachable!()
            };
            if *actual == Val::Undefined {
                return Err(format!("{operator} needs a value, got undefined"));
            }
            let re = regex_lite::Regex::new(pattern)
                .map_err(|e| format!("invalid regular expression `{pattern}`: {e}"))?;
            let found = re.is_match(&actual.as_text());
            if operator == "matches" { found } else { !found }
        }
        "startsWith" | "endsWith" => {
            if *actual == Val::Undefined {
                return Err(format!("{operator} needs a value, got undefined"));
            }
            let (text, part) = (actual.as_text(), one().as_text());
            if operator == "startsWith" {
                text.starts_with(&part)
            } else {
                text.ends_with(&part)
            }
        }
        "between" => {
            let Rhs::List(list) = rhs else { unreachable!() };
            let [low, high] = list.as_slice() else {
                return Err("between needs two numbers: `low, high`".into());
            };
            let (Some(low), Some(high)) = (low.number(), high.number()) else {
                return Err("between needs two numbers: `low, high`".into());
            };
            let a = actual
                .number()
                .ok_or_else(|| format!("between needs a number, got {}", actual.describe()))?;
            low <= a && a <= high
        }
        "isEmpty" | "isNotEmpty" => {
            let empty = match actual {
                Val::Json(Value::String(s)) => s.is_empty(),
                Val::Json(Value::Array(a)) => a.is_empty(),
                Val::Json(Value::Object(o)) => o.is_empty(),
                other => {
                    return Err(format!(
                        "{operator} needs a text, an array or an object, got {}",
                        other.describe()
                    ));
                }
            };
            if operator == "isEmpty" { empty } else { !empty }
        }
        "isNull" => *actual == Val::Json(Value::Null),
        "isUndefined" => *actual == Val::Undefined,
        "isDefined" => *actual != Val::Undefined,
        "isTruthy" => actual.truthy(),
        "isFalsy" => !actual.truthy(),
        "isJson" => matches!(actual, Val::Json(Value::Object(_))),
        "isNumber" => matches!(actual, Val::Json(Value::Number(_))),
        "isString" => matches!(actual, Val::Json(Value::String(_))),
        "isBoolean" => matches!(actual, Val::Json(Value::Bool(_))),
        "isArray" => matches!(actual, Val::Json(Value::Array(_))),
        other => return Err(format!("unknown operator `{other}`")),
    })
}

fn evaluate_one(
    assertion: &Assertion,
    response: &ScriptResponse,
    body: &ParsedBody,
    scope: &Scope,
) -> AssertionResult {
    let operator = match assertion.operator.trim() {
        "" => "eq",
        op => op,
    };
    let known = UNARY.contains(&operator) || BINARY.contains(&operator);
    let expected = if UNARY.contains(&operator) {
        String::new()
    } else {
        scope.interpolate(&assertion.value).value
    };
    let mut result = AssertionResult {
        expression: assertion.expression.clone(),
        operator: assertion.operator.clone(),
        expected: expected.clone(),
        actual: None,
        passed: false,
        error: None,
    };
    let actual = match lhs(&assertion.expression, response, body, scope) {
        Ok(v) => v,
        Err(e) => {
            result.error = Some(format!(
                "cannot evaluate `{}`: {e}",
                assertion.expression.trim()
            ));
            return result;
        }
    };
    result.actual = actual.actual();
    if !known {
        result.error = Some(format!("unknown operator `{operator}`"));
        return result;
    }
    match apply(operator, &actual, &rhs(operator, &expected)) {
        Ok(passed) => {
            result.passed = passed;
            if !passed && actual == Val::Undefined && operator != "isDefined" {
                result.error = Some(format!(
                    "`{}` did not resolve (no such value)",
                    assertion.expression.trim()
                ));
            }
        }
        Err(e) => {
            result.error = Some(if actual == Val::Undefined {
                format!(
                    "`{}` did not resolve (no such value)",
                    assertion.expression.trim()
                )
            } else {
                e
            });
        }
    }
    result
}

/// The standard reason phrase of a status code (`res.statusText`); HTTP/2 has none on the
/// wire, so the standard one is used everywhere.
fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_parse_dots_brackets_and_quotes() {
        assert_eq!(
            segments(".items[0].id").unwrap(),
            vec![
                Segment::Key("items".into()),
                Segment::Index(0),
                Segment::Key("id".into())
            ]
        );
        assert_eq!(
            segments("[\"a b\"]['c.d'].content-type").unwrap(),
            vec![
                Segment::Key("a b".into()),
                Segment::Key("c.d".into()),
                Segment::Key("content-type".into())
            ]
        );
        assert!(segments("[x]").is_err());
        assert!(segments(".").is_err());
        assert!(segments("[\"a]").is_err());
    }

    #[test]
    fn literals_follow_bruno() {
        assert_eq!(literal("200"), Val::Json(Value::from(200)));
        assert_eq!(literal(" 1.5 "), Val::Json(Value::from(1.5)));
        assert_eq!(literal("0x1A"), Val::Json(Value::from(26)));
        assert_eq!(literal("true"), Val::Json(Value::Bool(true)));
        assert_eq!(literal("null"), Val::Json(Value::Null));
        assert_eq!(literal("undefined"), Val::Undefined);
        assert_eq!(literal("\"200\""), Val::Json(Value::String("200".into())));
        assert_eq!(literal("'x'"), Val::Json(Value::String("x".into())));
        assert_eq!(literal("abc"), Val::Json(Value::String("abc".into())));
        assert_eq!(literal("1e3"), Val::Json(Value::from(1000)));
        assert_eq!(
            literal("Infinity"),
            Val::Json(Value::String("Infinity".into()))
        );
        assert_eq!(
            literal("90071992547409910"),
            Val::Json(Value::String("90071992547409910".into()))
        );
    }
}
