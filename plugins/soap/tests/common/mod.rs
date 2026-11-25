// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A synthetic SOAP test service on loopback and a small "app" around the plugin: it resolves
//! `{{variables}}` the way the core does (the plugin only ever sees `***` for secrets), runs
//! the plugin's `prepare`, sends over a socket and runs `response`.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rt_plugin_sdk::api::{
    Call, NotesReply, PluginBody, PluginContext, PluginRequest, PluginResponse, PrepareReply,
};
use rt_plugin_sdk::model::{Body, RequestSpec};
use rt_plugin_sdk::testing;
use rt_plugin_soap::Soap;
use rt_plugin_soap::security::{expires, parse_iso};
use rt_plugin_soap::xml::{self, Element};

pub const USER: &str = "alice";
/// Never allowed to appear in anything the plugin returns.
pub const PASSWORD: &str = "canary-p4ss-8c1e";

/// What the service saw.
#[derive(Debug, Clone)]
pub struct Seen {
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

pub type Log = Arc<Mutex<Vec<Seen>>>;

/// Starts the service on `port` (a thread that lives as long as the test process).
pub fn start(port: u16) -> Log {
    static STARTED: Mutex<Vec<(u16, Log)>> = Mutex::new(Vec::new());
    let mut started = STARTED.lock().unwrap();
    if let Some((_, log)) = started.iter().find(|(p, _)| *p == port) {
        return log.clone();
    }
    let log = spawn(port);
    started.push((port, log.clone()));
    log
}

fn spawn(port: u16) -> Log {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("port is free");
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = seen.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, &seen);
            });
        }
    });
    log
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn serve(mut stream: TcpStream, log: &Log) -> std::io::Result<()> {
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
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
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
    let body = String::from_utf8_lossy(&body).into_owned();
    let seen = Seen {
        path: path.clone(),
        headers,
        body,
    };
    log.lock().unwrap().push(seen.clone());
    let (status, content_type, payload) = handle(&seen);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&payload)?;
    Ok(())
}

const ENV11: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const ENV12: &str = "http://www.w3.org/2003/05/soap-envelope";

fn handle(seen: &Seen) -> (String, String, Vec<u8>) {
    let v12 = seen.path.starts_with("/soap12");
    let ct = seen
        .header("Content-Type")
        .unwrap_or("")
        .to_ascii_lowercase();
    let wrong_ct = if v12 {
        !ct.starts_with("application/soap+xml")
    } else {
        !ct.starts_with("text/xml") || seen.header("SOAPAction").is_none()
    };
    let reply_ct = if v12 {
        "application/soap+xml; charset=utf-8"
    } else {
        "text/xml; charset=utf-8"
    };
    let fault = |client: bool, reason: &str, detail: &str| -> (String, String, Vec<u8>) {
        let body = if v12 {
            format!(
                "<env:Envelope xmlns:env=\"{ENV12}\"><env:Body><env:Fault><env:Code><env:Value>env:{}</env:Value></env:Code><env:Reason><env:Text xml:lang=\"en\">{reason}</env:Text></env:Reason><env:Detail>{detail}</env:Detail></env:Fault></env:Body></env:Envelope>",
                if client { "Sender" } else { "Receiver" }
            )
        } else {
            format!(
                "<env:Envelope xmlns:env=\"{ENV11}\"><env:Body><env:Fault><faultcode>env:{}</faultcode><faultstring>{reason}</faultstring><detail>{detail}</detail></env:Fault></env:Body></env:Envelope>",
                if client { "Client" } else { "Server" }
            )
        };
        (
            "500 Internal Server Error".into(),
            reply_ct.into(),
            body.into_bytes(),
        )
    };
    if wrong_ct {
        return fault(true, "wrong Content-Type or missing SOAPAction", "");
    }
    let Ok(root) = xml::parse(&seen.body) else {
        return fault(true, "the body is not XML", "");
    };
    let version_ns = if v12 { ENV12 } else { ENV11 };
    if root.ns != version_ns {
        return fault(true, "wrong SOAP version", "");
    }
    let Some(op) = root.child("Body").and_then(|b| b.children.first()) else {
        return fault(true, "empty body", "");
    };
    let num = |name: &str| -> i64 {
        op.child(name)
            .and_then(|e| e.text.trim().parse().ok())
            .unwrap_or(0)
    };
    let env = if v12 { ENV12 } else { ENV11 };
    let ok = |inner: &str| -> (String, String, Vec<u8>) {
        (
            "200 OK".into(),
            reply_ct.into(),
            format!(
                "<env:Envelope xmlns:env=\"{env}\"><env:Body>{inner}</env:Body></env:Envelope>"
            )
            .into_bytes(),
        )
    };
    if seen.path == "/secure" || seen.path == "/ts" {
        return secure(&seen.path, &root, op, &fault, &ok);
    }
    match op.name.as_str() {
        "Add" => ok(&format!(
            "<c:AddResponse xmlns:c=\"urn:requesttailor:test:calc\"><c:result>{}</c:result></c:AddResponse>",
            num("a") + num("b")
        )),
        "Divide" if num("b") == 0 => fault(
            true,
            "Division by zero",
            &format!("<info><operand>{}</operand></info>", num("a")),
        ),
        "Divide" => ok(&format!(
            "<c:DivideResponse xmlns:c=\"urn:requesttailor:test:calc\"><c:result>{}</c:result></c:DivideResponse>",
            num("a") as f64 / num("b") as f64
        )),
        other => fault(true, &format!("unknown operation {other}"), ""),
    }
}

