// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand B: the query builder (request tab `query`). Reads the request URL, finds the
//! service root and entity set ([`crate::service::split`], [`crate::metadata::cached`]),
//! offers `$filter`, `$expand`, `$select`, `$orderby`, `$top`, `$skip`, `$count` (V4) /
//! `$inlinecount` (V2) from the model and writes the URL back (`ViewReply::update_request`).
//!
//! Interface: [`Query`], [`Query::parse`], [`Query::to_query_string`].
//!
//! Submodules: [`encode`] (templates, splitting, percent-encoding), [`literal`] (EDM literals
//! per version and the filter builder's expressions), [`url`] (rewriting the request URL),
//! and the tab itself (`tab`).

pub mod encode;
pub mod literal;
mod tab;
pub mod url;

pub use tab::{action, render};

use crate::edm::Version;
use encode::{decode, encode, escape_raw, find_outside, split_list, split_outside};

/// The system query options of one request, in the order they are written.
///
/// Values of system options are percent-decoded (`$filter=Name%20eq%20'x'` gives
/// `Name eq 'x'`); [`Query::to_query_string`] encodes them again. `{{templates}}` are opaque
/// in both directions. `$expand` items are kept as written, nested V4 options included
/// (`author($select=name)`).
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
    /// Other parameters (custom ones, `sap-client`, `$format`, `$search`, …), kept as is:
    /// name and value exactly as written in the URL (not decoded), in their order. A system
    /// option that does not fit its field (`$top={{top}}`, a second `$filter`, `$count=false`)
    /// is kept here too.
    pub other: Vec<(String, String)>,
}

/// The system options the builder knows, as they are written (lowercase).
const SYSTEM: &[&str] = &[
    "$filter", "$select", "$expand", "$orderby", "$top", "$skip", "$count",
];

impl Query {
    /// Reads a query string (without `?`).
    pub fn parse(query: &str) -> Query {
        let mut q = Query::default();
        for raw in split_outside(query, '&') {
            if raw.is_empty() {
                continue;
            }
            let (raw_name, raw_value) = split_pair(raw);
            if !q.take(&decode(raw_name), raw_value) {
                q.other.push((raw_name.to_owned(), raw_value.to_owned()));
            }
        }
        q
    }

    /// Puts one system option into its field; `false` if it does not fit (then it goes to
    /// `other`).
    fn take(&mut self, name: &str, raw_value: &str) -> bool {
        let value = decode(raw_value);
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "$filter" if self.filter.is_none() && !value.is_empty() => {
                self.filter = Some(value);
            }
            "$select" if self.select.is_empty() && !value.trim().is_empty() => {
                self.select = list(&value);
            }
            "$expand" if self.expand.is_empty() && !value.trim().is_empty() => {
                self.expand = list(&value);
            }
            "$orderby" if self.orderby.is_empty() && !value.trim().is_empty() => {
                self.orderby = list(&value).iter().map(|i| order_item(i)).collect();
            }
            "$top" if self.top.is_none() => match value.trim().parse() {
                Ok(n) => self.top = Some(n),
                Err(_) => return false,
            },
            "$skip" if self.skip.is_none() => match value.trim().parse() {
                Ok(n) => self.skip = Some(n),
                Err(_) => return false,
            },
            "$count" if !self.count && value.eq_ignore_ascii_case("true") => self.count = true,
            "$inlinecount" if !self.count && value.eq_ignore_ascii_case("allpages") => {
                self.count = true;
            }
            _ => return false,
        }
        true
    }

    /// Writes the query string (without `?`); V2 uses `$inlinecount=allpages` for `count`.
    /// System options come first, in the order `$filter`, `$select`, `$expand`, `$orderby`,
    /// `$top`, `$skip`, `$count`, then `other` as written.
    pub fn to_query_string(&self, version: Version) -> String {
        self.pieces(version).join("&")
    }

    /// The `name=value` pieces of [`Query::to_query_string`].
    pub fn pieces(&self, version: Version) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(filter) = &self.filter {
            out.push(format!("$filter={}", encode(filter)));
        }
        if !self.select.is_empty() {
            out.push(format!("$select={}", encode(&self.select.join(","))));
        }
        if !self.expand.is_empty() {
            out.push(format!("$expand={}", encode(&self.expand.join(","))));
        }
        if !self.orderby.is_empty() {
            let items: Vec<String> = self
                .orderby
                .iter()
                .map(|(p, desc)| {
                    if *desc {
                        format!("{p} desc")
                    } else {
                        p.clone()
                    }
                })
                .collect();
            out.push(format!("$orderby={}", encode(&items.join(","))));
        }
        if let Some(top) = self.top {
            out.push(format!("$top={top}"));
        }
        if let Some(skip) = self.skip {
            out.push(format!("$skip={skip}"));
        }
        if self.count {
            out.push(match version {
                Version::V2 => "$inlinecount=allpages".to_owned(),
                Version::V4 => "$count=true".to_owned(),
            });
        }
        for (name, value) in &self.other {
            let name = escape_raw(name, &['=']);
            out.push(if value.is_empty() {
                name
            } else {
                format!("{name}={}", escape_raw(value, &[]))
            });
        }
        out
    }
}

/// `name=value` as written; no `=` gives an empty value.
fn split_pair(raw: &str) -> (&str, &str) {
    match find_outside(raw, '=') {
        Some(i) => (&raw[..i], &raw[i + 1..]),
        None => (raw, ""),
    }
}

/// Which option a `name=value` piece sets: the system options by their lowercase name
/// (`$inlinecount` is `$count`), anything else by its exact decoded name.
pub(crate) fn slot(raw: &str) -> String {
    let name = decode(split_pair(raw).0);
    let lower = name.to_ascii_lowercase();
    if lower == "$inlinecount" {
        "$count".to_owned()
    } else if SYSTEM.contains(&lower.as_str()) {
        lower
    } else {
        format!("other:{name}")
    }
}

fn list(value: &str) -> Vec<String> {
    split_list(value)
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `name desc` / `name asc` / `name`.
fn order_item(item: &str) -> (String, bool) {
    if let Some((property, direction)) = item.trim().rsplit_once(char::is_whitespace) {
        if direction.eq_ignore_ascii_case("desc") {
            return (property.trim().to_owned(), true);
        }
        if direction.eq_ignore_ascii_case("asc") {
            return (property.trim().to_owned(), false);
        }
    }
    (item.trim().to_owned(), false)
}

/// Parses `$orderby` text as typed in a form (`a desc,b`).
pub fn parse_orderby(text: &str) -> Vec<(String, bool)> {
    list(text).iter().map(|i| order_item(i)).collect()
}

/// Parses a `$select`/`$expand` list as typed in a form.
pub fn parse_list(text: &str) -> Vec<String> {
    list(text)
}
