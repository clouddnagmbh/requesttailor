// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Breaking changes of a GraphQL schema between two versions (wave 18, strand H).
//!
//! The schema is SDL: a file named in `extensions.requesttailor.contract.graphql`, else
//! `schema.graphql`, `schema.gql` or `schema.graphqls` next to the collection, then at the
//! repository root (see [`super::schema_wanted`]). It comes from an introspection (`sdl`) or is
//! kept by hand. Nothing is fetched from a server.
//!
//! Rules, from the point of view of a client that worked against the old schema:
//!
//! - Breaking: a type, field, argument, enum value, directive, interface or union member
//!   removed; a type of a field or argument changed (also a list against a single value); an
//!   output type that may now be null, an input that is now required (a new required argument or
//!   input field, an argument that became non-null); a type that changed its kind.
//! - Not breaking: additions, optional arguments and input fields, an output that became
//!   non-null, an input that became optional, descriptions, `@deprecated` (listed as hints).
//!
//! Each change names the requests of the collection that use the part (`SpecChange.requests`):
//! the query of a GraphQL request is walked through the schema.

mod diff;
mod model;
mod syntax;
mod usage;

use crate::FileSet;

use super::{ContractError, SchemaSources, SpecChanges};

/// Changes from `old` to `new`; see [`super::schema_changes`].
pub fn changes(
    old: Option<&SchemaSources>,
    new: Option<&SchemaSources>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let (o, n) = (model::load(old)?, model::load(new)?);
    let mut found = diff::diff(&o, &n);
    if !found.is_empty()
        && found
            .iter()
            .any(|c| !matches!(c.target, diff::Target::None))
    {
        let used = usage::requests(files, &o, &n)?;
        usage::assign(&mut found, &used, &o, &n);
    }
    Ok(SpecChanges {
        from_version: None,
        to_version: None,
        changes: found.into_iter().map(|c| c.change).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::syntax::parse;

    #[test]
    fn a_query_reads() {
        let doc = parse(b"query Q($a: [ID!]! = [1]) @x { a: b(c: {d: $a}) @y { ...F ... on T { e } } } fragment F on T { g }").unwrap();
        assert_eq!(doc.operations.len(), 1);
        assert_eq!(doc.fragments.len(), 1);
    }
}
