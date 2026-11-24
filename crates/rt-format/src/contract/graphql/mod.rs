// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Breaking changes of a GraphQL schema between two versions (wave 18, strand H). Interface:
//! [`changes`] says "not implemented yet" until the strand fills this directory in.

use crate::FileSet;

use super::{ContractError, SchemaSources, SpecChanges};

/// Changes from `old` to `new`; see [`super::schema_changes`].
pub fn changes(
    old: Option<&SchemaSources>,
    new: Option<&SchemaSources>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let _ = (old, new, files);
    Err(ContractError::Unsupported(
        "GraphQL schema changes are not implemented yet (wave 18, strand H).".into(),
    ))
}
