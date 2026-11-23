// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reads the model's answer: the JSON block the prompt asked for, checked and turned into an
//! [`AiAnswer`]. A request proposal inherits auth; nothing here is saved.

use rt_core::model::{
    Assertion, Auth, Body, KeyValue, Param, ParamKind, RawKind, RequestSpec, Script, ScriptPhase,
};
use serde_json::{Map, Value};

use super::{AiAnswer, AiError, AiTask};

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Rust's `rt_core::assert` operators (Bruno's set).
const OPERATORS: &[&str] = &[
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

/// Longest explanation kept (characters).
const MAX_TEXT: usize = 20_000;

fn bad(reason: impl Into<String>) -> AiError {
    AiError::Answer(reason.into())
}

/// The JSON object of the answer: the first ```json block, else the first fenced block,
/// else the text from the first `{` to the last `}`.
fn json_block(text: &str) -> Option<Map<String, Value>> {
    let mut candidates: Vec<&str> = Vec::new();
    for tag in ["```json", "```JSON", "```"] {
        if let Some(start) = text.find(tag) {
            let rest = &text[start + tag.len()..];
            let rest = rest.split_once('\n').map(|(_, r)| r).unwrap_or(rest);
            if let Some(end) = rest.find("```") {
                candidates.push(&rest[..end]);
            }
        }
    }
    if let (Some(a), Some(b)) = (text.find('{'), text.rfind('}'))
        && a < b
    {
        candidates.push(&text[a..=b]);
    }
    candidates
        .into_iter()
        .find_map(|c| match serde_json::from_str::<Value>(c.trim()) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        })
}

fn string(map: &Map<String, Value>, key: &str) -> String {
    match map.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn text_of(map: &Map<String, Value>) -> String {
    string(map, "text").chars().take(MAX_TEXT).collect()
}

fn pairs(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| i.as_object())
        .map(|o| (string(o, "name"), string(o, "value")))
        .filter(|(n, _)| !n.trim().is_empty())
        .collect()
}

fn body(value: Option<&Value>) -> Result<Body, AiError> {
    let Some(Value::Object(b)) = value else {
        return Ok(Body::None);
    };
    let data = match b.get("data") {
        Some(Value::String(s)) => s.clone(),
        // The model gave the JSON itself instead of its text.
        Some(v @ (Value::Object(_) | Value::Array(_))) => {
            serde_json::to_string_pretty(v).unwrap_or_default()
        }
        _ => String::new(),
    };
    let raw = |kind| Body::Raw {
        kind,
        data: data.clone(),
    };
    Ok(match string(b, "type").to_ascii_lowercase().as_str() {
        "" | "none" => Body::None,
        "json" => raw(RawKind::Json),
        "text" => raw(RawKind::Text),
        "xml" => raw(RawKind::Xml),
        "form" | "form-urlencoded" | "form-url-encoded" => Body::FormUrlEncoded {
            fields: pairs(b.get("fields"))
                .into_iter()
                .map(|(n, v)| KeyValue::new(n, v))
                .collect(),
        },
        other => return Err(bad(format!("unknown body type `{other}`"))),
    })
}

fn request(map: &Map<String, Value>) -> Result<RequestSpec, AiError> {
    let Some(Value::Object(r)) = map.get("request") else {
        return Err(bad("no request in the answer"));
    };
    let method = string(r, "method").trim().to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(bad(format!("`{method}` is not an HTTP method")));
    }
    let url = string(r, "url").trim().to_owned();
    if url.is_empty() || url.chars().any(char::is_control) {
        return Err(bad("the proposed request has no URL"));
    }
    let name = match string(r, "name").trim() {
        "" => format!("{method} request"),
        n => n.chars().take(120).collect(),
    };
    let params = match r.get("params") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_object())
            .filter(|o| !string(o, "name").trim().is_empty())
            .map(|o| Param {
                name: string(o, "name"),
                value: string(o, "value"),
                kind: if string(o, "kind") == "path" {
                    ParamKind::Path
                } else {
                    ParamKind::Query
                },
                enabled: true,
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(RequestSpec {
        name,
        method,
        url,
        headers: pairs(r.get("headers"))
            .into_iter()
            .map(|(n, v)| KeyValue::new(n, v))
            .collect(),
        params,
        body: body(r.get("body"))?,
        auth: Auth::Inherit,
        ..RequestSpec::default()
    })
}

fn assertions(value: Option<&Value>) -> Vec<Assertion> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| i.as_object())
        .filter_map(|o| {
            let expression = string(o, "expression").trim().to_owned();
            let operator = string(o, "operator").trim().to_owned();
            if expression.is_empty() || !OPERATORS.contains(&operator.as_str()) {
                return None;
            }
            let description = string(o, "description");
            Some(Assertion {
                expression,
                operator,
                value: string(o, "value"),
                description: (!description.trim().is_empty()).then_some(description),
                enabled: true,
            })
        })
        .collect()
}

