// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand G: sequence diagrams (acceptance point 10). The model from runs, pages,
//! streams and the agent audit log; Mermaid with every escape case (golden files in
//! `testdata/insight/sequence/`, `RT_UPDATE_GOLDEN=1` rewrites them); values only on request
//! and only after the sharing rules (canary secret and canary personal data); the limits.

use std::sync::Arc;
use std::time::Duration;

use rt_app::agents::{AuditEntry, AuditOutcome};
use rt_app::sequence::{
    MAX_LABEL, MAX_MESSAGES, MAX_MESSAGES_PER_SWITCH, MAX_TEXT, MessageKind, NoteAnchor, Recorder,
    SequenceModel, SequenceOptions, SequenceSource, model_from_audit, model_from_history,
    model_from_stream, render, to_markdown,
};
use rt_core::guard::Caller;
use rt_core::observe::{ExchangeObserver, ObserverSet, exchange_of};
use rt_core::send::{
    REPORT_VERSION, ReceivedResponse, ReportBody, SendError, SendReport, SentRequest,
};
use rt_core::stream::{Direction, Frame, StreamEvent, StreamHead, StreamLog};
use rt_core::transport::{HttpVersion, Timing};
use serde_json::json;

const SECRET: &str = "RT-CANARY-sequence-secret-4b7e";
const PERSONAL: &str = "alice.canary@example.test";

struct Ex {
    method: &'static str,
    url: String,
    status: Option<u16>,
    ms: u64,
    body: String,
    headers: Vec<(String, String)>,
    error: Option<SendError>,
    tests: Vec<(&'static str, bool)>,
    assertions: Vec<(&'static str, bool)>,
    caller: Caller,
    path: &'static str,
}

fn ex(method: &'static str, url: &str, status: u16) -> Ex {
    Ex {
        method,
        url: url.into(),
        status: Some(status),
        ms: 41,
        body: "{}".into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        error: None,
        tests: vec![],
        assertions: vec![],
        caller: Caller::App,
        path: "orders/list.yml",
    }
}

impl Ex {
    fn body(mut self, b: &str) -> Self {
        self.body = b.into();
        self
    }
    fn path(mut self, p: &'static str) -> Self {
        self.path = p;
        self
    }
    fn caller(mut self, c: Caller) -> Self {
        self.caller = c;
        self
    }
    fn header(mut self, n: &str, v: &str) -> Self {
        self.headers.push((n.into(), v.into()));
        self
    }
    fn failing_test(mut self, name: &'static str) -> Self {
        self.tests = vec![("status is 200", true), (name, false)];
        self
    }
    fn failing_assertion(mut self) -> Self {
        self.assertions = vec![("res.status", false), ("res.body.id", true)];
        self
    }
    fn error(mut self, e: SendError) -> Self {
        self.status = None;
        self.error = Some(e);
        self
    }

    fn report(&self) -> SendReport {
        let mut report: SendReport = serde_json::from_value(json!({
            "version": REPORT_VERSION, "request_path": self.path, "environment": null,
            "request": null, "response": null, "variables": [], "unresolved": [], "error": null
        }))
        .unwrap();
        report.request = Some(SentRequest {
            method: self.method.into(),
            url: self.url.clone(),
            headers: vec![("Authorization".into(), format!("Bearer {SECRET}"))],
            body_size: 0,
        });
        if let Some(status) = self.status {
            report.response = Some(ReceivedResponse {
                tls: None,
                status,
                version: HttpVersion::Http11,
                headers: self.headers.clone(),
                url: self.url.clone(),
                redirects: vec![],
                size: self.body.len() as u64,
                body: ReportBody::Utf8(self.body.clone()),
                truncated: false,
                timing: Timing {
                    dns: None,
                    connect: None,
                    tls: None,
                    ttfb: Duration::from_millis(self.ms / 2),
                },
                total: Duration::from_millis(self.ms),
            });
        }
        report.error = self.error.clone();
        report.tests = self
            .tests
            .iter()
            .map(|(n, p)| {
                serde_json::from_value(json!({"name": n, "passed": p, "origin": "request"}))
                    .unwrap()
            })
            .collect();
        report.assertions = self
            .assertions
            .iter()
            .map(|(e, p)| {
                serde_json::from_value(json!({
                    "expression": e, "operator": "eq", "expected": "200", "actual": "500", "passed": p
                }))
                .unwrap()
            })
            .collect();
        report
    }

    /// What a shell hands the observers: masked with the send's secret values.
    fn exchange(&self) -> rt_core::observe::Exchange {
        exchange_of(
            &self.report(),
            &[SECRET.to_owned()],
            self.caller,
            Some("demo".into()),
        )
    }
}

fn recorder_of(items: &[Ex]) -> Recorder {
    let r = Recorder::new();
    for item in items {
        r.observe(&item.exchange());
    }
    r
}

fn session(r: &Recorder, options: &SequenceOptions) -> SequenceModel {
    r.model(&SequenceSource::Session, options).unwrap()
}

fn labels(m: &SequenceModel) -> Vec<String> {
    m.messages.iter().map(|m| m.label.clone()).collect()
}

fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/insight/sequence")
        .join(name);
    if std::env::var_os("RT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        actual, want,
        "golden file {name} differs (RT_UPDATE_GOLDEN=1 rewrites)"
    );
}

fn off() -> SequenceOptions {
    SequenceOptions::default()
}

fn on() -> SequenceOptions {
    SequenceOptions {
        include_query: false,
        include_values: true,
    }
}

#[test]
fn a_run_becomes_participants_requests_and_responses() {
    let r = recorder_of(&[
        ex("GET", "https://api.example.com/orders?limit=5", 200),
        ex("POST", "https://api.example.com/orders", 201).path("orders/create.yml"),
        ex("GET", "https://auth.example.com/health", 503),
    ]);
    let m = session(&r, &off());
    let ids: Vec<&str> = m.participants.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["client", "api.example.com", "auth.example.com"]);
    assert_eq!(
        labels(&m),
        [
            "GET /orders",
            "200 (41 ms)",
            "POST /orders",
            "201 (41 ms)",
            "GET /health",
            "503 (41 ms)"
        ]
    );
    assert_eq!(m.messages[0].kind, MessageKind::Request);
    assert_eq!(m.messages[1].kind, MessageKind::Response);
    assert_eq!(m.messages[0].from, "client");
    assert_eq!(m.messages[0].to, "api.example.com");
    assert_eq!(m.messages[1].to, "client");
    assert_eq!(
        m.messages[0].request_path.as_deref(),
        Some("orders/list.yml")
    );
    assert_eq!(m.omitted, 0);
}

