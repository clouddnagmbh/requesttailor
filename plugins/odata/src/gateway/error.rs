// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reads the error body of an SAP Gateway (or CAP) OData service: V2 as JSON or XML, V4 as
//! JSON. Tolerant: whatever is missing stays `None`; a body that is not an OData error is not
//! an error of this reader (`parse` answers `None`).

use serde::Serialize;
use serde_json::Value;

use crate::metadata::xml;

/// Which of the three layouts the body had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    V2Json,
    V2Xml,
    V4Json,
}

/// One entry of `errordetails` (V2) or `details` (V4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    pub message: String,
    /// `error`, `warning`, `info` or `success` (V4's numeric severity is translated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// What the system says about solving it (`Error_Resolution`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Resolution {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Resolution {
    fn is_empty(&self) -> bool {
        self.transaction.is_none() && self.note.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatewayError {
    pub shape: Shape,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
    /// As sent, usually `yyyymmddhhmmss` (UTC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
    #[serde(skip_serializing_if = "Resolution::is_empty")]
    pub resolution: Resolution,
}

/// The error in a response body, by its media type and text. `None`: not an OData error.
pub fn parse(content_type: Option<&str>, text: &str) -> Option<GatewayError> {
    let media = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return parse_json(trimmed);
    }
    if trimmed.starts_with('<') && (media.contains("xml") || media.is_empty()) {
        return parse_xml(trimmed);
    }
    None
}

fn string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_json(text: &str) -> Option<GatewayError> {
    let root: Value = serde_json::from_str(text).ok()?;
    let error = root.get("error")?.as_object()?;
    // V2: `message` is `{ lang, value }`; V4: a string.
    let (shape, message) = match error.get("message") {
        Some(Value::Object(m)) => (Shape::V2Json, m.get("value").and_then(string)),
        Some(other) => (Shape::V4Json, string(other)),
        None => (Shape::V4Json, None),
    };
    let message = message?;
    let inner = error.get("innererror");
    let field = |name: &str| inner.and_then(|i| i.get(name)).and_then(string);
    let list = match shape {
        Shape::V2Json => inner.and_then(|i| i.get("errordetails")),
        _ => error.get("details"),
    };
    let details = list
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(json_detail).collect())
        .unwrap_or_default();
    let resolution = inner
        .and_then(|i| i.get("Error_Resolution"))
        .map(|r| Resolution {
            transaction: r.get("SAP_Transaction").and_then(string),
            note: r.get("SAP_Note").and_then(string),
        })
        .unwrap_or_default();
    Some(GatewayError {
        shape,
        code: error.get("code").and_then(string).unwrap_or_default(),
        message,
        transaction_id: field("transactionid"),
        timestamp: field("timestamp"),
        target: error.get("target").and_then(string),
        details,
        resolution,
    })
}

fn json_detail(item: &Value) -> Option<Detail> {
    let message = item.get("message").and_then(string)?;
    let severity = item
        .get("severity")
        .and_then(string)
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| {
            // V4: `@Common.numericSeverity` 1 success, 2 info, 3 warning, 4 error.
            item.get("@Common.numericSeverity")
                .and_then(Value::as_u64)
                .map(|n| numeric_severity(n).to_owned())
        });
    Some(Detail {
        code: item.get("code").and_then(string).unwrap_or_default(),
        message,
        severity,
        target: item.get("target").and_then(string),
    })
}

fn numeric_severity(n: u64) -> &'static str {
    match n {
        1 => "success",
        2 => "info",
        3 => "warning",
        _ => "error",
    }
}

fn parse_xml(text: &str) -> Option<GatewayError> {
    let root = xml::parse(text).ok()?;
    if root.name != "error" {
        return None;
    }
    let text_of = |e: &xml::Element, name: &str| {
        e.child(name)
            .map(|c| c.text.trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    let message = text_of(&root, "message")?;
    let inner = root.child("innererror");
    let from_inner = |name: &str| inner.and_then(|i| text_of(i, name));
    let details = inner
        .and_then(|i| i.child("errordetails"))
        .map(|d| {
            d.children_named("errordetail")
                .filter_map(|e| {
                    Some(Detail {
                        code: text_of(e, "code").unwrap_or_default(),
                        message: text_of(e, "message")?,
                        severity: text_of(e, "severity").map(|s| s.to_ascii_lowercase()),
                        target: text_of(e, "target"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let resolution = inner
        .and_then(|i| i.child("Error_Resolution"))
        .map(|r| Resolution {
            transaction: text_of(r, "SAP_Transaction"),
            note: text_of(r, "SAP_Note"),
        })
        .unwrap_or_default();
    Some(GatewayError {
        shape: Shape::V2Xml,
        code: text_of(&root, "code").unwrap_or_default(),
        message,
        transaction_id: from_inner("transactionid"),
        timestamp: from_inner("timestamp"),
        target: None,
        details,
        resolution,
    })
}

impl GatewayError {
    /// `yyyymmddhhmmss` as `2026-11-24 10:30:15 UTC`; anything else as sent.
    pub fn time(&self) -> Option<String> {
        let t = self.timestamp.as_deref()?;
        if t.len() == 14 && t.bytes().all(|b| b.is_ascii_digit()) {
            return Some(format!(
                "{}-{}-{} {}:{}:{} UTC",
                &t[0..4],
                &t[4..6],
                &t[6..8],
                &t[8..10],
                &t[10..12],
                &t[12..14]
            ));
        }
        Some(t.to_owned())
    }

    /// One line for the send's notes.
    pub fn summary(&self) -> String {
        let mut line = String::from("OData error");
        if !self.code.is_empty() {
            line.push(' ');
            line.push_str(&self.code);
        }
        line.push_str(": ");
        line.push_str(&self.message);
        if let Some(id) = &self.transaction_id {
            line.push_str(&format!(" (transaction ID {id})"));
        }
        line
    }
}
