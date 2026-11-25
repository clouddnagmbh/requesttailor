// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Building a [`SequenceModel`] from recorded exchanges, the agent audit log, the history and
//! stream logs. One [`Builder`] enforces the limits ([`MAX_MESSAGES`], [`MAX_MESSAGES_PER_SWITCH`],
//! labels of [`MAX_LABEL`] characters); the sources only say what happened.
//!
//! Values: without `include_values` no value of the traffic is in the model. Path segments that
//! look like ids become `{id}`, the query is left out unless asked for, test and assertion
//! failures say how many and which test, nothing more. With `include_values` short excerpts
//! (at most [`EXCERPT`] characters) of path segments, response fields and failed assertions
//! appear, only from the already masked report, never from fields named like a secret, and
//! only if the secret scanner finds nothing in them (the sharing rules of `rt_app::share`).

use rt_core::guard::Caller;
use rt_core::stream::{Direction, StreamEvent, StreamLog};

use crate::agents::{AuditEntry, AuditOutcome};
use crate::history::HistoryEntry;

use super::record::{FailedAssertion, Record, error_kind, split_url};
use super::{
    Group, MAX_LABEL, MAX_MESSAGES, MAX_MESSAGES_PER_SWITCH, Message, MessageKind, Note,
    NoteAnchor, Participant, ParticipantKind, SequenceModel, SequenceOptions,
};

/// The longest excerpt of a value ("include values").
pub const EXCERPT: usize = 40;
const CLIENT: &str = "client";

/// Replaces control characters (newlines, tabs) by spaces and trims.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        out.push(if c.is_control() { ' ' } else { c });
    }
    out.trim().to_owned()
}