#[test]
fn the_query_is_left_out_unless_asked_for_and_ids_become_placeholders() {
    let url = "https://api.example.com/orders/12345/items/3f2b9c1e-7a4d-4e1b-9c3a-0d5e6f7a8b9c?page=2&key=abc";
    let r = recorder_of(&[ex("GET", url, 200)]);
    let plain = session(&r, &off());
    assert_eq!(plain.messages[0].label, "GET /orders/{id}/items/{id}");
    let with_query = session(
        &r,
        &SequenceOptions {
            include_query: true,
            include_values: false,
        },
    );
    assert!(with_query.messages[0].label.contains("?page=2&key=abc"));
    // An address in the path is data, not a name of the API.
    let personal = recorder_of(&[ex(
        "GET",
        &format!("https://api.example.com/users/{PERSONAL}/orders"),
        200,
    )]);
    assert_eq!(
        session(&personal, &off()).messages[0].label,
        "GET /users/{id}/orders"
    );
    // Values on: the path as it was.
    let values = session(&r, &on());
    assert!(values.messages[0].label.contains("12345"));
}

#[test]
fn runs_are_stretches_between_begin_and_end() {
    let r = Recorder::new();
    r.observe(&ex("GET", "https://api.example.com/before", 200).exchange());
    let one = r.begin_run();
    r.observe(&ex("GET", "https://api.example.com/a", 200).exchange());
    r.end_run();
    let two = r.begin_run();
    r.observe(&ex("GET", "https://api.example.com/b", 200).exchange());
    r.observe(&ex("GET", "https://api.example.com/c", 200).exchange());
    r.end_run();
    assert_eq!((one, two), (1, 2));
    let run = |id| {
        labels(
            &r.model(&SequenceSource::Run { run_id: id }, &off())
                .unwrap(),
        )
        .into_iter()
        .step_by(2)
        .collect::<Vec<_>>()
    };
    assert_eq!(run(1), ["GET /a"]);
    assert_eq!(run(2), ["GET /b", "GET /c"]);
    assert_eq!(run(0), ["GET /b", "GET /c"], "0 is the latest run");
    assert_eq!(session(&r, &off()).messages.len(), 8);
    r.clear();
    assert!(r.is_empty());
}