type Reply = (String, String, Vec<u8>);

fn secure(
    path: &str,
    root: &Element,
    op: &Element,
    fault: &dyn Fn(bool, &str, &str) -> Reply,
    ok: &dyn Fn(&str) -> Reply,
) -> Reply {
    let sec = root
        .child("Header")
        .and_then(|h| h.children.iter().find(|c| c.name == "Security"));
    let Some(sec) = sec else {
        return fault(true, "wsse:InvalidSecurity: no security header", "");
    };
    let token = sec.child("UsernameToken");
    let user = token
        .and_then(|t| t.child("Username"))
        .map(|e| e.text.trim().to_owned());
    let pass = token
        .and_then(|t| t.child("Password"))
        .map(|e| e.text.trim().to_owned());
    if path == "/secure" && (user.as_deref() != Some(USER) || pass.as_deref() != Some(PASSWORD)) {
        return fault(
            true,
            "wsse:FailedAuthentication: bad username or password",
            "",
        );
    }
    if let Some(ts) = sec.child("Timestamp") {
        let expires = ts.child("Expires").and_then(|e| parse_iso(&e.text));
        if expires.is_none_or(|e| e < now_ms()) {
            return fault(true, "wsu:MessageExpired: the timestamp has expired", "");
        }
    }
    let name = op
        .child("name")
        .map(|e| e.text.trim().to_owned())
        .unwrap_or_default();
    ok(&format!(
        "<v:GetSecretResponse xmlns:v=\"urn:requesttailor:test:vault\"><v:value>value-of-{name}</v:value></v:GetSecretResponse>"
    ))
}

// -------------------------------------------------------------------------------------------
// The app around the plugin

