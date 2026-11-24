// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A tiny server for the watch tests: every request is answered with
//! `{"id": 1, "name": <name>, "echo": <request body>}`, waits `delay_ms` first, and the
//! arrival time and request line of each request are recorded.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Server {
    pub port: u16,
    pub name: Arc<Mutex<String>>,
    pub delay_ms: Arc<AtomicU64>,
    /// `(arrival, "GET /path")` of every request.
    pub seen: Arc<Mutex<Vec<(Instant, String)>>>,
}

pub fn spawn() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let name = Arc::new(Mutex::new("tea".to_owned()));
    let delay_ms = Arc::new(AtomicU64::new(0));
    let seen: Arc<Mutex<Vec<(Instant, String)>>> = Arc::default();
    let (n, d, s) = (name.clone(), delay_ms.clone(), seen.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let arrival = Instant::now();
            let (n, d, s) = (n.clone(), d.clone(), s.clone());
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 65536];
                let read = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..read]).into_owned();
                let line = head.lines().next().unwrap_or_default();
                let mut parts = line.split(' ');
                let method = parts.next().unwrap_or_default();
                let path = parts.next().unwrap_or_default();
                s.lock()
                    .unwrap()
                    .push((arrival, format!("{method} {path}")));
                let echo = head.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
                std::thread::sleep(std::time::Duration::from_millis(d.load(Ordering::SeqCst)));
                let body = serde_json::json!({
                    "id": 1,
                    "name": n.lock().unwrap().clone(),
                    "echo": echo,
                })
                .to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    Server {
        port,
        name,
        delay_ms,
        seen,
    }
}