#[test]
fn pages_are_a_loop_with_page_numbers() {
    let page = |p: &str| {
        ex(
            "GET",
            &format!("https://api.example.com/items?page={p}"),
            200,
        )
        .path("items.yml")
    };
    let r = Recorder::new();
    r.observe(&ex("GET", "https://api.example.com/other", 200).exchange());
    for p in ["1", "2", "3"] {
        r.observe(&page(p).exchange());
    }
    let m = r
        .model(
            &SequenceSource::Pages {
                request_path: "items.yml".into(),
            },
            &off(),
        )
        .unwrap();
    assert_eq!(m.messages.len(), 6);
    assert_eq!(m.groups.len(), 1);
    assert_eq!(
        (m.groups[0].kind.as_str(), m.groups[0].from, m.groups[0].to),
        ("loop", 0, 5)
    );
    let texts: Vec<&str> = m.notes.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(texts, ["page 1/3", "page 2/3", "page 3/3"]);
    let text = render(&m, &off()).mermaid;
    assert!(text.contains("loop 3 pages"));
    assert!(text.contains("Note over P1: page 3/3"));
}

#[test]
fn failures_become_notes() {
    let r = recorder_of(&[
        ex("GET", "https://api.example.com/orders", 500)
            .failing_test("returns a list")
            .failing_assertion(),
        ex("GET", "https://api.example.com/slow", 200).error(SendError::Script("boom".into())),
    ]);
    let m = session(&r, &off());
    let notes: Vec<&str> = m.notes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        notes
            .iter()
            .any(|t| t.starts_with("1 of 2 tests failed: returns a list")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|t| t.starts_with("1 of 2 assertions failed: res.status eq")),
        "{notes:?}"
    );
    // The assertion's numbers are values: not without the switch.
    assert!(!notes.iter().any(|t| t.contains("500")), "{notes:?}");
    let values = session(&r, &on());
    assert!(values.notes.iter().any(|n| n.text.contains("got 500")));
    assert!(m.messages.iter().any(|m| m.label == "error: script"));
}

#[test]
fn a_refused_send_is_a_note_and_nothing_goes_to_the_server() {
    let mut e = ex("GET", "https://api.example.com/orders", 200);
    e.status = None;
    e.error = Some(SendError::Blocked {
        rule: "no-prod-writes".into(),
        reason: "writing to production".into(),
    });
    let r = recorder_of(&[e]);
    let m = session(&r, &off());
    assert_eq!(m.participants.len(), 1, "no server was contacted");
    assert!(
        m.notes
            .iter()
            .any(|n| n.text.contains("blocked by no-prod-writes"))
    );
}

#[test]
fn an_agent_run_has_an_agent_participant() {
    let r = recorder_of(&[ex("GET", "https://api.example.com/orders", 200).caller(Caller::Agent)]);
    let m = session(&r, &off());
    assert_eq!(m.participants[0].id, "agent");
    assert_eq!(m.messages[0].from, "agent");
    assert!(m.messages[0].label.starts_with("send_request"));
    assert_eq!(m.messages.last().unwrap().to, "agent");
}

