// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gate (2) of wave 8, strand E: no secret value appears in a model call.
//!
//! A collection whose environment, request URL, headers and body, OAuth 2.0 token and
//! response all carry secret values is sent for real (native transport) to a local server
//! that echoes everything back, secrets included, in plain, URL-encoded and base64 form.
//! Then all three AI tasks run against a fake provider endpoint on the same server, for both
//! providers; the raw bytes of every model call are recorded and searched for every secret
//! value in every form a value can reasonably take on the wire.
//!
//! Native only: it binds a local port (port 0) and sends through the native transport.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use rt_app::ai::{self, AiAnswer, AiConfig, AiContext, AiProviderKind, AiTask};
use rt_app::auth::{MemorySecrets, SecretTokens};
use rt_app::{ResponseStore, SendInput, Workspace};
use rt_core::model::ScriptPhase;
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::native::NativeTransport;
use rt_format::{COLLECTION_FILE, FileSet};

const API_KEY: &str = "kc-API/key+value=1-7f3a9";
const PASSWORD: &str = "p@ss w\"rd&x=1-keychain";
const CLIENT_SECRET: &str = "client-secret-3c1d2e";
const OAUTH_TOKEN: &str = "oauth-access-token-9b8a7c6d";
const REFRESH_TOKEN: &str = "oauth-refresh-token-5e4f";
const TRACE: &str = "trace-0123456789abcdef";
const SESSION: &str = "sess-runtime-41d";
/// The provider key goes in its header only, never in a body.
const PROVIDER_KEY: &str = "sk-gate-provider-key-000";

const ALL_SECRETS: &[&str] = &[
    API_KEY,
    PASSWORD,
    CLIENT_SECRET,
    OAUTH_TOKEN,
    REFRESH_TOKEN,
    TRACE,
    SESSION,
];

fn percent(s: &str, plus: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' if plus => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Every form a value can take in a request: raw, JSON-escaped (once and twice),
/// percent-encoded, base64.
fn forms(secret: &str) -> Vec<String> {
    let json = serde_json::to_string(secret).unwrap();
    let json = json[1..json.len() - 1].to_owned();
    let json2 = serde_json::to_string(&json).unwrap();
    let json2 = json2[1..json2.len() - 1].to_owned();
    let b = secret.as_bytes();
    let e = &base64::engine::general_purpose::STANDARD;
    let u = &base64::engine::general_purpose::URL_SAFE_NO_PAD;
    vec![
        secret.to_owned(),
        json,
        json2,
        percent(secret, false),
        percent(secret, true),
        e.encode(b),
        e.encode(b).trim_end_matches('=').to_owned(),
        u.encode(b),
    ]
}

/// One HTTP/1.1 request read from `stream`: the raw bytes, the request line and the body.
fn read_request(stream: &mut TcpStream) -> Option<(Vec<u8>, String, Vec<u8>)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut raw = Vec::new();
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.is_empty() {
        return None;
    }
    let request_line = line.trim_end().to_owned();
    raw.extend_from_slice(line.as_bytes());
    let mut length = 0usize;
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        raw.extend_from_slice(line.as_bytes());
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    raw.extend_from_slice(&body);
    Some((raw, request_line, body))
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &str) {
    let mut out = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (n, v) in headers {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// The model's answers, as the provider would wrap them.
fn model_text(prompt: &str) -> &'static str {
    if prompt.contains("Task: write one HTTP request") {
        "```json\n{\"text\": \"Creates a user.\", \"request\": {\"name\": \"Create admin\", \"method\": \"POST\", \"url\": \"{{base}}/api/admins\", \"headers\": [{\"name\": \"X-Api-Key\", \"value\": \"{{apiKey}}\"}], \"body\": {\"type\": \"json\", \"data\": \"{\\\"name\\\": \\\"{{name}}\\\"}\"}}}\n```"
    } else if prompt.contains("Task: explain") {
        "```json\n{\"text\": \"The server answered **401**: the token was not accepted.\"}\n```"
    } else {
        "```json\n{\"text\": \"Checks the status.\", \"tests\": \"test(\\\"created\\\", () => expect(rt.response.status).to.equal(201));\", \"assertions\": [{\"expression\": \"res.status\", \"operator\": \"eq\", \"value\": \"201\"}]}\n```"
    }
}

struct Server {
    port: u16,
    /// Raw bytes of every model call.
    calls: Arc<Mutex<Vec<Vec<u8>>>>,
}

/// Token endpoint, echo API and fake model endpoints on one local port.
fn serve() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&calls);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Some((raw, line, body)) = read_request(&mut stream) else {
                continue;
            };
            let target = line.split(' ').nth(1).unwrap_or("").to_owned();
            let json = [("Content-Type", "application/json".to_owned())];
            if target.starts_with("/token") {
                let token = serde_json::json!({
                    "access_token": OAUTH_TOKEN, "refresh_token": REFRESH_TOKEN,
                    "token_type": "Bearer", "expires_in": 3600,
                });
                respond(&mut stream, 200, &json, &token.to_string());
            } else if target.starts_with("/api/") {
                // Everything comes back: the request as received, and secrets in other forms.
                let text = String::from_utf8_lossy(&raw).into_owned();
                let auth = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                    .unwrap_or("")
                    .to_owned();
                let echo = serde_json::json!({
                    "error": "unauthorized",
                    "message": format!("key {API_KEY} rejected for {OAUTH_TOKEN}"),
                    "received": text,
                    "password_b64": base64::engine::general_purpose::STANDARD.encode(PASSWORD),
                    "key_url": percent(API_KEY, false),
                    "session": SESSION,
                });
                let headers = [
                    ("Content-Type", "application/json".to_owned()),
                    (
                        "X-Echo-Auth",
                        auth.replace("authorization:", "")
                            .replace("Authorization:", ""),
                    ),
                    ("X-Trace", TRACE.to_owned()),
                ];
                respond(&mut stream, 401, &headers, &echo.to_string());
            } else {
                recorded.lock().unwrap().push(raw);
                let prompt = String::from_utf8_lossy(&body).into_owned();
                let text = model_text(&prompt);
                let answer = if target.ends_with("/v1/messages") {
                    serde_json::json!({
                        "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": text }],
                        "stop_reason": "end_turn",
                    })
                } else {
                    serde_json::json!({
                        "choices": [{ "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
                    })
                };
                respond(&mut stream, 200, &json, &answer.to_string());
            }
        }
    });
    Server { port, calls }
}

