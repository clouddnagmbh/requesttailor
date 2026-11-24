// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A tiny JSON server for the snapshot tests: `GET /item` answers `{"id": 1, "name": <name>,
//! "stamp": <n>, "auth": <Authorization header>}` where `n` counts the requests (a volatile
//! value) and `name` can be changed by the test.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct Server {
    pub port: u16,
    pub name: Arc<Mutex<String>>,
}

pub fn spawn() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let name = Arc::new(Mutex::new("tea".to_owned()));
    let counter = Arc::new(AtomicUsize::new(100));
    let shared = name.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let auth = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .starts_with("authorization:")
                        .then(|| l.split_once(':').unwrap().1.trim().to_owned())
                })
                .unwrap_or_default();
            let body = serde_json::json!({
                "id": 1,
                "name": shared.lock().unwrap().clone(),
                "stamp": counter.fetch_add(1, Ordering::SeqCst),
                "auth": auth,
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ndate: Mon, 24 Nov 2026 10:00:00 GMT\r\nset-cookie: sid=zzz\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Server { port, name }
}