/// `text` cut to `max` characters, with an ellipsis when it was longer.
pub(crate) fn clip(text: &str, max: usize) -> String {
    let text = clean(text);
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn secret_like(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    [
        "pass",
        "secret",
        "token",
        "key",
        "auth",
        "cookie",
        "credential",
        "signature",
        "session",
        "bearer",
        "pin",
        "cvv",
        "iban",
    ]
    .iter()
    .any(|w| k.contains(w))
}

/// Whether the scanner would flag `text` (the sharing rule: such an excerpt is withheld).
fn flagged(text: &str) -> bool {
    !crate::secret_scan::scan(&[("excerpt.txt", text)]).is_empty()
}

/// An excerpt that passed the rules, or `None` (withheld).
fn safe_excerpt(text: &str) -> Option<String> {
    let text = clip(text, EXCERPT);
    (!flagged(&text) && !text.contains("***")).then_some(text)
}

fn looks_like_id(segment: &str) -> bool {
    if segment.is_empty() || segment.starts_with("{{") {
        return false;
    }
    // An address or an encoded value is data, not a name of the API.
    if segment.contains(['@', '%']) || segment.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let hex = segment
        .chars()
        .filter(|c| *c != '-')
        .all(|c| c.is_ascii_hexdigit());
    if hex && segment.len() >= 8 {
        return true;
    }
    segment.len() >= 12
        && segment.chars().any(|c| c.is_ascii_digit())
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=')
}

/// The path as the diagram shows it.
fn path_label(path: &str, query: Option<&str>, options: &SequenceOptions) -> String {
    let mut out = String::new();
    for (i, segment) in path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if options.include_values {
            out.push_str(&clip(segment, EXCERPT));
        } else if looks_like_id(segment) {
            out.push_str("{id}");
        } else {
            out.push_str(&clip(segment, EXCERPT * 2));
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    if options.include_query
        && let Some(q) = query
    {
        out.push('?');
        out.push_str(&clean(q));
    }
    out
}

/// `key=value` pairs of the scalar fields of a JSON object (a few), or `None`.
fn body_excerpt(head: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(head).ok()?;
    let object = match &value {
        serde_json::Value::Object(o) => o,
        serde_json::Value::Array(items) => {
            return Some(format!("{} items", items.len()));
        }
        _ => return None,
    };
    let mut parts = Vec::new();
    for (key, value) in object {
        if parts.len() >= 4 {
            break;
        }
        if secret_like(key) {
            continue;
        }
        let text = match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => continue,
        };
        let Some(safe) = safe_excerpt(&text) else {
            continue;
        };
        parts.push(format!("{}={}", clip(key, 24), safe));
    }
    let text = parts.join(", ");
    (!text.is_empty() && !flagged(&text)).then_some(text)
}

struct Builder {
    model: SequenceModel,
    streak: Option<String>,
    streak_len: usize,
}

impl Builder {
    fn new() -> Self {
        Self {
            model: SequenceModel::default(),
            streak: None,
            streak_len: 0,
        }
    }

    fn participant(&mut self, id: &str, label: &str, kind: ParticipantKind) {
        if self.model.participants.iter().any(|p| p.id == id) {
            return;
        }
        self.model.participants.push(Participant {
            id: id.to_owned(),
            label: clip(label, MAX_LABEL),
            kind,
        });
    }

    fn server(&mut self, host: &str) -> String {
        let id = clean(host);
        self.participant(&id, &id, ParticipantKind::Server);
        id
    }

    /// Room for `n` more messages that involve `other` (a server or agent id)?
    fn fits(&mut self, other: &str, n: usize) -> bool {
        if self.model.messages.len() + n > MAX_MESSAGES {
            return false;
        }
        let same = self.streak.as_deref() == Some(other);
        let len = if same { self.streak_len } else { 0 };
        len + n <= MAX_MESSAGES_PER_SWITCH
    }

    fn count(&mut self, other: &str, n: usize) {
        if self.streak.as_deref() == Some(other) {
            self.streak_len += n;
        } else {
            self.streak = Some(other.to_owned());
            self.streak_len = n;
        }
    }

    fn message(
        &mut self,
        kind: MessageKind,
        from: &str,
        to: &str,
        label: &str,
        path: Option<&str>,
    ) {
        self.model.messages.push(Message {
            kind,
            from: from.to_owned(),
            to: to.to_owned(),
            label: clip(label, MAX_LABEL),
            request_path: path.map(str::to_owned),
        });
    }

    /// A note after the last message.
    fn note(&mut self, anchor: NoteAnchor, text: &str) {
        let Some(after) = self.model.messages.len().checked_sub(1) else {
            return;
        };
        let text = clip(text, MAX_LABEL * 2);
        if text.is_empty() {
            return;
        }
        self.model.notes.push(Note {
            after,
            anchor,
            text,
        });
    }

    fn over(&mut self, participant: &str, text: &str) {
        self.note(
            NoteAnchor::Over {
                participant: participant.to_owned(),
            },
            text,
        );
    }

    fn finish(mut self) -> SequenceModel {
        let omitted = self.model.omitted;
        if omitted > 0 {
            self.over(CLIENT, &format!("{omitted} more messages not shown"));
        }
        self.model
    }
}

fn failed_assertion_text(a: &FailedAssertion, include_values: bool) -> String {
    let mut text = format!("{} {}", clip(&a.expression, 30), clip(&a.operator, 20));
    if include_values {
        if let Some(expected) = safe_excerpt(&a.expected) {
            text.push(' ');
            text.push_str(&expected);
        }
        if let Some(actual) = a.actual.as_deref().and_then(safe_excerpt) {
            text.push_str(&format!(", got {actual}"));
        }
    }
    text
}

pub(crate) fn model_from_records(
    records: &[&Record],
    options: &SequenceOptions,
    pages: bool,
) -> SequenceModel {
    let mut b = Builder::new();
    if records.iter().any(|r| r.caller == Caller::Agent) {
        b.participant("agent", "Agent", ParticipantKind::Agent);
    }
    b.participant(CLIENT, "Client", ParticipantKind::Client);
    let total = records.len();
    let mut first: Option<usize> = None;
    for (index, r) in records.iter().enumerate() {
        let blocked = r.error == Some("blocked");
        let server = match (&r.host, blocked) {
            (Some(host), false) => Some(b.server(host)),
            _ => None,
        };
        let other = server.clone().unwrap_or_else(|| CLIENT.to_owned());
        let agent = r.caller == Caller::Agent;
        let need = 2 + if agent { 2 } else { 0 };
        if !b.fits(&other, need) {
            b.model.omitted += need as u32;
            continue;
        }
        b.count(&other, need);
        let label = format!(
            "{} {}",
            r.method,
            path_label(&r.path, r.query.as_deref(), options)
        );
        if agent {
            b.message(
                MessageKind::Request,
                "agent",
                CLIENT,
                &format!("send_request {}", r.request_path),
                Some(&r.request_path),
            );
        }
        first.get_or_insert(b.model.messages.len());
        match &server {
            Some(server) => {
                b.message(
                    MessageKind::Request,
                    CLIENT,
                    server,
                    &label,
                    Some(&r.request_path),
                );
                let reply = match (r.status, r.error) {
                    (Some(status), _) => match r.millis {
                        Some(ms) => format!("{status} ({ms} ms)"),
                        None => status.to_string(),
                    },
                    (None, Some(kind)) => format!("error: {kind}"),
                    (None, None) => "no response".to_owned(),
                };
                b.message(
                    MessageKind::Response,
                    server,
                    CLIENT,
                    &reply,
                    Some(&r.request_path),
                );
                if pages {
                    b.over(server, &format!("page {}/{}", index + 1, total));
                }
                if options.include_values {
                    // One note per fact: each stays inside the label limit.
                    if let Some(ct) = r.content_type.as_deref().and_then(safe_excerpt) {
                        b.over(server, &format!("Content-Type: {ct}"));
                    }
                    if let Some(loc) = r.location.as_deref().and_then(safe_excerpt) {
                        b.over(server, &format!("Location: {loc}"));
                    }
                    if let Some(body) = r.body_head.as_deref().and_then(body_excerpt) {
                        b.over(server, &body);
                    }
                }
            }
            None => {
                // Nothing went out: the request never had a URL, or the guard refused it.
                let text = match (&r.rule, r.error) {
                    (Some(rule), _) => format!("{}: blocked by {}", r.request_path, clip(rule, 40)),
                    (None, Some(kind)) => format!("{}: {kind} error, not sent", r.request_path),
                    (None, None) => format!("{}: not sent", r.request_path),
                };
                b.message(
                    MessageKind::Request,
                    CLIENT,
                    CLIENT,
                    &r.request_path,
                    Some(&r.request_path),
                );
                b.message(
                    MessageKind::Response,
                    CLIENT,
                    CLIENT,
                    "not sent",
                    Some(&r.request_path),
                );
                b.over(CLIENT, &text);
            }
        }
        if r.tests_total > 0 && !r.tests_failed.is_empty() {
            let names = r
                .tests_failed
                .iter()
                .map(|n| clip(n, 30))
                .collect::<Vec<_>>()
                .join(", ");
            b.over(
                CLIENT,
                &format!(
                    "{} of {} tests failed: {names}",
                    r.tests_failed.len(),
                    r.tests_total
                ),
            );
        }
        if r.assertions_total > 0 && !r.assertions_failed.is_empty() {
            let items = r
                .assertions_failed
                .iter()
                .map(|a| failed_assertion_text(a, options.include_values))
                .collect::<Vec<_>>()
                .join("; ");
            b.over(
                CLIENT,
                &format!(
                    "{} of {} assertions failed: {items}",
                    r.assertions_failed.len(),
                    r.assertions_total
                ),
            );
        }
        if agent {
            let last = r.status.map_or_else(
                || r.error.map_or("done".to_owned(), |k| format!("error: {k}")),
                |s| s.to_string(),
            );
            b.message(
                MessageKind::Response,
                CLIENT,
                "agent",
                &last,
                Some(&r.request_path),
            );
        }
    }
    if pages && let Some(from) = first {
        let to = b.model.messages.len();
        if to > from {
            b.model.groups.push(Group {
                kind: "loop".into(),
                label: format!("{total} pages"),
                from,
                to: to - 1,
            });
        }
    }
    b.finish()
}

/// The MCP runs of the audit log (`agents/audit.jsonl`): the agent asks (tool and request),
/// the client sends, the answer comes back; refusals and denials say why. The log holds no
/// values; `detail` (masked text) appears only with `include_values`.
pub fn model_from_audit(entries: &[AuditEntry], options: &SequenceOptions) -> SequenceModel {
    let mut b = Builder::new();
    b.participant(CLIENT, "Client", ParticipantKind::Client);
    for e in entries {
        let agent_id = format!("agent:{}", clean(&e.agent));
        let first_agent = !b.model.participants.iter().any(|p| p.id == agent_id);
        if first_agent {
            b.participant(&agent_id, &e.agent, ParticipantKind::Agent);
            // Agents sit left of the client.
            if let Some(i) = b.model.participants.iter().position(|p| p.id == agent_id) {
                let p = b.model.participants.remove(i);
                b.model.participants.insert(0, p);
            }
        }
        let (host, path, query) = e
            .url
            .as_deref()
            .map_or((None, String::new(), None), split_url);
        let server = host.as_deref().map(|h| b.server(h));
        let other = server.clone().unwrap_or_else(|| agent_id.clone());
        let need = if server.is_some() { 4 } else { 2 };
        if !b.fits(&other, need) {
            b.model.omitted += need as u32;
            continue;
        }
        b.count(&other, need);
        let ask = match &e.request_path {
            Some(p) => format!("{} {}", e.tool, p),
            None => e.tool.clone(),
        };
        b.message(
            MessageKind::Request,
            &agent_id,
            CLIENT,
            &ask,
            e.request_path.as_deref(),
        );
        let mut result = match e.outcome {
            AuditOutcome::Read => "ok".to_owned(),
            AuditOutcome::Sent => "sent".to_owned(),
            AuditOutcome::Refused => "refused".to_owned(),
            AuditOutcome::Denied => "denied".to_owned(),
            AuditOutcome::TimedOut => "timed out".to_owned(),
            AuditOutcome::Failed => "failed".to_owned(),
        };
        if let Some(server) = &server {
            let method = e.method.as_deref().unwrap_or("");
            b.message(
                MessageKind::Request,
                CLIENT,
                server,
                &format!("{method} {}", path_label(&path, query.as_deref(), options)),
                e.request_path.as_deref(),
            );
            if let Some(status) = e.status {
                b.message(
                    MessageKind::Response,
                    server,
                    CLIENT,
                    &status.to_string(),
                    e.request_path.as_deref(),
                );
                result = status.to_string();
            } else {
                b.message(
                    MessageKind::Response,
                    server,
                    CLIENT,
                    &result.clone(),
                    e.request_path.as_deref(),
                );
            }
        }
        b.message(
            MessageKind::Response,
            CLIENT,
            &agent_id,
            &result,
            e.request_path.as_deref(),
        );
        let mut facts = Vec::new();
        if let Some(rule) = &e.rule {
            facts.push(format!("rule: {}", clip(rule, 40)));
        }
        if matches!(
            e.outcome,
            AuditOutcome::Refused | AuditOutcome::Denied | AuditOutcome::TimedOut
        ) {
            facts.push(format!("outcome: {result}"));
        }
        if let Some(by) = &e.approved_by {
            facts.push(format!("approved by {}", clip(by, 20)));
        }
        if options.include_values && !e.detail.is_empty() {
            match safe_excerpt(&e.detail) {
                Some(d) => facts.push(d),
                None => facts.push("detail withheld".to_owned()),
            }
        }
        if !facts.is_empty() {
            b.over(CLIENT, &facts.join("; "));
        }
    }
    b.finish()
}

/// Entries of the history: method, path and status only (the history keeps nothing more).
pub fn model_from_history(entries: &[HistoryEntry], options: &SequenceOptions) -> SequenceModel {
    let records: Vec<Record> = entries
        .iter()
        .map(|e| {
            let (host, path, query) = e
                .url
                .as_deref()
                .map_or((None, String::new(), None), split_url);
            Record {
                run: 0,
                caller: Caller::App,
                request_path: e.request_path.clone(),
                method: e.method.clone(),
                host,
                path,
                query,
                status: e.status,
                millis: e.duration_ms.map(|d| d.round() as u64),
                error: e.error.as_ref().map(error_kind),
                rule: None,
                tests_total: 0,
                tests_failed: Vec::new(),
                assertions_total: 0,
                assertions_failed: Vec::new(),
                content_type: None,
                location: None,
                body_head: None,
            }
        })
        .collect();
    let refs: Vec<&Record> = records.iter().collect();
    model_from_records(&refs, options, false)
}

/// A WebSocket, SSE or gRPC session: the handshake, then the messages as one `loop`.
pub fn model_from_stream(log: &StreamLog, options: &SequenceOptions) -> SequenceModel {
    let mut b = Builder::new();
    b.participant(CLIENT, "Client", ParticipantKind::Client);
    let (host, path, query) = split_url(&log.url);
    let server = b.server(host.as_deref().unwrap_or("server"));
    let kind = clip(&log.kind, 12);
    let label = format!(
        "{} {}",
        kind.to_uppercase(),
        path_label(&path, query.as_deref(), options)
    );
    let rp = Some(log.request_path.as_str());
    let mut loop_from: Option<usize> = None;
    let mut loop_to = 0;
    for event in &log.events {
        let n = b.model.messages.len();
        if !b.fits(&server, 1) {
            b.model.omitted += 1;
            continue;
        }
        match event {
            StreamEvent::Opened { head, .. } => {
                if !b.fits(&server, 2) {
                    b.model.omitted += 2;
                    continue;
                }
                b.count(&server, 2);
                b.message(MessageKind::Request, CLIENT, &server, &label, rp);
                b.message(
                    MessageKind::Response,
                    &server,
                    CLIENT,
                    &head.status.to_string(),
                    rp,
                );
            }
            StreamEvent::Message {
                direction,
                frame,
                name,
                ..
            } => {
                b.count(&server, 1);
                let what = match name {
                    Some(name) => format!("{kind} {}", clip(name, 30)),
                    None => format!("{kind} message"),
                };
                let (kind_m, from, to) = match direction {
                    Direction::Sent => (MessageKind::Request, CLIENT, server.as_str()),
                    Direction::Received => (MessageKind::Response, server.as_str(), CLIENT),
                };
                let mut text = what;
                if options.include_values
                    && let rt_core::stream::Frame::Text(t) = frame
                    && let Some(excerpt) = safe_excerpt(t)
                {
                    text = format!("{text}: {excerpt}");
                }
                loop_from.get_or_insert(n);
                loop_to = n;
                b.message(kind_m, from, to, &text, rp);
            }
            StreamEvent::Closed {
                code, by_client, ..
            } => {
                let who = if *by_client { "client" } else { "server" };
                let text = match code {
                    Some(c) => format!("closed by {who}, code {c}"),
                    None => format!("closed by {who}"),
                };
                b.over(&server, &text);
            }
            StreamEvent::Error { .. } => {
                b.over(&server, "error");
            }
        }
    }
    if let Some(from) = loop_from {
        b.model.groups.push(Group {
            kind: "loop".into(),
            label: "messages".into(),
            from,
            to: loop_to,
        });
    }
    b.finish()
}