/// The answer to `task` in the model's `text`.
pub fn parse(task: &AiTask, text: &str) -> Result<AiAnswer, AiError> {
    let block = json_block(text);
    match task {
        // Wave 16, strand B: their parsers. `prepare` refuses these tasks until then.
        AiTask::RequestFromSnippet { .. }
        | AiTask::ExplainResponse { .. }
        | AiTask::SuggestAssertions { .. }
        | AiTask::SuggestNames { .. }
        | AiTask::DraftDocs { .. } => Err(AiError::Unsupported(
            "this assistant task arrives with wave 16".to_owned(),
        )),
        AiTask::CreateRequest { .. } => {
            let map = block.ok_or_else(|| bad("no JSON block with a request in the answer"))?;
            Ok(AiAnswer {
                text: text_of(&map),
                request: Some(request(&map)?),
                ..Default::default()
            })
        }
        AiTask::ExplainError { .. } => {
            // A model that forgets the block still explains: take its text.
            let text = match block {
                Some(map) => text_of(&map),
                None => text.trim().chars().take(MAX_TEXT).collect(),
            };
            if text.trim().is_empty() {
                return Err(bad("the answer was empty"));
            }
            Ok(AiAnswer {
                text,
                ..Default::default()
            })
        }
        AiTask::SuggestTests { .. } => {
            let map = block.ok_or_else(|| bad("no JSON block with tests in the answer"))?;
            let code = string(&map, "tests").trim().to_owned();
            let scripts: Vec<Script> = if code.is_empty() {
                Vec::new()
            } else {
                vec![Script {
                    phase: ScriptPhase::Tests,
                    code,
                }]
            };
            let assertions = assertions(map.get("assertions"));
            if scripts.is_empty() && assertions.is_empty() {
                return Err(bad("no tests or assertions in the answer"));
            }
            Ok(AiAnswer {
                text: text_of(&map),
                scripts,
                assertions,
                ..Default::default()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create() -> AiTask {
        AiTask::CreateRequest {
            description: "d".into(),
            folder: String::new(),
        }
    }

    fn tests_task() -> AiTask {
        AiTask::SuggestTests {
            path: "a.yml".into(),
            response: 1,
        }
    }

    #[test]
    fn a_request_from_a_json_block() {
        let text = "Here it is:\n```json\n{\"text\": \"Creates a user.\", \"request\": {\"name\": \"Create user\", \"method\": \"post\", \"url\": \"{{baseUrl}}/users\", \"headers\": [{\"name\": \"Content-Type\", \"value\": \"application/json\"}], \"params\": [{\"name\": \"id\", \"value\": \"1\", \"kind\": \"path\"}], \"body\": {\"type\": \"json\", \"data\": {\"name\": \"{{name}}\"}}}}\n```";
        let a = parse(&create(), text).unwrap();
        let r = a.request.unwrap();
        assert_eq!(a.text, "Creates a user.");
        assert_eq!(
            (r.name.as_str(), r.method.as_str()),
            ("Create user", "POST")
        );
        assert_eq!(r.url, "{{baseUrl}}/users");
        assert_eq!(
            r.headers,
            [KeyValue::new("Content-Type", "application/json")]
        );
        assert_eq!(r.params[0].kind, ParamKind::Path);
        assert_eq!(r.auth, Auth::Inherit);
        match r.body {
            Body::Raw { kind, data } => {
                assert_eq!(kind, RawKind::Json);
                assert!(data.contains("\"name\": \"{{name}}\""), "{data}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invalid_requests_are_refused_with_a_reason() {
        for (text, reason) in [
            ("no json here", "no JSON block"),
            ("```json\n{\"text\": \"x\"}\n```", "no request"),
            (
                "{\"request\": {\"method\": \"FETCH\", \"url\": \"x\"}}",
                "not an HTTP method",
            ),
            (
                "{\"request\": {\"method\": \"GET\", \"url\": \" \"}}",
                "no URL",
            ),
            (
                "{\"request\": {\"method\": \"GET\", \"url\": \"x\", \"body\": {\"type\": \"yaml\"}}}",
                "unknown body type",
            ),
        ] {
            match parse(&create(), text) {
                Err(AiError::Answer(m)) => assert!(m.contains(reason), "{m}"),
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn explanations_with_or_without_a_block() {
        let task = AiTask::ExplainError {
            path: "a.yml".into(),
            response: 1,
        };
        assert_eq!(
            parse(
                &task,
                "```json\n{\"text\": \"**401**: the token expired.\"}\n```"
            )
            .unwrap()
            .text,
            "**401**: the token expired."
        );
        assert_eq!(parse(&task, " plain words ").unwrap().text, "plain words");
        assert!(matches!(parse(&task, "  "), Err(AiError::Answer(_))));
    }

    #[test]
    fn tests_and_assertions() {
        let text = r#"```json
{"text": "Checks status and body.", "tests": "test(\"ok\", () => expect(rt.response.status).to.equal(200));",
 "assertions": [{"expression": "res.status", "operator": "eq", "value": "200", "description": "OK"},
                {"expression": "res.body.items", "operator": "isArray"},
                {"expression": "res.body", "operator": "looksGood"}]}
```"#;
        let a = parse(&tests_task(), text).unwrap();
        assert_eq!(a.scripts.len(), 1);
        assert_eq!(a.scripts[0].phase, ScriptPhase::Tests);
        assert!(a.scripts[0].code.contains("rt.response.status"));
        // The unknown operator is left out.
        assert_eq!(a.assertions.len(), 2);
        assert_eq!(a.assertions[0].description.as_deref(), Some("OK"));
        assert_eq!(a.assertions[1].value, "");
        assert!(matches!(
            parse(&tests_task(), "{\"text\": \"nothing\"}"),
            Err(AiError::Answer(_))
        ));
    }
}
