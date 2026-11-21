// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The internal request model. Format-neutral: `rt-format` maps files to these types, and
//! everything else (variables, transport, CLI, WASM, UI) works only on them.
//!
//! Changes here are interface changes (working rule 1): they go in their own commit.

mod collection;
mod env;
mod request;

pub use collection::{Collection, Folder, Item, RequestContext, RequestDefaults, RequestEntry};
pub use env::{Environment, Variable};
pub use request::{
    ApiKeyPlacement, Auth, Body, KeyValue, MultipartPart, Param, ParamKind, PartKind, RawKind,
    RequestSpec, Settings,
};