fn audit(tool: &str, outcome: AuditOutcome) -> AuditEntry {
    AuditEntry {
        at: 1,
        agent: "claude".into(),
        tool: tool.into(),
        collection: "demo".into(),
        request_path: Some("orders/create.yml".into()),
        environment: Some("Production".into()),
        method: Some("POST".into()),
        url: Some("https://api.example.com/orders?token=***".into()),
        status: None,
        outcome,
        approved_by: None,
        rule: None,
        detail: format!("refused for {PERSONAL}"),
    }
}

#[test]
fn an_mcp_run_comes_from_the_audit_log() {
    let mut sent = audit("send_request", AuditOutcome::Sent);
    sent.status = Some(201);
    sent.approved_by = Some("app".into());
    let mut refused = audit("send_request", AuditOutcome::Refused);
    refused.rule = Some("no-prod-writes".into());
    let list = AuditEntry {
        url: None,
        method: None,
        request_path: None,
        ..audit("list_requests", AuditOutcome::Read)
    };
    let m = model_from_audit(&[list, sent, refused], &off());
    assert_eq!(m.participants[0].id, "agent:claude");
    assert_eq!(m.participants[0].label, "claude");
    let l = labels(&m);
    assert_eq!(l[0], "list_requests");
    assert!(l.contains(&"send_request orders/create.yml".to_owned()));
    assert!(l.contains(&"POST /orders".to_owned()));
    assert!(l.contains(&"201".to_owned()));
    assert!(l.contains(&"refused".to_owned()));
    let notes: Vec<&str> = m.notes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        notes.iter().any(|t| t.contains("rule: no-prod-writes")),
        "{notes:?}"
    );
    assert!(notes.iter().any(|t| t.contains("approved by app")));
    // The detail is text of the log: only with values.
    assert!(!format!("{m:?}").contains(PERSONAL));
    let values = model_from_audit(&[audit("send_request", AuditOutcome::Refused)], &on());
    assert!(format!("{values:?}").contains(PERSONAL));
}

#[test]
fn history_entries_give_method_path_and_status_only() {
    let entries: Vec<rt_app::HistoryEntry> = serde_json::from_value(json!([{
        "id": 1, "at": 1, "collection": "demo", "request_path": "orders/list.yml",
        "name": "List", "environment": null, "method": "GET",
        "url": "https://api.example.com/orders/77?x=1", "status": 200, "error": null,
        "duration_ms": 12.4, "size": 10
    }]))
    .unwrap();
    let m = model_from_history(&entries, &off());
    assert_eq!(labels(&m), ["GET /orders/{id}", "200 (12 ms)"]);
}

#[test]
fn a_stream_is_a_loop_of_messages() {
    let log = StreamLog {
        version: 1,
        request_path: "chat.yml".into(),
        environment: None,
        kind: "websocket".into(),
        url: "wss://chat.example.com:8443/room/42".into(),
        events: vec![
            StreamEvent::Opened {
                at_ms: 0,
                head: StreamHead {
                    status: 101,
                    version: HttpVersion::Http11,
                    headers: vec![],
                    url: "wss://chat.example.com:8443/room/42".into(),
                },
            },
            StreamEvent::Message {
                at_ms: 1,
                direction: Direction::Sent,
                frame: Frame::Text(format!("hello {PERSONAL}")),
                name: None,
                id: None,
            },
            StreamEvent::Message {
                at_ms: 2,
                direction: Direction::Received,
                frame: Frame::Text("ack".into()),
                name: Some("ack".into()),
                id: None,
            },
            StreamEvent::Closed {
                at_ms: 3,
                code: Some(1000),
                reason: "bye".into(),
                trailers: vec![],
                by_client: true,
            },
        ],
        notes: vec![],
    };
    let m = model_from_stream(&log, &off());
    assert_eq!(m.participants[1].id, "chat.example.com:8443");
    assert_eq!(
        labels(&m),
        [
            "WEBSOCKET /room/{id}",
            "101",
            "websocket message",
            "websocket ack"
        ]
    );
    assert_eq!(m.groups.len(), 1);
    assert_eq!((m.groups[0].from, m.groups[0].to), (2, 3));
    assert!(
        m.notes
            .iter()
            .any(|n| n.text == "closed by client, code 1000")
    );
    assert!(!format!("{m:?}").contains(PERSONAL));
    assert!(format!("{:?}", model_from_stream(&log, &on())).contains(PERSONAL));
}