/// A variable as the core knows it: secret ones are masked before a plugin sees the request.
pub struct Var(pub &'static str, pub String, pub bool);

fn substitute(text: &str, vars: &[Var], masked: bool) -> String {
    let mut out = text.to_owned();
    for Var(name, value, secret) in vars {
        let v = if masked && *secret { "***" } else { value };
        out = out.replace(&format!("{{{{{name}}}}}"), v);
    }
    out
}

pub struct Sent {
    pub request: PluginRequest,
    pub prepare: PrepareReply,
    pub response: PluginResponse,
    pub notes: NotesReply,
    /// Everything the plugin returned as JSON, to look for secrets.
    pub plugin_output: String,
}

/// Sends `spec` the way the app does: resolve variables, run `prepare` on the masked request,
/// apply the patch (headers; a body patch holding the mask would be refused), send, run
/// `response`.
pub fn send(spec: &RequestSpec, vars: &[Var]) -> Sent {
    let to_request = |masked: bool| PluginRequest {
        method: spec.method.clone(),
        url: substitute(&spec.url, vars, masked),
        headers: spec
            .headers
            .iter()
            .filter(|h| h.enabled)
            .map(|h| (h.name.clone(), substitute(&h.value, vars, masked)))
            .collect(),
        body: match &spec.body {
            Body::Raw { data, .. } => Some(PluginBody {
                content_type: None,
                text: Some(substitute(data, vars, masked)),
                base64: None,
            }),
            _ => None,
        },
    };
    let masked = to_request(true);
    let mut real = to_request(false);
    let call = Call::Prepare {
        request: masked,
        context: testing::context("soap/request.yml"),
    };
    let prepare: PrepareReply = testing::call(&Soap, &call).expect("prepare");
    let mut output = serde_json::to_string(&prepare).unwrap();
    let patch = &prepare.patch;
    assert!(
        patch
            .body
            .as_ref()
            .and_then(|b| b.text.as_deref())
            .is_none_or(|t| !t.contains("***")),
        "a patch must not carry the mask"
    );
    real.headers.retain(|(n, _)| {
        !patch
            .set_headers
            .iter()
            .any(|(s, _)| s.eq_ignore_ascii_case(n))
    });
    real.headers.extend(patch.set_headers.iter().cloned());
    if let Some(b) = &patch.body {
        // The core applies the plugin's body to the unmasked request only when the plugin saw
        // no secret (it cannot have changed one); here the patch is the whole new body.
        real.body = Some(b.clone());
    }
    let response = wire(&real);
    let call = Call::Response {
        request: real.clone(),
        response: response.clone(),
        context: testing::context("soap/request.yml"),
    };
    // The response hook gets the masked request too.
    let call = match call {
        Call::Response {
            response, context, ..
        } => Call::Response {
            request: to_request(true),
            response,
            context,
        },
        c => c,
    };
    let notes: NotesReply = testing::call(&Soap, &call).expect("response");
    output.push_str(&serde_json::to_string(&notes).unwrap());
    Sent {
        request: real,
        prepare,
        response,
        notes,
        plugin_output: output,
    }
}

/// One HTTP/1.1 request over a socket.
pub fn wire(request: &PluginRequest) -> PluginResponse {
    let url = request.url.strip_prefix("http://").expect("http url");
    let (hostport, path) = url
        .split_once('/')
        .map_or((url, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let body = request
        .body
        .as_ref()
        .and_then(|b| b.text.clone())
        .unwrap_or_default();
    let mut head = format!("{} {path} HTTP/1.1\r\nHost: {hostport}\r\n", request.method);
    for (n, v) in &request.headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut stream = TcpStream::connect(hostport).expect("service reachable");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let payload = raw[split + 4..].to_vec();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone());
    let body = match String::from_utf8(payload.clone()) {
        Ok(text) => PluginBody {
            content_type: ct,
            text: Some(text),
            base64: None,
        },
        Err(_) => rt_plugin_sdk::binary_body(ct.as_deref(), &payload),
    };
    PluginResponse {
        status,
        headers,
        url: request.url.clone(),
        body,
        truncated: false,
    }
}

pub fn context() -> PluginContext {
    testing::context("soap/request.yml")
}

pub fn expires_of(text: &str) -> Option<u64> {
    expires(text)
}

// -------------------------------------------------------------------------------------------
// Reading what the importer wrote

use rt_plugin_sdk::api::{ImportReply, PluginFile};
use rt_plugin_sdk::model::{Collection, Item};

pub fn testdata(name: &str) -> PluginFile {
    let path = format!("{}/testdata/{name}", env!("CARGO_MANIFEST_DIR"));
    rt_plugin_sdk::text_file(name, &std::fs::read_to_string(path).unwrap())
}

pub fn import(importer: &str, files: Vec<PluginFile>) -> ImportReply {
    testing::call(
        &Soap,
        &Call::Import {
            importer: importer.into(),
            files,
        },
    )
    .unwrap()
}

/// The reply read the way the host reads it; no issues allowed.
pub fn read(reply: &ImportReply) -> Collection {
    let files = rt_format::FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    };
    assert_eq!(files.files.len(), reply.files.len(), "paths are unique");
    let doc = rt_format::read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    doc.model().clone()
}

/// The request `name` in `folder`.
pub fn request(collection: &Collection, folder: &str, name: &str) -> RequestSpec {
    collection
        .items
        .iter()
        .find_map(|i| match i {
            Item::Folder(f) if f.name == folder => f.items.iter().find_map(|i| match i {
                Item::Request(r) if r.spec.name == name => Some(r.spec.clone()),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no request {folder}/{name}"))
}

pub fn body(spec: &RequestSpec) -> &str {
    match &spec.body {
        Body::Raw { data, .. } => data,
        other => panic!("body {other:?}"),
    }
}

pub fn header<'a>(spec: &'a RequestSpec, name: &str) -> Option<&'a str> {
    spec.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

pub fn body_of(response: &PluginResponse) -> String {
    response.body.text.clone().unwrap_or_default()
}
