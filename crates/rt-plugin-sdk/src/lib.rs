// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-plugin-sdk`: write RequestTailor plugins in Rust. Implements the binary interface of
//! `crates/rt-plugin/API.md` (exports `rt_alloc`, `rt_dealloc`, `rt_call`; import `rt.host`)
//! so a plugin only implements the hooks it contributes and calls typed host functions.
//!
//! Interface commit: re-exports the API types. Strand B (wave 5) owns this crate, the
//! TypeScript SDK in `sdk/ts`, the example plugin and the developer documentation.

pub use rt_plugin::api;
