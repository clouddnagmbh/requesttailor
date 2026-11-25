// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand E: the webhook inbox (`rt_app::inbox`). Acceptance: it takes requests of five
//! methods on any path on loopback, answers with a fixed reply, refuses foreign peers, a body over
//! 2 MiB (`413`) and more than 200 requests per second (`429`), holds at most 500 requests and
//! 64 MiB in memory only, and "Make request" turns secrets into variables without a value.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use rt_app::inbox::{
    CapturedRequest, Inbox, InboxEvent, InboxSettings, Limits, MAX_BODY_BYTES, MAX_ENTRIES,
    is_loopback_peer,
};
use rt_core::model::Body;

const CANARY: &str = "RT-CANARY-inbox-7f3a9c";

fn loose() -> Limits {
    Limits {
        per_second: 1_000_000,
        ..Limits::default()
    }
}

fn started(limits: Limits) -> (Inbox, u16) {
    let inbox = Inbox::with_limits(limits);
    inbox
        .set_settings(InboxSettings {
            port: 0,
            ..InboxSettings::default()
        })
        .unwrap();
    let port = inbox.start().unwrap().port.unwrap();
    (inbox, port)
}

struct Answer {
    status: u16,
    head: String,
    body: String,
}

fn raw(port: u16, request: &[u8]) -> Answer {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = s.write_all(request);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Answer {
        status: head
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        head: head.to_owned(),
        body: body.to_owned(),
    }
}

