// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand B: the query builder (request tab `query`). Reads the request URL, finds the
//! service root and entity set ([`crate::service::split`], [`crate::metadata::cached`]),
//! offers `$filter`, `$expand`, `$select`, `$orderby`, `$top`, `$skip`, `$count` (V4) /
//! `$inlinecount` (V2) from the model and writes the URL back (`ViewReply::update_request`).
//!
//! Interface: [`Query`], [`Query::parse`], [`Query::to_query_string`].

use rt_plugin_sdk::api::ViewReply;
use rt_plugin_sdk::{Action, Result, Tab};

use crate::edm::Version;

/// The system query options of one request, in the order they are written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub filter: Option<String>,
    pub expand: Vec<String>,
    pub select: Vec<String>,
    /// `(property, descending)`.
    pub orderby: Vec<(String, bool)>,
    pub top: Option<u64>,
    pub skip: Option<u64>,
    pub count: bool,
    /// Other parameters (custom ones, `sap-client`, `$format`, `$search`, …), kept as is.
    pub other: Vec<(String, String)>,
}

impl Query {
    /// Reads a query string (without `?`).
    pub fn parse(query: &str) -> Query {
        let _ = query;
        Query::default()
    }

    /// Writes the query string (without `?`); V2 uses `$inlinecount=allpages` for `count`.
    pub fn to_query_string(&self, version: Version) -> String {
        let _ = version;
        String::new()
    }
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let _ = tab;
    Err(crate::todo("the query builder"))
}

pub fn action(action: Action) -> Result<ViewReply> {
    let _ = action;
    Err(crate::todo("the query builder"))
}
