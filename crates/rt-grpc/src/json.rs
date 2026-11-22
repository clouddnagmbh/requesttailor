// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Messages as JSON (proto3 JSON mapping) and the JSON skeleton of a message type.

use prost_reflect::prost::Message as _;
use prost_reflect::{
    DeserializeOptions, DynamicMessage, FieldDescriptor, Kind, MessageDescriptor, SerializeOptions,
};
use serde_json::Value;

use crate::GrpcError;

/// JSON text → protobuf bytes. Empty text is the empty message.
pub(crate) fn encode(desc: MessageDescriptor, json: &str) -> Result<Vec<u8>, GrpcError> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut de = serde_json::Deserializer::from_str(json);
    let message = DynamicMessage::deserialize_with_options(
        desc.clone(),
        &mut de,
        &DeserializeOptions::new().deny_unknown_fields(true),
    )
    .map_err(|e| GrpcError::Message(format!("{}: {e}", desc.full_name())))?;
    de.end()
        .map_err(|e| GrpcError::Message(format!("{}: {e}", desc.full_name())))?;
    Ok(message.encode_to_vec())
}

/// Protobuf bytes → pretty JSON (two-space indent).
pub(crate) fn decode(desc: MessageDescriptor, bytes: &[u8]) -> Result<String, GrpcError> {
    let message = DynamicMessage::decode(desc.clone(), bytes)
        .map_err(|e| GrpcError::Message(format!("{}: {e}", desc.full_name())))?;
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::pretty(&mut out);
    message
        .serialize_with_options(&mut ser, &SerializeOptions::new())
        .map_err(|e| GrpcError::Message(format!("{}: {e}", desc.full_name())))?;
    String::from_utf8(out).map_err(|e| GrpcError::Message(e.to_string()))
}

/// Every field with its default, nested messages expanded (recursive types once), the first
/// field of each `oneof` only, one element in lists and maps. Fields keep their order.
pub(crate) fn template(desc: &MessageDescriptor) -> String {
    let mut out = String::new();
    message_value(desc, &mut Vec::new()).write(&mut out, 0);
    out
}

/// A JSON value whose object keys keep the field order (serde_json's map sorts them).
enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    Scalar(Value),
}

impl Json {
    fn write(&self, out: &mut String, indent: usize) {
        fn pad(out: &mut String, n: usize) {
            out.extend(std::iter::repeat_n(' ', n));
        }
        match self {
            Json::Scalar(v) => out.push_str(&v.to_string()),
            Json::Object(entries) if entries.is_empty() => out.push_str("{}"),
            Json::Array(items) if items.is_empty() => out.push_str("[]"),
            Json::Object(entries) => {
                out.push_str("{\n");
                for (i, (key, value)) in entries.iter().enumerate() {
                    pad(out, indent + 2);
                    out.push_str(&Value::from(key.as_str()).to_string());
                    out.push_str(": ");
                    value.write(out, indent + 2);
                    out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
                }
                pad(out, indent);
                out.push('}');
            }
            Json::Array(items) => {
                out.push_str("[\n");
                for (i, value) in items.iter().enumerate() {
                    pad(out, indent + 2);
                    value.write(out, indent + 2);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                pad(out, indent);
                out.push(']');
            }
        }
    }
}

fn message_value(desc: &MessageDescriptor, stack: &mut Vec<String>) -> Json {
    if let Some(v) = well_known(desc.full_name()) {
        return v;
    }
    if stack.iter().any(|s| s == desc.full_name()) || stack.len() > 16 {
        return Json::Object(Vec::new());
    }
    stack.push(desc.full_name().to_owned());
    let mut entries = Vec::new();
    let mut oneofs_seen: Vec<String> = Vec::new();
    for field in desc.fields() {
        if let Some(oneof) = field.containing_oneof() {
            // Proto3 `optional` is a synthetic oneof with one field: always included.
            let synthetic = oneof.fields().len() == 1 && oneof.name().starts_with('_');
            if !synthetic {
                if oneofs_seen.iter().any(|o| o == oneof.name()) {
                    continue;
                }
                oneofs_seen.push(oneof.name().to_owned());
            }
        }
        entries.push((field.json_name().to_owned(), field_value(&field, stack)));
    }
    stack.pop();
    Json::Object(entries)
}

fn field_value(field: &FieldDescriptor, stack: &mut Vec<String>) -> Json {
    if field.is_map() {
        let Kind::Message(entry) = field.kind() else {
            return Json::Object(Vec::new());
        };
        let key = match scalar_value(&entry.map_entry_key_field().kind(), stack) {
            Json::Scalar(Value::String(s)) if !s.is_empty() => s,
            Json::Scalar(Value::String(_)) => "key".to_owned(),
            Json::Scalar(other) => other.to_string(),
            _ => "key".to_owned(),
        };
        let value = scalar_value(&entry.map_entry_value_field().kind(), stack);
        return Json::Object(vec![(key, value)]);
    }
    let one = scalar_value(&field.kind(), stack);
    if field.is_list() {
        Json::Array(vec![one])
    } else {
        one
    }
}

fn scalar_value(kind: &Kind, stack: &mut Vec<String>) -> Json {
    Json::Scalar(match kind {
        Kind::Double | Kind::Float => Value::from(0.0),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 | Kind::Uint32 | Kind::Fixed32 => {
            Value::from(0)
        }
        // 64-bit integers are strings in the proto3 JSON mapping.
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 | Kind::Uint64 | Kind::Fixed64 => {
            Value::from("0")
        }
        Kind::Bool => Value::Bool(false),
        Kind::String | Kind::Bytes => Value::from(""),
        Kind::Enum(e) => Value::from(e.default_value().name()),
        Kind::Message(m) => return message_value(m, stack),
    })
}

/// Well-known types have their own JSON form.
fn well_known(name: &str) -> Option<Json> {
    Some(match name {
        "google.protobuf.Timestamp" => Json::Scalar(Value::from("1970-01-01T00:00:00Z")),
        "google.protobuf.Duration" => Json::Scalar(Value::from("0s")),
        "google.protobuf.FieldMask" => Json::Scalar(Value::from("")),
        "google.protobuf.Struct" | "google.protobuf.Empty" => Json::Object(Vec::new()),
        "google.protobuf.Value" => Json::Scalar(Value::Null),
        "google.protobuf.ListValue" => Json::Array(Vec::new()),
        "google.protobuf.StringValue" | "google.protobuf.BytesValue" => {
            Json::Scalar(Value::from(""))
        }
        "google.protobuf.BoolValue" => Json::Scalar(Value::Bool(false)),
        "google.protobuf.Int64Value" | "google.protobuf.UInt64Value" => {
            Json::Scalar(Value::from("0"))
        }
        "google.protobuf.Int32Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.DoubleValue"
        | "google.protobuf.FloatValue" => Json::Scalar(Value::from(0)),
        "google.protobuf.Any" => {
            Json::Object(vec![("@type".into(), Json::Scalar(Value::from("")))])
        }
        _ => return None,
    })
}
