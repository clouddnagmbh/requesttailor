// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 5 file mappings, one module per owner, so the strands do not share files:
//! - `oauth2`: `type: oauth2` and the collection auth in `extensions` (strand C).
//! - `awsv4`: `type: awsv4` (strand D).
//! - `certs`: `clientCertificates` in `config` and environments (strand D).
//! - `secrets`: `externalSecrets` of environments (strand E).
//!
//! Readers are complete where the interface commit needs the model filled; writers that
//! say "not yet" leave the file unchanged, and the read-back check of every edit then
//! refuses a change they cannot write.

pub(crate) mod awsv4;
pub(crate) mod certs;
pub(crate) mod oauth2;
pub(crate) mod secrets;
