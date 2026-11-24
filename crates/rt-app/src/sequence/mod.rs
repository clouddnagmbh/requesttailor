// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sequence diagrams from runs (wave 19, strand G): `Backend.sequence`, `--sequence <file>`
//! of `rt run`, `rt send` and `rt mcp`. A [`SequenceModel`] has participants and messages and
//! is rendered as Mermaid text (`sequenceDiagram`), as a Markdown block and, in the app, as
//! an own SVG view (no library, deterministic, virtualised, keyboard).
//!
//! **Participants:** `Client` (the collection or app or script), one server per host
//! (`api.example.com`), optionally `Script` and `Plugin`, and `Agent` for MCP runs.
//! **Messages:** a request (method, path **without query**, or with the masked query on
//! request) and its response (status, duration). **Notes:** failed assertions and tests,
//! rate waits, page sequence (`page 3/20`), error kinds, flows (a node name as a `rect`,
//! `loop` or `alt` group when the source event carries one), MCP runs (tool name, rule,
//! result from the audit log; values only from masked reports).
//!
//! **Values:** off by default. With `include_values` short excerpts (≤ 40 characters) of path
//! parameters, body fields and response fields appear, passed through the same rules as
//! sharing (mask of all secret values, scanner; no header values except `Content-Type` and
//! `Location`); the view warns that values may contain personal data.
//!
//! **Sources:** [`Recorder`] is an `ExchangeObserver` (anything that sends through
//! `send_with` with `Services.observers`: the runner, `rt run`, `rt mcp`, later flows), plus
//! `RunEvent`s, streams (a loop) and the history (metadata only). Limits: [`MAX_MESSAGES`]
//! messages per diagram (more as a "n more" note), [`MAX_MESSAGES_PER_SWITCH`] per
//! participant switch, text at most 1 MiB. Escaping for `;`, `#`, `:`, newlines and labels
//! over [`MAX_LABEL`] characters is the renderer's job. Interface: types only.

use serde::{Deserialize, Serialize};

pub const MAX_MESSAGES: usize = 2_000;
pub const MAX_MESSAGES_PER_SWITCH: usize = 500;
pub const MAX_LABEL: usize = 80;
pub const MAX_TEXT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParticipantKind {
    Client,
    Server,
    Script,
    Plugin,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    /// Stable id inside the model (`client`, `api.example.com`).
    pub id: String,
    pub label: String,
    pub kind: ParticipantKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    Request,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub kind: MessageKind,
    /// A participant id.
    pub from: String,
    pub to: String,
    /// `GET /orders` or `200 (41 ms)`; never longer than [`MAX_LABEL`].
    pub label: String,
    /// The request this message belongs to, to open it (`Enter`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,
}

/// What a note hangs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NoteAnchor {
    /// Over one participant.
    Over { participant: String },
    /// Over two participants.
    Between { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Index into `messages` after which the note appears.
    pub after: usize,
    pub anchor: NoteAnchor,
    pub text: String,
}

/// A group of messages (`rect`, `loop`, `alt`) from a flow node or a page loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub kind: String,
    pub label: String,
    /// The first and last message index.
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SequenceModel {
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub groups: Vec<Group>,
    /// Messages left out because of [`MAX_MESSAGES`].
    #[serde(default)]
    pub omitted: u32,
}

/// Where a model comes from (`SequenceApi.model`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SequenceSource {
    /// A run of the runner (its id as the app holds it).
    Run { run_id: u32 },
    /// What the recorder saw since the app started or the last `clear`.
    Session,
    /// The pages of one paged send.
    Pages { request_path: String },
    /// MCP runs from the audit log (`agents/audit.jsonl`), values only from masked reports.
    Mcp { since: Option<i64> },
    /// Entries of the history (metadata only).
    History { ids: Vec<u32> },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SequenceOptions {
    /// The masked query on request lines.
    #[serde(default)]
    pub include_query: bool,
    /// Short masked excerpts of values ("values may contain personal data").
    #[serde(default)]
    pub include_values: bool,
}

/// What `SequenceApi.render` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceText {
    /// Mermaid `sequenceDiagram` text.
    pub mermaid: String,
    /// The same as a fenced ```` ```mermaid ```` block for a PR description.
    pub markdown: String,
}

/// An `ExchangeObserver` that builds a [`SequenceModel`] from every exchange it sees. The
/// interface commit has the type only; strand G adds the model building.
#[derive(Debug, Default)]
pub struct Recorder {
    _private: (),
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }
}

impl rt_core::observe::ExchangeObserver for Recorder {
    fn observe(&self, exchange: &rt_core::observe::Exchange) {
        let _ = exchange;
    }
}
