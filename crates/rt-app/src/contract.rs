// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI contract for the app (wave 12): finding the spec, and the shared steps of the
//! contract check, breaking changes and `rt changelog` that desktop, web and CLI run the same
//! way on top of [`rt_format::contract`].
//!
//! Owner: wave 12 interface; implementation strand F.

pub use rt_format::contract::{
    ContractError, ContractFinding, ContractReport, ContractSpec, FindingKind, SPEC_CANDIDATES,
    SpecChange, SpecChangeKind, SpecChanges,
};

/// The spec path of the collection in `files`: configured in `opencollection.yml`, else the
/// first of [`SPEC_CANDIDATES`] for which `exists` says yes (the shell asks its work tree or
/// `GitApi.show`).
pub fn locate(files: &rt_format::FileSet, exists: impl Fn(&str) -> bool) -> Option<String> {
    rt_format::contract::configured_spec(files).or_else(|| {
        SPEC_CANDIDATES
            .iter()
            .find(|p| exists(p))
            .map(|p| (*p).to_owned())
    })
}