fn collection(port: u16) -> FileSet {
    let files = [
        (
            COLLECTION_FILE.to_owned(),
            format!(
                "opencollection: \"1.0.0\"\ninfo:\n  name: gate\nrequest:\n  variables:\n    - name: base\n      value: http://127.0.0.1:{port}\n"
            ),
        ),
        (
            "users/folder.yml".to_owned(),
            "info:\n  name: Users\n  type: folder\n  seq: 1\n".to_owned(),
        ),
        (
            "users/create.yml".to_owned(),
            r#"info:
  name: Create user
  type: http
  seq: 1

http:
  method: POST
  url: "{{base}}/api/users?key={{apiKey}}&trace={{traceId}}"
  headers:
    - name: X-Api-Key
      value: "{{apiKey}}"
    - name: X-Session
      value: "{{sessionToken}}"
  body:
    type: json
    data: |-
      {"name": "ann", "password": "{{password}}"}
  auth:
    type: oauth2
    flow: client_credentials
    accessTokenUrl: "{{base}}/token"
    credentials:
      clientId: cli
      clientSecret: "{{clientSecret}}"
      placement: body
    scope: read
"#
            .to_owned(),
        ),
        (
            "users/list.yml".to_owned(),
            "info:\n  name: List users\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/api/users\"\n  headers:\n    - name: X-Api-Key\n      value: \"{{apiKey}}\"\n".to_owned(),
        ),
        (
            "environments/Local.yml".to_owned(),
            "name: Local\nvariables:\n  - name: host\n    value: local\n  - secret: true\n    name: apiKey\n  - secret: true\n    name: password\n  - secret: true\n    name: clientSecret\n".to_owned(),
        ),
    ];
    FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p, t.into_bytes()))
            .collect(),
    }
}

