// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-core`: HTTP engine, variables, auth and runner.
//!
//! Module owners in wave 2: `model` (interface commit), `transport` (strand A),
//! `vars` and `prepare` (strand C), `send` (strand D).
//!
//! Wave 5 (interface commit: `model::auth`, `services`, the `send_with` pipeline):
//! `auth::oauth2` strand C; `auth::sigv4`, `tls`, `cookies` and client certificates in the
//! native transport strand D.

pub mod assert;
pub mod auth;
pub mod cookies;
pub mod guard;
pub mod model;
pub mod observe;
pub mod prepare;
pub mod runner;
pub mod script;
pub mod send;
pub mod serde_ms;
pub mod services;
pub mod snapshot;
pub mod stream;
pub mod tls;
pub mod transport;
pub mod vars;
