// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Breaking changes of Protobuf files between two versions (wave 18, strand H).
//!
//! The files are the `.proto` files of `config.protobuf` (and the `protoFilePath` of gRPC
//! requests) with the files they import, compiled by the same pure-Rust compiler the gRPC
//! requests use. Nothing is fetched from a server.
//!
//! Rules, from the point of view of a client or server that worked with the old files:
//!
//! - Breaking: a field number changed (the name now has another number), or a number reused
//!   by a field of another type; a field, message, enum, enum value, `rpc` or service removed
//!   (the message says whether a removed field's number is `reserved`); a field type changed
//!   unless the pair is wire-compatible (`int32` and `int64`, `string` and `bytes`, `sint32`
//!   and `sint64`, ... which is listed as a hint); `repeated`, `optional` or `required`
//!   switched; a `oneof` member moved; the streaming kind or the messages of an `rpc`
//!   changed; the package changed (the path of every call changes).
//! - Not breaking: additions, a renamed field with the same number and type (a hint: the JSON
//!   mapping uses the name), a renamed enum value (the same), new `reserved` statements.
//!
//! Each change names the gRPC requests of the collection it affects: the calls of a changed
//! `rpc` or service, and the calls whose request or response reaches a changed message or enum.

mod diff;
mod load;

use crate::FileSet;

use super::{ContractError, SchemaSources, SpecChanges};

pub(super) use load::imports_of;

/// Changes from `old` to `new`; see [`super::schema_changes`].
pub fn changes(
    old: Option<&SchemaSources>,
    new: Option<&SchemaSources>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let (o, n) = (load::schema(old)?, load::schema(new)?);
    let mut found = diff::diff(&o, &n);
    if found
        .iter()
        .any(|c| !matches!(c.target, diff::Target::None))
    {
        let calls = diff::calls(files)?;
        diff::assign(&mut found, &calls, &o, &n);
    }
    Ok(SpecChanges {
        from_version: None,
        to_version: None,
        changes: found.into_iter().map(|c| c.change).collect(),
    })
}
