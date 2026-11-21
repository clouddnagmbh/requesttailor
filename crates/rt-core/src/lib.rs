// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-core`: HTTP engine, variables, auth and runner.
//!
//! Module owners in wave 2: `model` (interface commit), `transport` (strand A),
//! `vars` and `prepare` (strand C), `send` (strand D).

pub mod model;
pub mod prepare;
pub mod send;
pub mod serde_ms;
pub mod transport;
pub mod vars;