fn nasty_model() -> SequenceModel {
    serde_json::from_value(json!({
        "participants": [
            {"id": "client", "label": "Client", "kind": "client"},
            {"id": "api.example.com:8443", "label": "api.example.com:8443", "kind": "server"},
            {"id": "agent", "label": "Agent; #1 <bot>", "kind": "agent"}
        ],
        "messages": [
            {"kind": "request", "from": "client", "to": "api.example.com:8443", "label": "GET /a;b#c:d%%e"},
            {"kind": "response", "from": "api.example.com:8443", "to": "client", "label": "line one\nline two\ttab"},
            {"kind": "request", "from": "agent", "to": "client", "label": "x".repeat(200)},
            {"kind": "response", "from": "client", "to": "agent", "label": "<script>`tick`</script>"},
            {"kind": "request", "from": "client", "to": "api.example.com:8443", "label": ""},
            {"kind": "response", "from": "api.example.com:8443", "to": "client", "label": "200"}
        ],
        "notes": [
            {"after": 1, "anchor": {"kind": "over", "participant": "client"}, "text": "semi; hash# colon: pct%"},
            {"after": 3, "anchor": {"kind": "between", "from": "agent", "to": "client"}, "text": "two\nlines"}
        ],
        "groups": [
            {"kind": "rect", "label": "flow node", "from": 0, "to": 3},
            {"kind": "loop", "label": "each page; again", "from": 1, "to": 1},
            {"kind": "alt", "label": "when 500", "from": 4, "to": 5}
        ],
        "omitted": 0
    }))
    .unwrap()
}

#[test]
fn mermaid_escapes_every_special_case() {
    let model = nasty_model();
    let text = render(&model, &off());
    golden("escape.mmd", &text.mermaid);
    golden("escape.md", &text.markdown);
    for line in text.mermaid.lines().skip(1) {
        let body = line.trim();
        if body.starts_with("%%") {
            continue;
        }
        // After the first colon of a statement no raw `;`, `#` (outside an entity) or
        // line break may remain.
        if let Some((_, label)) = body.split_once(": ") {
            // Entities (`#59;`, `#lt;`) are the only place `#` and `;` may stand.
            let mut plain = String::new();
            let mut rest = label;
            while let Some(i) = rest.find('#') {
                plain.push_str(&rest[..i]);
                rest = &rest[i + 1..];
                let end = rest.find(';').unwrap_or_else(|| panic!("bare # in {line}"));
                assert!(
                    end <= 4 && rest[..end].chars().all(|c| c.is_ascii_alphanumeric()),
                    "bare # in {line}"
                );
                rest = &rest[end + 1..];
            }
            plain.push_str(rest);
            for bad in [';', '#', ':', '<', '>', '`', '%'] {
                assert!(!plain.contains(bad), "{bad:?} in {line}");
            }
        }
    }
    // Labels are cut at the limit.
    assert!(
        text.mermaid
            .lines()
            .all(|l| l.chars().count() < MAX_LABEL + 80)
    );
}

#[test]
fn a_run_renders_as_mermaid_and_markdown() {
    let r = recorder_of(&[
        ex("GET", "https://api.example.com/orders", 200),
        ex("GET", "https://api.example.com/orders/9", 404).failing_test("finds it"),
    ]);
    let text = render(&session(&r, &off()), &off());
    golden("run.mmd", &text.mermaid);
    assert_eq!(text.markdown, to_markdown(&text.mermaid));
    assert!(text.markdown.starts_with("```mermaid\nsequenceDiagram\n"));
    assert!(text.markdown.ends_with("\n```\n"));
}

