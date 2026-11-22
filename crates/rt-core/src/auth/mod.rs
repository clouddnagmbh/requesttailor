// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Auth that needs more than a header from the file: OAuth 2.0 (network, cache, prompts)
//! and AWS SigV4 (signs the final request). Called by `crate::send::send_with`; the simple
//! schemes (basic, bearer, API key) stay in `crate::prepare`.

pub mod oauth2;
pub mod sigv4;
