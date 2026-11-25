// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand E: the budget `inbox_latency`: from the last byte of a request arriving on
//! the loopback port to the event at a subscriber. Median 5 ms, p99 20 ms over 2 000 requests of
//! 2 KB; nothing is lost up to 200 requests per second; the buffer keeps its limits.
//! `node scripts/perf/run.mjs --only inbox` runs `inbox_latency_is_measured` in release mode.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

use rt_app::inbox::{Inbox, InboxEvent, InboxSettings, Limits, MAX_ENTRIES, MAX_TOTAL_BYTES};

const REQUESTS: usize = 2_000;
const MEDIAN_MS: f64 = 5.0;
const P99_MS: f64 = 20.0;

fn started() -> (Inbox, u16) {
    let inbox = Inbox::with_limits(Limits {
        per_second: 1_000_000,
        ..Limits::default()
    });
    inbox
        .set_settings(InboxSettings {
            port: 0,
            ..InboxSettings::default()
        })
        .unwrap();
    let port = inbox.start().unwrap().port.unwrap();
    (inbox, port)
}

fn request(port: u16, body: &str) -> Instant {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.set_nodelay(true).unwrap();
    let head = format!(
        "POST /hooks/perf HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    let sent = Instant::now();
    s.write_all(body.as_bytes()).unwrap();
    let mut answer = Vec::new();
    s.read_to_end(&mut answer).unwrap();
    sent
}

#[test]
fn inbox_latency_is_measured() {
    let (inbox, port) = started();
    let body = format!("{{\"data\":\"{}\"}}", "x".repeat(2_000));
    let mut next = inbox.events(0).next;
    let mut ms = Vec::with_capacity(REQUESTS);
    for _ in 0..REQUESTS {
        let subscriber = {
            let inbox = inbox.clone();
            std::thread::spawn(move || {
                let page = inbox.wait(next, Duration::from_secs(5));
                (Instant::now(), page)
            })
        };
        let sent = request(port, &body);
        let (arrived, page) = subscriber.join().unwrap();
        assert!(page.events.iter().any(|e| matches!(e, InboxEvent::Request { .. })));
        next = page.next;
        // The event may have come before the answer was read; the interval starts at the last byte.
        ms.push(arrived.saturating_duration_since(sent).as_secs_f64() * 1000.0);
    }
    ms.sort_by(f64::total_cmp);
    let median = ms[ms.len() / 2];
    let p99 = ms[ms.len() * 99 / 100];
    println!("inbox_latency median {median:.3} ms, p99 {p99:.3} ms");
    if let Ok(dir) = std::env::var("RT_PERF_OUT") {
        std::fs::write(
            std::path::Path::new(&dir).join("inbox.json"),
            format!(
                "{{\"inbox_latency\":{median:.3},\"p99\":{p99:.3},\"requests\":{REQUESTS}}}"
            ),
        )
        .unwrap();
    }
    assert!(median <= MEDIAN_MS, "median {median:.3} ms over {MEDIAN_MS} ms");
    assert!(p99 <= P99_MS, "p99 {p99:.3} ms over {P99_MS} ms");
    // The buffer kept its limits all along.
    let status = inbox.status();
    assert_eq!(status.held, MAX_ENTRIES as u64);
    assert_eq!(status.dropped, (REQUESTS - MAX_ENTRIES) as u64);
    assert!(inbox.held_bytes() <= MAX_TOTAL_BYTES);
    inbox.stop();
}
