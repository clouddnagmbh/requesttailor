// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Minimal failing request, also called "shrink" (wave 19, strand E): `Backend.shrink`,
//! `rt shrink`. A failing request (the [`Predicate`]) is cut down to the smallest set of
//! headers, query parameters, cookies and body fields that still fails.
//!
//! Method: delta debugging (ddmin) over an ordered list of *parts*: headers (except `Host`,
//! `Content-Length`, authentication headers, which stay unless the user says otherwise,
//! and anything in `keep`), query parameters, the JSON body as a tree (fields and array
//! elements down to depth 8), form fields, cookies. Each probe sends **one** request (guard,
//! production question, rate tracker) and hits if the predicate holds. A final 1-minimal
//! check removes each remaining part alone. Only parts the request already has are removed;
//! no value is invented. Simplifying values (shorten long strings, numbers to 0/1) is a
//! second, explicitly switched-on pass.
//!
//! Limits: [`DEFAULT_PROBES`] probes (hard [`MAX_PROBES`]), [`DEFAULT_SECONDS`] seconds
//! (hard [`MAX_SECONDS`]), [`MAX_PARTS`] parts. Writing requests only after a confirmation
//! ("This sends up to N requests that write."), never against production. A hit is
//! confirmed once; a predicate that flips ends with `flaky`. Output: the shrunk request as
//! an unsaved draft, "Copy as curl" and "Copy link" through `rt_app::share` (no secret
//! values), and what was removed (paths, no values).

use serde::{Deserialize, Serialize};

mod engine;
mod json;
mod parts;
mod predicate;

pub use engine::shrink;
pub use predicate::{describe as describe_failure, failure_of, holds};

use rt_core::model::RequestSpec;

pub const DEFAULT_PROBES: u32 = 100;
pub const MAX_PROBES: u32 = 500;
pub const DEFAULT_SECONDS: u32 = 120;
pub const MAX_SECONDS: u32 = 600;
pub const MAX_PARTS: usize = 2_000;

/// What counts as "still fails".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Predicate {
    /// The status is exactly `status`.
    Status {
        status: u16,
    },
    /// The status is 5xx.
    ServerError,
    /// An assertion of the collection fails (`assertion`: its index or name; `None`: any).
    AssertionFails {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assertion: Option<String>,
    },
    /// The response body matches the regular expression.
    BodyMatches {
        pattern: String,
    },
    /// A response header is present / missing.
    HeaderPresent {
        name: String,
    },
    HeaderMissing {
        name: String,
    },
    /// The failure of the last response (its status and the failed assertion): the default.
    LastFailure,
}

/// What to shrink (`ShrinkApi.run`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShrinkRequest {
    /// The request in the active collection, as edited in memory.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub predicate: Predicate,
    /// Parts that must stay: `header:Authorization`, `query:key`, `body:$.id`.
    #[serde(default)]
    pub keep: Vec<String>,
    /// `0`: [`DEFAULT_PROBES`].
    #[serde(default)]
    pub max_probes: u32,
    /// `0`: [`DEFAULT_SECONDS`].
    #[serde(default)]
    pub max_seconds: u32,
    /// The second pass over values.
    #[serde(default)]
    pub simplify_values: bool,
    /// The user confirmed sending a writing request up to `max_probes` times (never
    /// against production, whatever this says).
    #[serde(default)]
    pub confirmed_writes: bool,
}

/// One part that was removed, by path, never by value (`header:X-Debug`, `body:$.items[3]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShrinkStep {
    pub part: String,
    /// `header`, `query`, `cookie`, `body`, `form`; or `value` for the second pass.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShrinkResult {
    /// The smallest request found (the original if nothing could go).
    pub request: RequestSpec,
    pub removed: Vec<ShrinkStep>,
    pub probes: u32,
    /// The failure did not reproduce at all.
    #[serde(default)]
    pub not_reproduced: bool,
    /// The predicate flipped between hit and miss ("flaky, result may not be minimal").
    #[serde(default)]
    pub flaky: bool,
    /// Every remaining part was checked alone.
    #[serde(default)]
    pub one_minimal: bool,
    /// A limit (probes, time) ended the search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<String>,
}