fn call(port: u16, method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> Answer {
    let mut req = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (n, v) in headers {
        req.push_str(&format!("{n}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    raw(port, req.as_bytes())
}

fn requests(inbox: &Inbox) -> Vec<CapturedRequest> {
    inbox
        .events(0)
        .events
        .into_iter()
        .filter_map(|e| match e {
            InboxEvent::Request { request } => Some(*request),
            _ => None,
        })
        .collect()
}

#[test]
fn takes_five_methods_on_any_path_and_answers_with_a_fixed_reply() {
    let (inbox, port) = started(loose());
    for (i, m) in ["POST", "PUT", "PATCH", "GET", "DELETE"].iter().enumerate() {
        let a = call(
            port,
            m,
            &format!("/hooks/orders/{i}?x=1&y=two"),
            &[("Content-Type", "application/json"), ("X-Event", "order")],
            "{\"id\":7}",
        );
        assert_eq!(a.status, 200, "{m}");
        assert_eq!(a.body, "", "the default answer has no body");
        assert!(!a.head.to_ascii_lowercase().contains("access-control"), "no CORS header: {}", a.head);
    }
    let held = requests(&inbox);
    assert_eq!(held.len(), 5);
    assert_eq!(held[0].method, "POST");
    assert_eq!(held[0].target, "/hooks/orders/0?x=1&y=two");
    assert_eq!(held[0].body.as_deref(), Some("{\"id\":7}"));
    assert_eq!(held[0].size, 8);
    assert!(held[0].headers.iter().any(|(n, v)| n == "X-Event" && v == "order"));
    assert_eq!(held.iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    // Other methods are not taken.
    assert_eq!(call(port, "OPTIONS", "/x", &[], "").status, 405);
    assert_eq!(requests(&inbox).len(), 5);
    inbox.stop();
}

#[test]
fn chunked_bodies_and_binary_bodies() {
    let (inbox, port) = started(loose());
    let a = raw(
        port,
        b"POST /c HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\n\r\n",
    );
    assert_eq!(a.status, 200);
    let mut bin = b"POST /b HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\n".to_vec();
    bin.extend_from_slice(&[0xff, 0xfe, 0x00]);
    assert_eq!(raw(port, &bin).status, 200);
    let held = requests(&inbox);
    assert_eq!(held[0].body.as_deref(), Some("hello world"));
    assert_eq!(held[1].body, None, "a binary body is not text");
    assert_eq!(held[1].size, 3);
    inbox.stop();
}

#[test]
fn the_reply_is_configurable_within_limits() {
    let (inbox, port) = started(loose());
    inbox
        .set_settings(InboxSettings {
            port,
            status: 202,
            body: "{\"ok\":true}".into(),
            content_type: Some("application/json".into()),
            delay_ms: 200,
        })
        .unwrap();
    let t = Instant::now();
    let a = call(port, "POST", "/x", &[], "a");
    assert!(t.elapsed() >= Duration::from_millis(200));
    assert_eq!(a.status, 202);
    assert_eq!(a.body, "{\"ok\":true}");
    assert!(a.head.contains("Content-Type: application/json"));
    // The reply never contains the capture.
    assert!(!a.body.contains('a') || a.body == "{\"ok\":true}");
    for bad in [
        InboxSettings { delay_ms: 5001, ..InboxSettings::default() },
        InboxSettings { status: 99, ..InboxSettings::default() },
        InboxSettings { status: 600, ..InboxSettings::default() },
        InboxSettings { content_type: Some("a\r\nb".into()), ..InboxSettings::default() },
    ] {
        assert!(inbox.set_settings(bad).is_err());
    }
    inbox.stop();
}

#[test]
fn listens_on_loopback_only_and_refuses_foreign_peers() {
    let (inbox, _) = started(loose());
    assert!(inbox.status().url.unwrap().starts_with("http://127.0.0.1:"));
    assert!(is_loopback_peer(SocketAddr::from((Ipv4Addr::LOCALHOST, 5))));
    assert!(is_loopback_peer("[::1]:5".parse().unwrap()));
    assert!(!is_loopback_peer("192.168.1.20:5".parse().unwrap()));
    assert!(!is_loopback_peer("10.0.0.1:5".parse().unwrap()));
    assert!(!is_loopback_peer("[2001:db8::1]:5".parse().unwrap()));
    assert!(!is_loopback_peer("0.0.0.0:5".parse().unwrap()));
    inbox.stop();
}

#[test]
fn a_body_over_2_mib_is_413_and_not_kept() {
    let (inbox, port) = started(loose());
    let big = MAX_BODY_BYTES + 1;
    let a = raw(
        port,
        format!("POST /big HTTP/1.1\r\nHost: x\r\nContent-Length: {big}\r\n\r\n").as_bytes(),
    );
    assert_eq!(a.status, 413);
    // Chunked: the limit applies as the chunks arrive.
    let chunk = "a".repeat(MAX_BODY_BYTES + 1);
    let req = format!(
        "POST /big HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{chunk}\r\n0\r\n\r\n",
        chunk.len()
    );
    assert_eq!(raw(port, req.as_bytes()).status, 413);
    // Exactly the limit is taken.
    let ok = "b".repeat(MAX_BODY_BYTES);
    assert_eq!(call(port, "POST", "/ok", &[], &ok).status, 200);
    let held = requests(&inbox);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].size, MAX_BODY_BYTES as u64);
    inbox.stop();
}

#[test]
fn more_than_200_requests_per_second_get_429() {
    let (inbox, port) = started(Limits::default());
    // A burst from 16 connections at once: more than the bucket (200) holds.
    let t = Instant::now();
    let results: Vec<u16> = (0..16)
        .map(|_| {
            std::thread::spawn(move || {
                (0..40)
                    .map(|_| call(port, "POST", "/r", &[], "x").status)
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    let secs = t.elapsed().as_secs_f64();
    let ok = results.iter().filter(|s| **s == 200).count();
    let limited = results.iter().filter(|s| **s == 429).count();
    assert_eq!(ok + limited, 640, "{results:?}");
    assert!(limited > 0, "a burst of 640 is limited (took {secs:.2} s)");
    assert!(
        ok as f64 <= 200.0 + 200.0 * secs + 1.0,
        "{ok} accepted in {secs:.2} s"
    );
    assert_eq!(requests(&inbox).len(), ok, "a refused request is not kept");
    inbox.stop();
}

#[test]
fn paced_at_200_per_second_nothing_is_lost() {
    let (inbox, port) = started(Limits::default());
    let start = Instant::now();
    for i in 0..200u32 {
        let due = start + Duration::from_millis(u64::from(i) * 5);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        assert_eq!(call(port, "POST", "/p", &[], "x").status, 200, "request {i}");
    }
    assert_eq!(requests(&inbox).len(), 200);
    inbox.stop();
}

#[test]
fn the_buffer_holds_500_requests_and_counts_what_fell_out() {
    let (inbox, port) = started(loose());
    for i in 0..(MAX_ENTRIES + 20) {
        assert_eq!(call(port, "POST", &format!("/n/{i}"), &[], "x").status, 200);
    }
    let held = requests(&inbox);
    assert_eq!(held.len(), MAX_ENTRIES);
    assert_eq!(held[0].target, "/n/20", "the oldest fell out");
    let status = inbox.status();
    assert_eq!((status.held, status.dropped), (MAX_ENTRIES as u64, 20));
    let dropped: u64 = inbox
        .events(0)
        .events
        .iter()
        .map(|e| if let InboxEvent::Dropped { count } = e { *count } else { 0 })
        .sum();
    assert_eq!(dropped, 20);
    assert!(inbox.get(1).is_none() && inbox.get(21).is_some());
    inbox.stop();
}

#[test]
fn the_buffer_holds_at_most_the_byte_limit() {
    let limits = Limits {
        max_total_bytes: 20_000,
        ..loose()
    };
    let (inbox, port) = started(limits);
    let body = "z".repeat(4_000);
    for _ in 0..12 {
        assert_eq!(call(port, "POST", "/b", &[], &body).status, 200);
    }
    assert!(inbox.held_bytes() <= 20_000, "{}", inbox.held_bytes());
    let status = inbox.status();
    assert!(status.dropped >= 7 && status.held <= 5, "{status:?}");
    inbox.stop();
}

#[test]
fn events_since_wait_and_clear() {
    let (inbox, port) = started(loose());
    let first = inbox.events(0);
    assert!(matches!(first.events[0], InboxEvent::Started { port: p } if p == port));
    call(port, "POST", "/a", &[], "1");
    call(port, "POST", "/b", &[], "2");
    let after = inbox.events(first.next);
    assert_eq!(after.events.len(), 2);
    assert_eq!(inbox.events(after.next).events.len(), 0);
    // `wait` returns as soon as something arrives.
    let waiter = {
        let inbox = inbox.clone();
        let next = after.next;
        std::thread::spawn(move || inbox.wait(next, Duration::from_secs(5)))
    };
    std::thread::sleep(Duration::from_millis(50));
    call(port, "POST", "/c", &[], "3");
    assert_eq!(waiter.join().unwrap().events.len(), 1);
    inbox.clear();
    let status = inbox.status();
    assert_eq!(status.held, 0);
    assert!(requests(&inbox).is_empty());
    assert!(matches!(inbox.events(0).events.last(), Some(InboxEvent::Cleared)));
    // Ids go on counting.
    call(port, "POST", "/d", &[], "4");
    assert_eq!(requests(&inbox)[0].id, 4);
    inbox.stop();
}

#[test]
fn start_stop_and_a_port_in_use() {
    let (inbox, port) = started(loose());
    assert_eq!(inbox.start().unwrap().port, Some(port), "starting twice is harmless");
    let other = Inbox::new();
    other
        .set_settings(InboxSettings {
            port,
            ..InboxSettings::default()
        })
        .unwrap();
    let err = other.start().unwrap_err().to_string();
    assert!(err.contains("in use"), "{err}");
    assert!(!other.status().running);
    let stopped = inbox.stop();
    assert!(!stopped.running);
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err(), "the port is free again");
    // After a stop it starts again; what was captured stays.
    call_after_restart(&inbox);
}

fn call_after_restart(inbox: &Inbox) {
    let port = inbox.start().unwrap().port.unwrap();
    assert_eq!(call(port, "GET", "/again", &[], "").status, 200);
    assert_eq!(requests(inbox).len(), 1);
    inbox.stop();
}

/// Files below `dir` changed since `since` and containing the canary (depth 3, small files).
fn files_with_canary(dir: &std::path::Path, since: std::time::SystemTime, depth: u8, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                files_with_canary(&path, since, depth - 1, out);
            }
        } else if meta.len() < 4 * 1024 * 1024
            && meta.modified().is_ok_and(|m| m >= since)
            && std::fs::read(&path).is_ok_and(|b| b.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()))
        {
            out.push(path.display().to_string());
        }
    }
}

#[test]
fn nothing_goes_to_a_file() {
    let since = std::time::SystemTime::now() - Duration::from_secs(1);
    let (inbox, port) = started(loose());
    call(
        port,
        "POST",
        &format!("/hook?token={CANARY}"),
        &[("Authorization", &format!("Bearer {CANARY}"))],
        &format!("{{\"password\":\"{CANARY}\"}}"),
    );
    assert_eq!(requests(&inbox).len(), 1);
    inbox.stop();
    let mut found = Vec::new();
    files_with_canary(&std::env::temp_dir(), since, 3, &mut found);
    if let Some(data) = rt_plugin::host::dir::data_dir() {
        files_with_canary(&data, since, 4, &mut found);
    }
    files_with_canary(&std::env::current_dir().unwrap(), since, 2, &mut found);
    assert!(found.is_empty(), "the canary reached a file: {found:?}");
}

#[test]
fn make_request_replays_against_base_url_and_turns_secrets_into_variables() {
    let (inbox, port) = started(loose());
    let body = format!(
        "{{\"order\":42,\"password\":\"{CANARY}-p\",\"nested\":{{\"api_key\":\"{CANARY}-k\"}},\"note\":\"plain\"}}"
    );
    call(
        port,
        "POST",
        &format!("/hooks/orders?x=1&api_key={CANARY}-q&token={CANARY}-t"),
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {CANARY}-a")),
            ("Cookie", &format!("sid={CANARY}-c")),
            ("X-Hub-Signature-256", &format!("sha256={CANARY}-s")),
            ("X-Api-Key", &format!("{CANARY}-h")),
            ("X-Event", "order.created"),
            ("Connection", "keep-alive"),
            ("Accept-Encoding", "gzip"),
        ],
        &body,
    );
    let draft = inbox.make_request(1).unwrap();
    let json = serde_json::to_string(&draft).unwrap();
    assert!(!json.contains(CANARY), "no secret value in the draft: {json}");
    assert_eq!(draft.spec.method, "POST");
    assert_eq!(draft.name, "POST /hooks/orders");
    assert!(
        draft.spec.url.starts_with("{{baseUrl}}/hooks/orders?x=1&api_key={{api_key}}&token={{token}}"),
        "{}",
        draft.spec.url
    );
    let names: Vec<&str> = draft.spec.headers.iter().map(|h| h.name.as_str()).collect();
    assert!(names.contains(&"Content-Type") && names.contains(&"X-Event"));
    for gone in ["Host", "Content-Length", "Connection", "Accept-Encoding"] {
        assert!(!names.contains(&gone), "{gone} is not replayed");
    }
    let value = |n: &str| draft.spec.headers.iter().find(|h| h.name == n).unwrap().value.clone();
    assert_eq!(value("Authorization"), "{{authorization}}");
    assert_eq!(value("Cookie"), "{{cookie}}");
    assert_eq!(value("X-Hub-Signature-256"), "{{x_hub_signature_256}}");
    assert_eq!(value("X-Api-Key"), "{{x_api_key}}");
    assert_eq!(value("X-Event"), "order.created");
    let Body::Raw { data, .. } = &draft.spec.body else {
        panic!("a raw body: {:?}", draft.spec.body)
    };
    assert!(data.contains("\"order\":42") && data.contains("\"note\":\"plain\""));
    assert!(data.contains("{{password}}") && data.contains("{{api_key}}"));
    for want in ["authorization", "cookie", "x_hub_signature_256", "x_api_key", "api_key", "token", "password"] {
        assert!(draft.secrets.contains(&want.to_owned()), "{want} in {:?}", draft.secrets);
    }
    assert!(inbox.make_request(99).is_err());
    inbox.stop();
}

#[test]
fn the_scanner_catches_credentials_in_ordinary_headers_and_bodies() {
    let (inbox, port) = started(loose());
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJyZWQifQ.c2lnbmF0dXJlLXZhbHVlLTEyMzQ1";
    call(
        port,
        "POST",
        "/x",
        &[("X-Forwarded-Auth", &format!("Bearer {jwt}")), ("Content-Type", "text/plain")],
        &format!("aws_secret_access_key: {CANARY}-wJalrXUtnFEMI/K7MDENG/bPxRfiCY"),
    );
    let draft = inbox.make_request(1).unwrap();
    let json = serde_json::to_string(&draft).unwrap();
    assert!(!json.contains(jwt), "{json}");
    assert!(!json.contains(CANARY), "{json}");
    inbox.stop();
}

#[test]
fn form_bodies_become_fields_with_secrets_as_variables() {
    let (inbox, port) = started(loose());
    call(
        port,
        "POST",
        "/f",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        &format!("a=1&client_secret={CANARY}&b=two+words"),
    );
    let draft = inbox.make_request(1).unwrap();
    let Body::FormUrlEncoded { fields } = &draft.spec.body else {
        panic!("form: {:?}", draft.spec.body)
    };
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[1].value, "{{client_secret}}");
    assert_eq!(fields[2].value, "two words");
    assert!(!serde_json::to_string(&draft).unwrap().contains(CANARY));
    inbox.stop();
}
