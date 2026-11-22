// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OpenCollection items that are not plain HTTP (wave 8): `graphql` (strand B), `grpc`
//! (strand B), `websocket` (strand A). Each module reads its item type into a
//! [`RequestSpec`](rt_core::model::RequestSpec) and writes changes back with the same rule as
//! HTTP items: unchanged parts stay byte-identical, and the edited file must read back as the
//! request (otherwise the edit is refused).

pub(crate) mod graphql;
pub(crate) mod grpc;
pub(crate) mod websocket;

use rt_core::model::{Body, Protocol, RequestEntry, RequestSpec};

use crate::read::Builder;
use crate::yaml::Document;

type Result<T> = std::result::Result<T, String>;

/// The item type a spec is written as: `http`, `graphql`, `grpc` or `websocket`.
pub(crate) fn item_type(spec: &RequestSpec) -> &'static str {
    match (&spec.protocol, &spec.body) {
        (Protocol::Grpc(_), _) => "grpc",
        (Protocol::WebSocket(_), _) => "websocket",
        (Protocol::Http, Body::GraphQl { .. }) => "graphql",
        (Protocol::Http, _) => "http",
    }
}

/// Reads a non-HTTP item; `None` for item types no module reads (`script`, `app`) or an
/// item a module cannot read (it records an issue on the builder).
pub(crate) fn read(builder: &mut Builder<'_>, path: &str, kind: &str) -> Option<RequestEntry> {
    match kind {
        "graphql" => graphql::read(builder, path),
        "grpc" => grpc::read(builder, path),
        "websocket" | "ws" => websocket::read(builder, path),
        _ => None,
    }
}

/// Text of a new file for a non-HTTP spec; `None` for HTTP.
pub(crate) fn new_text(spec: &RequestSpec) -> Option<String> {
    match item_type(spec) {
        "graphql" => Some(graphql::new_text(spec)),
        "grpc" => Some(grpc::new_text(spec)),
        "websocket" => Some(websocket::new_text(spec)),
        _ => None,
    }
}

/// Converts the file between `http` and `graphql` when `spec`'s body says the other type
/// (strand B); `None` when the file already has the right type.
pub(crate) fn convert(doc: &Document, spec: &RequestSpec) -> Option<Result<Document>> {
    graphql::convert(doc, item_type(spec))
}

/// Edits a non-HTTP item; `None` when `spec` is HTTP and the file is an HTTP item.
pub(crate) fn apply(doc: &Document, path: &str, spec: &RequestSpec) -> Option<Result<Document>> {
    match item_type(spec) {
        "graphql" => Some(graphql::apply(doc, path, spec)),
        "grpc" => Some(grpc::apply(doc, path, spec)),
        "websocket" => Some(websocket::apply(doc, path, spec)),
        _ => None,
    }
}