fn input() -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        // `--var`-like runtime values (and session variables) the user typed.
        vars: vec![
            ("traceId".into(), TRACE.into()),
            ("sessionToken".into(), SESSION.into()),
        ],
        // Keychain values of the environment's secret variables.
        secrets: vec![
            ("apiKey".into(), API_KEY.into()),
            ("password".into(), PASSWORD.into()),
            ("clientSecret".into(), CLIENT_SECRET.into()),
        ],
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn no_secret_value_reaches_a_model() {
    let server = serve();
    let rt = runtime();
    let transport = NativeTransport::new().unwrap();
    let ws = Workspace::open(collection(server.port)).unwrap();
    let tokens = SecretTokens::new(Arc::new(MemorySecrets::default()), "gate");

    // The send, as the app does it: OAuth token fetched and stored, secrets injected.
    let snapshot = ws.snapshot("users/create.yml", input()).unwrap();
    let services = Services {
        tokens: Some(&tokens),
        ..Services::none()
    };
    let report = rt.block_on(snapshot.send_with(
        &transport,
        &SendOptions {
            body_limit: 1024 * 1024,
            iteration: None,
        },
        &services,
    ));
    let response = report.response.as_ref().expect("a response");
    assert_eq!(response.status, 401);
    let mut store = ResponseStore::new();
    let stored = store.insert(report);
    let id = stored.response_id.unwrap();
    // The precondition: the response the assistant reads does carry the secrets.
    let preview = serde_json::to_string(&store.report(id).unwrap()).unwrap();
    for s in [API_KEY, OAUTH_TOKEN, SESSION, TRACE] {
        assert!(
            forms(s).iter().any(|f| preview.contains(f.as_str())),
            "the echo should carry {s}"
        );
    }

    // What the shells pass to `run`: every secret in scope plus the stored tokens.
    let mut secrets = ai::secrets_in_scope(ws.collection(), &input());
    secrets.extend(ai::token_values(&tokens).unwrap());
    for s in ALL_SECRETS {
        assert!(secrets.iter().any(|x| x == s), "{s} not in scope");
    }

    let tasks = [
        AiTask::CreateRequest {
            description: format!("Create an admin like the others; my key is {API_KEY}"),
            folder: "users".into(),
        },
        AiTask::ExplainError {
            path: "users/create.yml".into(),
            response: id,
        },
        AiTask::SuggestTests {
            path: "users/create.yml".into(),
            response: id,
        },
    ];
    let configs = [
        AiConfig {
            provider: AiProviderKind::Anthropic,
            model: ai::DEFAULT_ANTHROPIC_MODEL.into(),
            base_url: Some(format!("http://127.0.0.1:{}", server.port)),
        },
        AiConfig {
            provider: AiProviderKind::OpenaiCompatible,
            model: "llama3.1".into(),
            base_url: Some(format!("http://127.0.0.1:{}/v1", server.port)),
        },
    ];
    let mut answers: Vec<AiAnswer> = Vec::new();
    for config in &configs {
        let provider = ai::provider(&transport, config, Some(PROVIDER_KEY.into()), false).unwrap();
        for task in &tasks {
            let context = AiContext::new(&ws, &store, task, &input()).unwrap();
            answers.push(
                rt.block_on(ai::run(task, &context, &*provider, &secrets))
                    .unwrap(),
            );
        }
    }

    let calls = server.calls.lock().unwrap();
    assert_eq!(calls.len(), 6, "three tasks for each of the two providers");
    for raw in calls.iter() {
        let text = String::from_utf8_lossy(raw);
        for secret in ALL_SECRETS {
            for form in forms(secret) {
                assert!(
                    !text.contains(form.as_str()),
                    "secret {secret:?} (as {form:?}) reached the model:\n{text}"
                );
            }
        }
        let (_, body) = text.split_once("\r\n\r\n").unwrap();
        assert!(
            !body.contains(PROVIDER_KEY),
            "the key is in the body:\n{body}"
        );
        // The context did arrive, masked: the folder's requests, the response, its status.
        assert!(body.contains("***"), "{body}");
    }
    let explain = String::from_utf8_lossy(&calls[1]);
    assert!(
        explain.contains("401") && explain.contains("unauthorized"),
        "{explain}"
    );
    let create = String::from_utf8_lossy(&calls[0]);
    assert!(
        create.contains("List users") && create.contains("{{apiKey}}"),
        "{create}"
    );

    // The answers are proposals of the right shape.
    for pair in answers.chunks(3) {
        let request = pair[0].request.as_ref().unwrap();
        assert_eq!(
            (request.method.as_str(), request.url.as_str()),
            ("POST", "{{base}}/api/admins")
        );
        assert!(pair[1].text.contains("401"));
        assert_eq!(pair[2].scripts[0].phase, ScriptPhase::Tests);
        assert_eq!(pair[2].assertions[0].value, "201");
    }
    // Nothing was saved or changed by the assistant.
    assert!(ws.dirty().is_empty());
}
