// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The listener of the inbox (native only): a small HTTP/1.1 server on `127.0.0.1`, one thread
//! per connection, `Connection: close` on every answer. It never binds another address,
//! drops peers that are not loopback, sends no CORS header (a web page may throw a request in
//! but cannot read the answer), never echoes a capture, and writes nothing to disk.
//!
//! Limits (see [`Limits`]): the head up to 64 KiB, the body up to 2 MiB (`413`), 200 requests
//! per second as a token bucket (`429` with `Retry-After`), at most 64 connections at once,
//! 15 seconds per request.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use rt_core::inbox::{CapturedRequest, InboxPage, InboxSettings, InboxStatus, MAX_DELAY_MS};
use web_time::{Instant, SystemTime, UNIX_EPOCH};

use super::buffer::{Buffer, Limits};
use super::{InboxDraft, draft};
use crate::AppError;

const MAX_HEAD: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 64;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_DEADLINE: Duration = Duration::from_secs(15);
/// Longest body of the fixed answer.
const MAX_REPLY_BODY: usize = 64 * 1024;
const METHODS: [&str; 5] = ["POST", "PUT", "PATCH", "GET", "DELETE"];

/// Whether the inbox serves a peer: loopback only.
pub fn is_loopback_peer(peer: SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

struct Running {
    port: u16,
    generation: u64,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

struct Inner {
    settings: InboxSettings,
    buffer: Buffer,
    run: Option<Running>,
    generation: u64,
    tokens: f64,
    refilled: Instant,
}

struct Shared {
    inner: Mutex<Inner>,
    changed: Condvar,
    limits: Limits,
    connections: AtomicUsize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The webhook inbox: settings, the buffer of captured requests and, while started, the
/// listener. Cheap to clone (a handle).
#[derive(Clone)]
pub struct Inbox {
    shared: Arc<Shared>,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbox {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    /// An inbox with other limits (tests).
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    settings: InboxSettings::default(),
                    buffer: Buffer::new(limits),
                    run: None,
                    generation: 0,
                    tokens: f64::from(limits.per_second),
                    refilled: Instant::now(),
                }),
                changed: Condvar::new(),
                limits,
                connections: AtomicUsize::new(0),
            }),
        }
    }

    pub fn settings(&self) -> InboxSettings {
        self.shared.lock().settings.clone()
    }

    /// Replaces the settings. A running inbox answers with the new status and body from the next
    /// request on; a new port takes effect at the next start.
    pub fn set_settings(&self, settings: InboxSettings) -> Result<(), AppError> {
        validate(&settings)?;
        self.shared.lock().settings = settings;
        Ok(())
    }

    /// Starts listening on `127.0.0.1:<settings.port>` (`0`: a free port). Starting a running
    /// inbox returns its status.
    pub fn start(&self) -> Result<InboxStatus, AppError> {
        let mut inner = self.shared.lock();
        if inner.run.is_some() {
            return Ok(status_of(&inner));
        }
        let port = inner.settings.port;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
            AppError::Shell(if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("Port {port} is already in use. Pick another port.")
            } else {
                format!("Cannot listen on port {port}: {e}")
            })
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Shell(e.to_string()))?
            .port();
        inner.generation += 1;
        let generation = inner.generation;
        let stop = Arc::new(AtomicBool::new(false));
        let shared = self.shared.clone();
        let flag = stop.clone();
        let accept = std::thread::Builder::new()
            .name("rt-inbox".into())
            .spawn(move || accept_loop(&listener, &shared, &flag, generation))
            .map_err(|e| AppError::Shell(e.to_string()))?;
        inner.run = Some(Running {
            port,
            generation,
            stop,
            accept: Some(accept),
        });
        inner.buffer.started(port);
        let status = status_of(&inner);
        drop(inner);
        self.shared.changed.notify_all();
        Ok(status)
    }

    /// Stops listening (the captured requests stay until cleared or the process ends).
    pub fn stop(&self) -> InboxStatus {
        let run = self.shared.lock().run.take();
        if let Some(mut run) = run {
            run.stop.store(true, Ordering::SeqCst);
            // Wake the accept loop.
            let _ = TcpStream::connect_timeout(
                &SocketAddr::from((Ipv4Addr::LOCALHOST, run.port)),
                Duration::from_millis(500),
            );
            if let Some(handle) = run.accept.take() {
                let _ = handle.join();
            }
            self.shared.lock().buffer.stopped();
            self.shared.changed.notify_all();
        }
        self.status()
    }

    pub fn status(&self) -> InboxStatus {
        status_of(&self.shared.lock())
    }

    /// The events after `since` (`0`: all that are held).
    pub fn events(&self, since: u64) -> InboxPage {
        self.shared.lock().buffer.events(since)
    }

    /// Like [`Inbox::events`], but waits up to `timeout` for the first new event.
    pub fn wait(&self, since: u64, timeout: Duration) -> InboxPage {
        let guard = self.shared.lock();
        let (guard, _) = self
            .shared
            .changed
            .wait_timeout_while(guard, timeout, |i| i.buffer.seq() <= since)
            .unwrap_or_else(|e| e.into_inner());
        guard.buffer.events(since)
    }

    pub fn get(&self, id: u64) -> Option<CapturedRequest> {
        self.shared.lock().buffer.get(id)
    }

    pub fn clear(&self) {
        self.shared.lock().buffer.clear();
        self.shared.changed.notify_all();
    }

    /// The captured request as a request against `{{baseUrl}}` (see [`draft`]).
    pub fn make_request(&self, id: u64) -> Result<InboxDraft, AppError> {
        let request = self
            .get(id)
            .ok_or_else(|| AppError::Shell(format!("The request {id} is no longer held.")))?;
        Ok(draft(&request))
    }

    /// Bytes held (for tests of the limits).
    pub fn held_bytes(&self) -> usize {
        self.shared.lock().buffer.total_bytes()
    }
}

