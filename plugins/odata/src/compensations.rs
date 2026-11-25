// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Clean-up rules of OData (wave 19, strand H) as data in the manifest
//! (`Contributions.compensations`), so the app can put back what a writing request changed
//! without the core knowing OData. No WASM call per response; the app executes the rules
//! (guard, production question, conflict check, audit), the plugin only supplies data.
//!
//! What the rules say:
//!
//! - **Created entity** (`POST`): the entity is named by the `Location` header (V2 and V4),
//!   `OData-EntityId` (V4, also on `204`), `d.__metadata.uri` / `d.__metadata.id` (V2 body)
//!   or `@odata.id` (V4 body). The first four are absolute URLs with the key in them, quoted
//!   and composite as the server wrote it (`SalesOrderItemSet(SalesOrderID='1',ItemPosition='10')`,
//!   `Books(ID=7,IsActiveEntity=true)`), so the undoing `DELETE` goes to exactly that URL and
//!   no key literal is built by the app. `@odata.id` comes last: services answer it relative
//!   to the service root, which the data cannot express (see `API-WUENSCHE.md`).
//! - **Version** (`PATCH`, `PUT`, `MERGE`, `DELETE`): the ETag is the response header `ETag`
//!   or `d.__metadata.etag` (V2) / `@odata.etag` (V4); it becomes `If-Match` of the undoing
//!   request.
//! - **CSRF**: every writing call needs the token: `GET` with `x-csrf-token: fetch`, read
//!   `x-csrf-token` of the answer, send it back as `x-csrf-token`.
//! - **Drafts** are not reversible: a draft instance (`IsActiveEntity=false`) is edited or
//!   discarded in the draft's own life cycle, and `Edit`, `Prepare`, `Activate` change which
//!   instance exists, which a later write cannot put back. The reason says so.
//!
//! The rules are told apart by the service path, like the paging rules: the usual SAP and CAP
//! layouts (`/sap/opu/odata4/`, `/odata/v4/` for V4, `/sap/opu/odata/`, `/odata/v2/` for V2).
//! A service under another path has no rule; the user reverts such an entry by hand.

use rt_plugin_sdk::api::{
    CompensationContribution, CompensationMatch, CompensationRule, CompensationToken,
    Contributions, NotReversibleRule,
};

/// Ids of the rules, one per path layout.
pub const IDS: [&str; 4] = [
    "odata-v4-sap",
    "odata-v4-cap",
    "odata-v2-sap",
    "odata-v2-cap",
];

/// Why a draft is not put back.
pub const DRAFT_INSTANCE_REASON: &str = "A draft instance lives by the draft life cycle: discard or activate it in the app, a \
     later write cannot put it back.";
/// Why a draft action is not put back.
pub const DRAFT_ACTION_REASON: &str = "A draft action (Edit, Prepare, Activate) changes which instance exists; there is no \
     request that undoes it.";

#[derive(Clone, Copy, PartialEq)]
enum V {
    V2,
    V4,
}

fn not_reversible(version: V) -> Vec<NotReversibleRule> {
    let rule = |p: &str, reason: &str| NotReversibleRule {
        url_pattern: p.into(),
        reason: reason.into(),
    };
    let actions: Vec<&str> = match version {
        // RAP bound actions (namespace-qualified or not) and CAP `draft*` actions.
        V::V4 => vec![
            "*draftEdit*",
            "*draftPrepare*",
            "*draftActivate*",
            "*.Edit",
            "*.Edit?*",
            "*.Prepare",
            "*.Prepare?*",
            "*.Activate",
            "*.Activate?*",
            "*/Edit",
            "*/Edit?*",
            "*/Prepare",
            "*/Prepare?*",
            "*/Activate",
            "*/Activate?*",
        ],
        // SAP Gateway draft function imports.
        V::V2 => vec![
            "*EditAction*",
            "*PreparationAction*",
            "*ActivationAction*",
            "*ValidationAction*",
            "*draftEdit*",
            "*draftPrepare*",
            "*draftActivate*",
        ],
    };
    // The first match counts: actions before instances (an action on a draft names both).
    let mut out: Vec<_> = actions
        .into_iter()
        .map(|p| rule(p, DRAFT_ACTION_REASON))
        .collect();
    // Draft instances: V2 and V4 name them with the key `IsActiveEntity=false`.
    out.push(rule("*IsActiveEntity=false*", DRAFT_INSTANCE_REASON));
    out
}

fn rule(id: &str, pattern: &str, version: V) -> CompensationContribution {
    let v = |xs: &[&str]| xs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let (id_from, etag_from) = match version {
        V::V2 => (
            v(&["header:Location", "$.d.__metadata.uri", "$.d.__metadata.id"]),
            v(&["header:ETag", "$.d.__metadata.etag"]),
        ),
        V::V4 => (
            v(&["header:Location", "header:OData-EntityId", "$['@odata.id']"]),
            v(&["header:ETag", "$['@odata.etag']"]),
        ),
    };
    CompensationContribution {
        id: id.into(),
        matches: CompensationMatch {
            url_pattern: Some(pattern.into()),
            ..CompensationMatch::default()
        },
        rule: CompensationRule {
            id_from,
            // The id is the entity's own URL.
            resource_url: None,
            etag_from,
            token: Some(CompensationToken {
                fetch_header: "x-csrf-token".into(),
                fetch_value: "fetch".into(),
                read_header: "x-csrf-token".into(),
                send_header: "x-csrf-token".into(),
            }),
            not_reversible: not_reversible(version),
        },
    }
}

/// What this module adds to the manifest (the compensation rules). Keep it in step with
/// `plugin.json`.
pub fn contributions() -> Contributions {
    Contributions {
        compensations: vec![
            rule(IDS[0], "*/odata4/*", V::V4),
            rule(IDS[1], "*/odata/v4/*", V::V4),
            rule(IDS[2], "*/sap/opu/odata/*", V::V2),
            rule(IDS[3], "*/odata/v2/*", V::V2),
        ],
        ..Contributions::default()
    }
}
