// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging rules of OData (wave 18, strand J): `@odata.nextLink` (V4) and `d.__next` (V2) as
//! data in the manifest (`Contributions.paging`), so the core follows pages without knowing
//! OData. The core evaluates them like any rule; one of the user (a companion file or the
//! collection default) always wins over them.
//!
//! What the server sends is the finished link, with `$skiptoken` or `$top`/`$skip` in it: the
//! core follows it as it is. `__count` and `@odata.count` are not read here (the rule format
//! has no place for a total). A V2 answer for a single entity (`$.d` without `results`) has no
//! `__next` and so ends after the first page, which is right.
//!
//! The rules are told apart by the service path, because a rule applies to the first match
//! only: the usual SAP and CAP layouts (`/sap/opu/odata4/`, `/odata/v4/` for V4,
//! `/sap/opu/odata/`, `/odata/v2/` for V2). A service under any other path has no rule; the
//! user adds one for the request.

use rt_core::paging::{PagingRule, PagingSpec};
use rt_plugin_sdk::api::{Contributions, PagingContribution, PagingMatch};

/// Id of the V4 rules (one per path layout; the id carries the layout).
pub const V4_IDS: [&str; 2] = ["odata-v4-sap", "odata-v4-cap"];
/// Id of the V2 rules.
pub const V2_IDS: [&str; 2] = ["odata-v2-sap", "odata-v2-cap"];

fn rule(id: &str, pattern: &str, next: &str, merge: &str) -> PagingContribution {
    PagingContribution {
        id: id.into(),
        matches: PagingMatch {
            hosts: Vec::new(),
            content_types: vec!["application/json".into()],
            url_pattern: Some(pattern.into()),
        },
        rule: PagingSpec {
            rule: PagingRule::JsonUrl { next: next.into() },
            merge: Some(merge.into()),
        },
    }
}

/// What this module adds to the manifest (the paging rules). Keep it in step with
/// `plugin.json`.
pub fn contributions() -> Contributions {
    let v4 = |id, pattern| rule(id, pattern, "$['@odata.nextLink']", "$.value");
    let v2 = |id, pattern| rule(id, pattern, "$.d.__next", "$.d.results");
    Contributions {
        paging: vec![
            v4(V4_IDS[0], "*/odata4/*"),
            v4(V4_IDS[1], "*/odata/v4/*"),
            v2(V2_IDS[0], "*/sap/opu/odata/*"),
            v2(V2_IDS[1], "*/odata/v2/*"),
        ],
        ..Contributions::default()
    }
}
