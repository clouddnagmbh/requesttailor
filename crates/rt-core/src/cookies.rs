// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Cookies (wave 5, strand D): the RFC 6265 rules the shells' `CookieJar`s share (domain and
//! path matching, expiry, `Secure`, `HttpOnly`), plus the two steps `send_with` calls.
//!
//! Interface commit: the two steps in their simplest form. Strand D owns this file.

use crate::transport::ResponseHead;

/// Adds the jar's cookies to the request. A `Cookie` header the user wrote wins for the
/// names it sets.
pub fn add_cookie_header(headers: &mut Vec<(String, String)>, cookie: &str) {
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cookie"))
    {
        headers.push(("Cookie".to_owned(), cookie.to_owned()));
    }
}

/// The `Set-Cookie` header values of a response.
pub fn set_cookie_values(head: &ResponseHead) -> Vec<String> {
    head.headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, value)| value.clone())
        .collect()
}
