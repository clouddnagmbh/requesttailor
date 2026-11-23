// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The demo server on stdio and on a loopback port (native only): `rt mcp-demo` and the
//! desktop app's "Start on a local port".
//!
//! The HTTP server is deliberately small: HTTP/1.1, one request per connection
//! (`Connection: close`), bodies with `Content-Length` up to 4 MiB, a thread per
//! connection, on `127.0.0.1` only. That is all an MCP client needs and keeps the demo free
//! of dependencies.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

use super::Session;
use super::http::{HttpReply, HttpRequest, HttpServer, PATH};

/// Largest request body accepted.
const MAX_BODY: usize = 4 * 1024 * 1024;
/// Largest request head accepted.
const MAX_HEAD: usize = 64 * 1024;

/// Serves one session on `input`/`output` (newline-delimited JSON-RPC) until `input` ends.
/// Lines that are not JSON get a parse error with `id: null`.
pub fn serve_stdio(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut session = Session::new();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let messages = match serde_json::from_str::<Value>(&line) {
            Ok(message) => {
                let exchange = session.handle(&message);
                let mut out = exchange.notifications;
                out.extend(exchange.response);
                out
            }
            Err(e) => vec![json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") }
            })],
        };
        for m in messages {
            serde_json::to_writer(&mut output, &m)?;
            output.write_all(b"\n")?;
        }
        output.flush()?;
    }
    Ok(())
}

/// The demo server on a loopback port until [`stop`](Self::stop) or drop.
pub struct LocalServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    server: Arc<HttpServer>,
}

impl std::fmt::Debug for LocalServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl LocalServer {
    /// Listens on `127.0.0.1:port` (`0`: a free port).
    pub fn start(port: u16) -> io::Result<Self> {
        let listener =
            TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)))?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let server = Arc::new(HttpServer::new());
        let thread = {
            let stop = Arc::clone(&stop);
            let server = Arc::clone(&server);
            std::thread::Builder::new()
                .name("rt-mcp-demo".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let Ok(conn) = conn else { continue };
                        let server = Arc::clone(&server);
                        let _ = std::thread::Builder::new()
                            .name("rt-mcp-demo-conn".into())
                            .spawn(move || {
                                let _ = serve_connection(conn, &server);
                            });
                    }
                })?
        };
        Ok(Self {
            addr,
            stop,
            thread: Some(thread),
            server,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>/mcp`.
    pub fn url(&self) -> String {
        format!("http://{}{PATH}", self.addr)
    }

    /// Open MCP sessions.
    pub fn session_count(&self) -> usize {
        self.server.session_count()
    }

    /// Stops listening; connections being served finish on their own.
    pub fn stop(mut self) {
        self.shutdown();
    }

    /// Blocks until the server stops (never, unless the listener fails): `rt mcp-demo --http`.
    pub fn wait(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop.store(true, Ordering::SeqCst);
            // Wake the accept loop up so it sees the flag.
            let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
            let _ = thread.join();
        }
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve_connection(mut conn: TcpStream, server: &HttpServer) -> io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(30)))?;
    conn.set_write_timeout(Some(Duration::from_secs(30)))?;
    let reply = match read_request(&mut conn) {
        Ok(Some((method, path, headers, body))) => server.handle(&HttpRequest {
            method: &method,
            path: &path,
            headers: &headers,
            body: &body,
        }),
        Ok(None) => return Ok(()),
        Err(reply) => reply,
    };
    write_reply(&mut conn, &reply)
}

type Request = (String, String, Vec<(String, String)>, Vec<u8>);

/// Reads one request; `Ok(None)` when the peer closed without sending one, `Err` with the
/// reply for a request the server cannot take.
fn read_request(conn: &mut TcpStream) -> Result<Option<Request>, HttpReply> {
    let bad = |status: u16, text: &str| HttpReply {
        status,
        headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
        body: text.as_bytes().to_vec(),
    };
    let mut reader = BufReader::new(conn);
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = (&mut reader)
            .take((MAX_HEAD + 1 - head.len().min(MAX_HEAD)) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| bad(400, "Bad Request"))?;
        if n == 0 {
            return if head.is_empty() {
                Ok(None)
            } else {
                Err(bad(400, "Bad Request: incomplete head"))
            };
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            return Err(bad(431, "Request Header Fields Too Large"));
        }
    }
    let head = String::from_utf8(head).map_err(|_| bad(400, "Bad Request: head is not UTF-8"))?;
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(bad(400, "Bad Request: request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(bad(505, "HTTP Version Not Supported"));
    }
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let find = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    if find("transfer-encoding").is_some() {
        return Err(bad(411, "Length Required: send a Content-Length"));
    }
    let length = match find("content-length") {
        None => 0,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| bad(400, "Bad Request: Content-Length"))?,
    };
    if length > MAX_BODY {
        return Err(bad(413, "Content Too Large"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| bad(400, "Bad Request: body shorter than Content-Length"))?;
    Ok(Some((method.to_owned(), target.to_owned(), headers, body)))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

fn write_reply(conn: &mut TcpStream, reply: &HttpReply) -> io::Result<()> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (n, v) in &reply.headers {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        reply.body.len()
    ));
    conn.write_all(out.as_bytes())?;
    conn.write_all(&reply.body)?;
    conn.flush()
}
