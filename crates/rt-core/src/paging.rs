// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging rules (wave 18): data only, shared by the app (`rt_app::paging`), the companion file
//! `<request>.paging.json`, the collection default and a plugin's `contributes.paging`
//! (`rt_plugin::api::PagingContribution`). The core knows no vendor: `@odata.nextLink` or
//! `d.__next` are `json-url` rules a plugin ships as data.
//!
//! Following pages is bounded (`rt_app::paging::PagingLimits`) and stays on the origin of the
//! request; see `CLAUDE.md`.

use serde::{Deserialize, Serialize};

/// How the next page of a response is found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PagingRule {
    /// RFC 8288 `Link: <…>; rel="next"`.
    LinkHeader,
    /// A JSONPath (RFC 9535) to an absolute or relative URL in the body.
    JsonUrl { next: String },
    /// A JSONPath to a cursor value that is sent back as a query parameter, body field or
    /// header.
    Cursor { next: String, param: CursorParam },
}

/// Where a [`PagingRule::Cursor`] value goes in the next request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorParam {
    pub place: CursorPlace,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorPlace {
    Query,
    Body,
    Header,
}

/// A rule with how the pages are put together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagingSpec {
    pub rule: PagingRule,
    /// JSONPath of the array whose items of all pages are appended to the array of the first
    /// page. `None`: the pages are delivered one after the other, not merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<String>,
}
