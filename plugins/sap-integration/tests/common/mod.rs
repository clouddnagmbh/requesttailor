// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A synthetic Integration Suite tenant on loopback, written for this repository (no SAP
//! content): an OAuth token endpoint, the iFlow endpoint of the runtime and the message
//! processing log API. Three listeners, like the three hosts of a real tenant.
//!
//! - token host `48820`: `POST /oauth/token` (client credentials, Basic auth or form fields).
//!   Two clients: the API client gets `api-token`, the runtime client `rt-token`.
//! - runtime `48819`: `POST /http/orders` needs `rt-token`. A body containing `fail` makes
//!   the flow fail (500, log status `FAILED`), one containing `slow` leaves the log in
//!   `PROCESSING` for its first read. Every answer carries `SAP_MessageProcessingLogID`.
//! - API `48818`: `/api/v1/MessageProcessingLogs...` needs `api-token`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

pub const API_PORT: u16 = 48818;
pub const RUNTIME_PORT: u16 = 48819;
pub const TOKEN_PORT: u16 = 48820;

pub const API_CLIENT: &str = "sb-rt-api!b1001";
pub const RUNTIME_CLIENT: &str = "sb-rt-flow!b1002";
/// What the service-key fixtures call their secrets: never allowed to leave the app.
pub const API_SECRET: &str = "RT-CANARY-api-secret-7f3a";
pub const RUNTIME_SECRET: &str = "RT-CANARY-runtime-secret-91c4";

#[derive(Debug, Clone)]
pub struct Seen {
    pub port: u16,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone)]
struct Log {
    guid: String,
    flow: String,
    status: String,
    start: i64,
    end: Option<i64>,
    error: Option<String>,
    /// Reads that still answer `PROCESSING` before the real status shows.
    processing_reads: u32,
}

#[derive(Default)]
struct State {
    logs: Vec<Log>,
    seen: Vec<Seen>,
    counter: u32,
}

#[derive(Clone, Default)]
pub struct Tenant(Arc<Mutex<State>>);

impl Tenant {
    pub fn seen(&self) -> Vec<Seen> {
        self.0.lock().unwrap().seen.clone()
    }

    /// Everything the tenant saw, as one string (to look for secrets).
    pub fn seen_text(&self) -> String {
        format!("{:?}", self.seen())
    }
}

/// Starts the three listeners once per process.
pub fn start() -> Tenant {
    static STARTED: Mutex<Option<Tenant>> = Mutex::new(None);
    let mut started = STARTED.lock().unwrap();
    if let Some(t) = &*started {
        return t.clone();
    }
    let tenant = Tenant::default();
    for port in [API_PORT, RUNTIME_PORT, TOKEN_PORT] {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("port is free");
        let tenant = tenant.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tenant = tenant.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, port, &tenant);
                });
            }
        });
    }
    *started = Some(tenant.clone());
    tenant
}

fn serve(mut stream: TcpStream, port: u16, tenant: &Tenant) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let seen = Seen {
        port,
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    let reply = {
        let mut state = tenant.0.lock().unwrap();
        state.seen.push(seen.clone());
        match port {
            TOKEN_PORT => token(&seen),
            RUNTIME_PORT => runtime(&seen, &mut state),
            _ => api(&seen, &mut state),
        }
    };
    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.content_type,
        reply.body.len()
    );
    for (n, v) in &reply.headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(reply.body.as_bytes())?;
    Ok(())
}

struct Reply {
    status: &'static str,
    content_type: &'static str,
    headers: Vec<(String, String)>,
    body: String,
}

fn reply(status: &'static str, content_type: &'static str, body: impl Into<String>) -> Reply {
    Reply {
        status,
        content_type,
        headers: Vec::new(),
        body: body.into(),
    }
}

fn json(status: &'static str, body: impl Into<String>) -> Reply {
    reply(status, "application/json", body)
}

