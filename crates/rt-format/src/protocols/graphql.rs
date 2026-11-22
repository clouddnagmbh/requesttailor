// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: graphql` items: `graphql.{method, url, headers, params, body: {query, variables}, auth}` (body variants like HTTP). Wave 8, strand B.

use rt_core::model::{RequestEntry, RequestSpec};

use super::Result;
use crate::read::Builder;
use crate::yaml::Document;

/// Strand B. Until then the item is skipped, as before wave 8.
pub(crate) fn read(_builder: &mut Builder<'_>, _path: &str) -> Option<RequestEntry> {
    None
}

/// Strand B.
pub(crate) fn new_text(spec: &RequestSpec) -> String {
    format!("info:\n  name: {:?}\n  type: graphql\n", spec.name)
}

/// Strand B.
pub(crate) fn apply(_doc: &Document, _path: &str, _spec: &RequestSpec) -> Result<Document> {
    Err("graphql items can be edited from wave 8, strand B on".into())
}