fn status_of(inner: &Inner) -> InboxStatus {
    inner.buffer.status(inner.run.as_ref().map(|r| r.port))
}

/// Settings the inbox accepts.
pub fn validate(settings: &InboxSettings) -> Result<(), AppError> {
    let bad = |m: &str| Err(AppError::Shell(m.to_owned()));
    if !(200..=599).contains(&settings.status) {
        return bad("The status must be between 200 and 599.");
    }
    if settings.delay_ms > MAX_DELAY_MS {
        return bad("The delay is at most 5 seconds.");
    }
    if settings.body.len() > MAX_REPLY_BODY {
        return bad("The answer body is at most 64 KiB.");
    }
    if settings
        .content_type
        .as_deref()
        .is_some_and(|c| c.chars().any(|ch| ch.is_control()))
    {
        return bad("The content type must be a single line.");
    }
    Ok(())
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>, stop: &AtomicBool, generation: u64) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        let allowed = stream.peer_addr().is_ok_and(is_loopback_peer);
        if !allowed || shared.connections.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            continue; // dropped: the peer sees a closed connection
        }
        shared.connections.fetch_add(1, Ordering::SeqCst);
        let shared = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("rt-inbox-conn".into())
            .spawn({
                let shared = shared.clone();
                move || {
                    let _ = serve(stream, &shared, generation);
                    shared.connections.fetch_sub(1, Ordering::SeqCst);
                }
            });
        if spawned.is_err() {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

struct Head {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

fn serve(mut stream: TcpStream, shared: &Shared, generation: u64) -> std::io::Result<()> {
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut buf = Vec::with_capacity(4096);
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return reply(&mut stream, 431, "Request Header Fields Too Large");
        }
        if !read_more(&mut stream, &mut buf, deadline)? {
            return Ok(());
        }
    };
    let Some(head) = parse_head(&buf[..head_end]) else {
        return reply(&mut stream, 400, "Bad Request");
    };
    let mut rest: Vec<u8> = buf[head_end + 4..].to_vec();

    if !METHODS.contains(&head.method.as_str()) {
        return reply(&mut stream, 405, "Method Not Allowed");
    }
    if !take_token(shared) {
        let _ = stream.write_all(
            b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return Ok(());
    }

    let max = shared.limits.max_body_bytes;
    let header = |name: &str| {
        head.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    let chunked = header("transfer-encoding")
        .iter()
        .any(|v| v.to_ascii_lowercase().contains("chunked"));
    let lengths = header("content-length");
    let length = match (lengths.as_slice(), chunked) {
        (_, true) => None,
        ([], false) => Some(0usize),
        ([one], false) => match one.trim().parse::<usize>() {
            Ok(n) => Some(n),
            Err(_) => return reply(&mut stream, 400, "Bad Request"),
        },
        _ => return reply(&mut stream, 400, "Bad Request"),
    };
    if length.is_some_and(|n| n > max) {
        return reply(&mut stream, 413, "Payload Too Large");
    }
    if header("expect")
        .iter()
        .any(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let body = match length {
        Some(n) => {
            while rest.len() < n {
                if !read_more(&mut stream, &mut rest, deadline)? {
                    return Ok(());
                }
            }
            rest.truncate(n);
            rest
        }
        None => match read_chunked(&mut stream, rest, max, deadline)? {
            Chunked::Body(b) => b,
            Chunked::TooLarge => return reply(&mut stream, 413, "Payload Too Large"),
            Chunked::Bad => return reply(&mut stream, 400, "Bad Request"),
        },
    };

    let size = body.len() as u64;
    let captured = CapturedRequest {
        id: 0,
        at: unix_ms(),
        method: head.method,
        target: head.target,
        headers: head.headers,
        body: String::from_utf8(body).ok(),
        size,
    };
    let settings = {
        let mut inner = shared.lock();
        if inner.run.as_ref().is_none_or(|r| r.generation != generation) {
            return Ok(()); // stopped meanwhile
        }
        inner.buffer.push(captured);
        inner.settings.clone()
    };
    shared.changed.notify_all();
    if settings.delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(u64::from(settings.delay_ms)));
    }
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        settings.status,
        reason(settings.status),
        settings.body.len()
    );
    if let Some(ct) = settings.content_type.as_deref().filter(|c| !c.is_empty()) {
        out.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(&settings.body);
    stream.write_all(out.as_bytes())
}

fn take_token(shared: &Shared) -> bool {
    let mut inner = shared.lock();
    let cap = f64::from(shared.limits.per_second);
    let now = Instant::now();
    let elapsed = now.duration_since(inner.refilled).as_secs_f64();
    inner.refilled = now;
    inner.tokens = (inner.tokens + elapsed * cap).min(cap);
    if inner.tokens >= 1.0 {
        inner.tokens -= 1.0;
        true
    } else {
        false
    }
}

fn reply(stream: &mut TcpStream, status: u16, text: &str) -> std::io::Result<()> {
    stream.write_all(
        format!("HTTP/1.1 {status} {text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
}

fn read_more(stream: &mut TcpStream, buf: &mut Vec<u8>, deadline: Instant) -> std::io::Result<bool> {
    if Instant::now() >= deadline {
        return Ok(false);
    }
    let mut chunk = [0u8; 16 * 1024];
    match stream.read(&mut chunk) {
        Ok(0) => Ok(false),
        Ok(n) => {
            buf.extend_from_slice(&chunk[..n]);
            Ok(true)
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(raw: &[u8]) -> Option<Head> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let version = first.next()?;
    if first.next().is_some() || !version.starts_with("HTTP/1.") || method.is_empty() {
        return None;
    }
    if !target.starts_with('/') || target.chars().any(|c| c.is_control()) {
        return None;
    }
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return None;
        }
        headers.push((name.to_owned(), value.trim().to_owned()));
    }
    Some(Head {
        method,
        target,
        headers,
    })
}

enum Chunked {
    Body(Vec<u8>),
    TooLarge,
    Bad,
}

fn read_chunked(
    stream: &mut TcpStream,
    mut buf: Vec<u8>,
    max: usize,
    deadline: Instant,
) -> std::io::Result<Chunked> {
    let mut body = Vec::new();
    let mut pos = 0;
    loop {
        // Chunk size line.
        let line_end = loop {
            if let Some(i) = find(&buf[pos..], b"\r\n") {
                break pos + i;
            }
            if buf.len() - pos > 1024 {
                return Ok(Chunked::Bad);
            }
            if !read_more(stream, &mut buf, deadline)? {
                return Ok(Chunked::Bad);
            }
        };
        let Ok(line) = std::str::from_utf8(&buf[pos..line_end]) else {
            return Ok(Chunked::Bad);
        };
        let Ok(size) = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
        else {
            return Ok(Chunked::Bad);
        };
        pos = line_end + 2;
        if size == 0 {
            return Ok(Chunked::Body(body)); // trailers are ignored
        }
        if body.len().saturating_add(size) > max {
            return Ok(Chunked::TooLarge);
        }
        while buf.len() < pos + size + 2 {
            if !read_more(stream, &mut buf, deadline)? {
                return Ok(Chunked::Bad);
            }
        }
        body.extend_from_slice(&buf[pos..pos + size]);
        pos += size + 2;
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}