fn basic(seen: &Seen) -> Option<(String, String)> {
    let value = seen.header("Authorization")?.strip_prefix("Basic ")?;
    let bytes = decode64(value)?;
    let text = String::from_utf8(bytes).ok()?;
    let (id, secret) = text.split_once(':')?;
    Some((decode_form(id), decode_form(secret)))
}

fn decode_form(text: &str) -> String {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(body: &str, name: &str) -> Option<String> {
    body.split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| decode_form(v))
}

fn decode64(text: &str) -> Option<Vec<u8>> {
    let table = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | table(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn token(seen: &Seen) -> Reply {
    if seen.method != "POST" || !seen.path.starts_with("/oauth/token") {
        return json("404 Not Found", "{}");
    }
    let (id, secret) = basic(seen).unwrap_or_else(|| {
        (
            form(&seen.body, "client_id").unwrap_or_default(),
            form(&seen.body, "client_secret").unwrap_or_default(),
        )
    });
    if form(&seen.body, "grant_type").as_deref() != Some("client_credentials") {
        return json("400 Bad Request", r#"{"error":"unsupported_grant_type"}"#);
    }
    let access = match (id.as_str(), secret.as_str()) {
        (API_CLIENT, API_SECRET) => "api-token",
        (RUNTIME_CLIENT, RUNTIME_SECRET) => "rt-token",
        _ => {
            return json(
                "401 Unauthorized",
                r#"{"error":"invalid_client","error_description":"Bad credentials"}"#,
            );
        }
    };
    json(
        "200 OK",
        format!(r#"{{"access_token":"{access}","token_type":"bearer","expires_in":3600}}"#),
    )
}

fn bearer(seen: &Seen, token: &str) -> bool {
    seen.header("Authorization") == Some(&format!("Bearer {token}"))
}

const T0: i64 = 1_764_000_000_000;

fn runtime(seen: &Seen, state: &mut State) -> Reply {
    if !bearer(seen, "rt-token") {
        return json("401 Unauthorized", r#"{"error":"unauthorized"}"#);
    }
    if seen.method != "POST" || seen.path != "/http/orders" {
        return json("404 Not Found", r#"{"error":"no such endpoint"}"#);
    }
    state.counter += 1;
    let guid = format!("AGvDm3x{:04}", state.counter);
    let failing = seen.body.contains("fail");
    let slow = seen.body.contains("slow");
    state.logs.push(Log {
        guid: guid.clone(),
        flow: "Orders_Flow".into(),
        status: if failing { "FAILED" } else { "COMPLETED" }.into(),
        start: T0 + i64::from(state.counter) * 10_000,
        end: Some(T0 + i64::from(state.counter) * 10_000 + 2_500),
        error: failing.then(|| {
            "com.sap.gateway.core.ip.component.odata.exception.OsciException: \
             Order 4711 rejected, step Request_Reply_1"
                .to_owned()
        }),
        processing_reads: u32::from(slow),
    });
    let mut r = if failing {
        reply(
            "500 Internal Server Error",
            "text/plain",
            "Order 4711 rejected",
        )
    } else {
        json("200 OK", r#"{"accepted":true}"#)
    };
    r.headers.push(("SAP_MessageProcessingLogID".into(), guid));
    r
}

fn entry_json(log: &mut Log) -> String {
    let status = if log.processing_reads > 0 {
        log.processing_reads -= 1;
        "PROCESSING"
    } else {
        log.status.as_str()
    };
    let end = if status == "PROCESSING" {
        "null".to_owned()
    } else {
        log.end
            .map_or("null".into(), |e| format!("\"/Date({e})/\""))
    };
    format!(
        "{{\"MessageGuid\":\"{}\",\"CorrelationId\":\"CID-{}\",\"LogStart\":\"/Date({})/\",\"LogEnd\":{end},\"Sender\":\"Shop\",\"Receiver\":\"ERP\",\"IntegrationFlowName\":\"{}\",\"Status\":\"{status}\"}}",
        log.guid, log.guid, log.start, log.flow
    )
}

fn api(seen: &Seen, state: &mut State) -> Reply {
    if !bearer(seen, "api-token") {
        return json("401 Unauthorized", r#"{"error":"unauthorized"}"#);
    }
    let path = seen.path.split('?').next().unwrap_or("");
    let query = seen.path.split_once('?').map_or("", |(_, q)| q);
    let rest = path.strip_prefix("/api/v1/MessageProcessingLogs");
    let Some(rest) = rest else {
        return json("404 Not Found", "{}");
    };
    if rest.is_empty() {
        let decoded = decode_form(query);
        let failed_only = decoded.contains("Status eq 'FAILED'");
        let mut entries: Vec<String> = state
            .logs
            .iter_mut()
            .rev()
            .filter(|l| !failed_only || l.status == "FAILED")
            .map(entry_json)
            .collect();
        entries.truncate(5);
        return json(
            "200 OK",
            format!("{{\"d\":{{\"results\":[{}]}}}}", entries.join(",")),
        );
    }
    let Some(inner) = rest.strip_prefix("('") else {
        return json("404 Not Found", "{}");
    };
    let Some((guid, tail)) = inner.split_once("')") else {
        return json("404 Not Found", "{}");
    };
    let by_guid: HashMap<String, usize> = state
        .logs
        .iter()
        .enumerate()
        .map(|(i, l)| (l.guid.clone(), i))
        .collect();
    let Some(&i) = by_guid.get(guid) else {
        return json(
            "404 Not Found",
            r#"{"error":{"code":"Not Found","message":{"value":"Message processing log not found"}}}"#,
        );
    };
    match tail {
        "" => json(
            "200 OK",
            format!("{{\"d\":{}}}", entry_json(&mut state.logs[i])),
        ),
        "/ErrorInformation/$value" => match &state.logs[i].error {
            Some(text) => reply("200 OK", "text/plain", text.clone()),
            None => reply("404 Not Found", "text/plain", "no error information"),
        },
        _ => json("404 Not Found", "{}"),
    }
}

// -------------------------------------------------------------------------------------------
// Fixtures and reading what the importer wrote

use rt_plugin_sap_integration::Integration;
use rt_plugin_sdk::api::{Call, ImportReply, PluginFile};
use rt_plugin_sdk::model::{Collection, Environment, Item, RequestSpec};
use rt_plugin_sdk::testing;

pub fn fixture(name: &str) -> PluginFile {
    let path = format!("{}/testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    rt_plugin_sdk::text_file(name, &std::fs::read_to_string(path).unwrap())
}

pub fn import(files: Vec<PluginFile>) -> Result<ImportReply, String> {
    testing::call(
        &Integration,
        &Call::Import {
            importer: "service-key".into(),
            files,
        },
    )
}

pub fn file_set(reply: &ImportReply) -> rt_format::FileSet {
    rt_format::FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    }
}

/// The reply read the way the host reads it; no issues allowed.
pub fn read(reply: &ImportReply) -> Collection {
    let files = file_set(reply);
    assert_eq!(files.files.len(), reply.files.len(), "paths are unique");
    let doc = rt_format::read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    doc.model().clone()
}

pub fn request(collection: &Collection, name: &str) -> (String, RequestSpec) {
    fn find(items: &[Item], name: &str) -> Option<(String, RequestSpec)> {
        items.iter().find_map(|i| match i {
            Item::Request(r) if r.spec.name == name => Some((r.path.clone(), r.spec.clone())),
            Item::Folder(f) => find(&f.items, name),
            _ => None,
        })
    }
    find(&collection.items, name).unwrap_or_else(|| panic!("no request {name}"))
}

pub fn environment(collection: &Collection) -> Environment {
    collection
        .environments
        .first()
        .expect("an environment")
        .clone()
}
