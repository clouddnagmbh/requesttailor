// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The shape of compiled proto files as plain data (wave 18, strand H): what a comparison of two
//! versions needs, without the descriptor types. Messages, enums and services by full name,
//! fields by number, the `reserved` statements, and the streaming kind of every `rpc`. The
//! well-known types (`google/protobuf/*`) are left out: they are not part of a schema.

use std::collections::BTreeMap;

use prost_reflect::{Cardinality, DescriptorPool, Kind};

/// Compiled proto files, see the module description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtoSchema {
    /// Per file name (as imported): its package.
    pub files: BTreeMap<String, ProtoFile>,
    /// Messages by full name (`shop.Order.Item` for a nested one); map entries are not messages
    /// here, a `map<K, V>` field says so in its type.
    pub messages: BTreeMap<String, ProtoMessage>,
    pub enums: BTreeMap<String, ProtoEnum>,
    pub services: BTreeMap<String, ProtoService>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoFile {
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoMessage {
    pub file: String,
    /// Fields by number.
    pub fields: BTreeMap<u32, ProtoField>,
    /// `reserved 2, 15 to 20;` as inclusive ranges (`to max` is `u32::MAX`).
    pub reserved_numbers: Vec<(u32, u32)>,
    pub reserved_names: Vec<String>,
}

/// How often a field occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// proto3 without `optional`, or a message-typed field.
    Singular,
    /// `optional` (explicit presence on a scalar or enum), proto2 `optional`.
    Optional,
    Required,
    Repeated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoField {
    pub name: String,
    pub number: u32,
    /// A scalar (`int32`, `string`, …), the full name of a message or enum, or
    /// `map<key, value>` with the same notation for both.
    pub ty: String,
    pub label: Label,
    /// The `oneof` it is a member of (not the synthetic one of proto3 `optional`).
    pub oneof: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoEnum {
    pub file: String,
    /// `(name, number)` in declaration order.
    pub values: Vec<(String, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoService {
    pub file: String,
    pub methods: BTreeMap<String, ProtoMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoMethod {
    /// Full names of the request and response messages.
    pub input: String,
    pub output: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

fn scalar_or_name(kind: &Kind) -> String {
    match kind {
        Kind::Double => "double".into(),
        Kind::Float => "float".into(),
        Kind::Int32 => "int32".into(),
        Kind::Int64 => "int64".into(),
        Kind::Uint32 => "uint32".into(),
        Kind::Uint64 => "uint64".into(),
        Kind::Sint32 => "sint32".into(),
        Kind::Sint64 => "sint64".into(),
        Kind::Fixed32 => "fixed32".into(),
        Kind::Fixed64 => "fixed64".into(),
        Kind::Sfixed32 => "sfixed32".into(),
        Kind::Sfixed64 => "sfixed64".into(),
        Kind::Bool => "bool".into(),
        Kind::String => "string".into(),
        Kind::Bytes => "bytes".into(),
        Kind::Message(m) => m.full_name().to_owned(),
        Kind::Enum(e) => e.full_name().to_owned(),
    }
}

fn well_known(file: &str) -> bool {
    file.starts_with("google/protobuf/")
}

pub(crate) fn of(pool: &DescriptorPool) -> ProtoSchema {
    let mut schema = ProtoSchema::default();
    for file in pool.files().filter(|f| !well_known(f.name())) {
        schema.files.insert(
            file.name().to_owned(),
            ProtoFile {
                package: file.package_name().to_owned(),
            },
        );
    }
    for message in pool.all_messages() {
        let file = message.parent_file().name().to_owned();
        if well_known(&file) || message.is_map_entry() {
            continue;
        }
        let mut fields = BTreeMap::new();
        for field in message.fields() {
            let ty = match (field.is_map(), field.kind()) {
                (true, Kind::Message(entry)) => format!(
                    "map<{}, {}>",
                    scalar_or_name(&entry.map_entry_key_field().kind()),
                    scalar_or_name(&entry.map_entry_value_field().kind())
                ),
                (_, kind) => scalar_or_name(&kind),
            };
            let oneof = field
                .containing_oneof()
                .filter(|o| !o.is_synthetic())
                .map(|o| o.name().to_owned());
            let label = if field.is_list() || field.is_map() {
                Label::Repeated
            } else if field.cardinality() == Cardinality::Required {
                Label::Required
            } else if field.supports_presence()
                && oneof.is_none()
                && !matches!(field.kind(), Kind::Message(_))
            {
                Label::Optional
            } else {
                Label::Singular
            };
            fields.insert(
                field.number(),
                ProtoField {
                    name: field.name().to_owned(),
                    number: field.number(),
                    ty,
                    label,
                    oneof,
                },
            );
        }
        schema.messages.insert(
            message.full_name().to_owned(),
            ProtoMessage {
                file,
                fields,
                reserved_numbers: message
                    .reserved_ranges()
                    .map(|r| (r.start, r.end.saturating_sub(1)))
                    .collect(),
                reserved_names: message.reserved_names().map(str::to_owned).collect(),
            },
        );
    }
    for e in pool.all_enums() {
        let file = e.parent_file().name().to_owned();
        if well_known(&file) {
            continue;
        }
        schema.enums.insert(
            e.full_name().to_owned(),
            ProtoEnum {
                file,
                values: e
                    .values()
                    .map(|v| (v.name().to_owned(), v.number()))
                    .collect(),
            },
        );
    }
    for service in pool.services() {
        let file = service.parent_file().name().to_owned();
        if well_known(&file) || service.full_name().starts_with("grpc.reflection.") {
            continue;
        }
        let methods = service
            .methods()
            .map(|m| {
                (
                    m.name().to_owned(),
                    ProtoMethod {
                        input: m.input().full_name().to_owned(),
                        output: m.output().full_name().to_owned(),
                        client_streaming: m.is_client_streaming(),
                        server_streaming: m.is_server_streaming(),
                    },
                )
            })
            .collect();
        schema.services.insert(
            service.full_name().to_owned(),
            ProtoService { file, methods },
        );
    }
    schema
}