#[test]
fn values_are_off_by_default_and_pass_the_sharing_rules_when_on() {
    let body = json!({
        "id": 42,
        "email": PERSONAL,
        "password": "hunter2-canary",
        "api_token": SECRET,
        "note": format!("Bearer {SECRET}"),
        "plain": "a".repeat(100),
    })
    .to_string();
    let r = recorder_of(&[ex(
        "GET",
        &format!("https://api.example.com/users?api_key={SECRET}"),
        200,
    )
    .body(&body)
    .header("Location", "https://api.example.com/users/42")
    .header("Set-Cookie", &format!("sid={SECRET}"))]);
    let all = |options: &SequenceOptions| {
        let m = session(&r, options);
        let t = render(&m, options);
        format!("{m:?}\n{}\n{}", t.mermaid, t.markdown)
    };
    let default = all(&off());
    for forbidden in [SECRET, PERSONAL, "hunter2", "Set-Cookie", "sid="] {
        assert!(
            !default.contains(forbidden),
            "default output holds {forbidden}"
        );
    }
    let with_query = all(&SequenceOptions {
        include_query: true,
        include_values: false,
    });
    assert!(
        !with_query.contains(SECRET),
        "the masked query still carries the secret"
    );
    let values = all(&on());
    assert!(values.contains("id=42"), "{values}");
    assert!(values.contains(PERSONAL), "{values}");
    assert!(values.contains("Location: https://api.example.com/users/42"));
    for forbidden in [SECRET, "hunter2", "sid=", "password", "api_token", "Bearer"] {
        assert!(
            !values.contains(forbidden),
            "values output holds {forbidden}"
        );
    }
    // Excerpts are short.
    let mermaid = render(&session(&r, &on()), &on()).mermaid;
    let plain = mermaid.lines().find(|l| l.contains("plain=")).unwrap();
    let longest_run = plain.split(|c| c != 'a').map(str::len).max().unwrap_or(0);
    assert!(longest_run <= 40, "{plain}");
}

#[test]
fn a_secret_in_a_value_withholds_the_excerpt() {
    // Not masked by the send (it was not a known value), but the scanner knows the shape.
    let body = json!({"id": 1, "note": "ghp_16C7e42F292c6912E7710c838347Ae178B4a"}).to_string();
    let r = recorder_of(&[ex("GET", "https://api.example.com/x", 200).body(&body)]);
    let text = render(&session(&r, &on()), &on()).mermaid;
    assert!(!text.contains("ghp_"), "{text}");
}

#[test]
fn the_limits_hold() {
    // 8 hosts in turn: the message cap bites.
    let r = Recorder::new();
    for i in 0..1300 {
        let host = i % 8;
        r.observe(&ex("GET", &format!("https://h{host}.example.com/p"), 200).exchange());
    }
    let m = session(&r, &off());
    assert_eq!(m.participants.len(), 9);
    assert_eq!(m.messages.len(), MAX_MESSAGES);
    assert_eq!(m.omitted as usize, 1300 * 2 - MAX_MESSAGES);
    let last = m.notes.last().unwrap();
    assert_eq!(last.text, format!("{} more messages not shown", m.omitted));
    assert_eq!(last.after, MAX_MESSAGES - 1);
    assert!(matches!(&last.anchor, NoteAnchor::Over { participant } if participant == "client"));
    let text = render(&m, &off()).mermaid;
    assert!(text.len() < MAX_TEXT);
    assert!(text.contains("more messages not shown"));

    // One host in a row: 500 messages, then "n more".
    let r = Recorder::new();
    for _ in 0..400 {
        r.observe(&ex("GET", "https://one.example.com/p", 200).exchange());
    }
    let m = session(&r, &off());
    assert_eq!(m.messages.len(), MAX_MESSAGES_PER_SWITCH);
    assert_eq!(m.omitted, 800 - MAX_MESSAGES_PER_SWITCH as u32);
}

#[test]
fn the_recorder_is_an_observer_in_a_set_and_keeps_a_bounded_list() {
    let recorder = Arc::new(Recorder::new());
    let set = ObserverSet::new().with(recorder.clone());
    set.notify(&ex("GET", "https://api.example.com/orders", 200).exchange());
    assert_eq!(recorder.len(), 1);
    for _ in 0..5000 {
        set.notify(&ex("GET", "https://api.example.com/orders", 200).exchange());
    }
    assert!(recorder.len() <= 4000);
}
