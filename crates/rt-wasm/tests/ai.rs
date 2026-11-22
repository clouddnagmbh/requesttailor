// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant's masking in the browser build (wave 8, strand E): the same context,
//! prompt and masking code as natively (`crates/rt-app/tests/ai_gate.rs`), compiled to
//! WebAssembly. A provider records the exact bytes the Anthropic provider would send from
//! the browser (`fetch`, direct browser access header); no network is used.
//!
//! Run: `RT_CONFORMANCE_PORT=47897 scripts/conformance-wasm.sh -- --test ai`

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use rt_app::ai::{
    self, AiCall, AiConfig, AiContext, AiError, AiProvider, AiProviderKind, AiTask,
    AnthropicProvider,
};
use rt_app::{ResponseStore, SendInput, Workspace};
use rt_core::send::SendReport;
use rt_core::services::LocalBoxFuture;
use rt_core::transport::fetch::FetchTransport;
use rt_format::{COLLECTION_FILE, FileSet};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const API_KEY: &str = "kc-API/key+value=1-7f3a9";
const PASSWORD: &str = "p@ss w\"rd&x=1-keychain";
const TOKEN: &str = "oauth-access-token-9b8a7c6d";
const SESSION: &str = "sess-runtime-41d";

/// Builds the browser request for each call, keeps its bytes, answers with `answer`.
struct Recording<'t> {
    inner: AnthropicProvider<'t, FetchTransport>,
    sent: RefCell<Vec<String>>,
    answer: String,
}

impl AiProvider for Recording<'_> {
    fn complete<'a>(&'a self, call: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>> {
        let request = self.inner.request(&call);
        let mut raw = format!("{} {}\n", request.method, request.url);
        for (n, v) in &request.headers {
            raw.push_str(&format!("{n}: {v}\n"));
        }
        raw.push_str(&String::from_utf8_lossy(&request.body.unwrap().bytes));
        self.sent.borrow_mut().push(raw);
        let answer = self.answer.clone();
        Box::pin(async move { Ok(answer) })
    }
}

fn workspace() -> Workspace {
    let files = [
        (
            COLLECTION_FILE,
            "opencollection: \"1.0.0\"\ninfo:\n  name: gate\n",
        ),
        (
            "create.yml",
            "info:\n  name: Create\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: \"{{base}}/users?key={{apiKey}}\"\n  headers:\n    - name: X-Session\n      value: \"{{sessionToken}}\"\n  body:\n    type: json\n    data: |-\n      {\"password\": \"{{password}}\"}\n",
        ),
        (
            "environments/Local.yml",
            "name: Local\nvariables:\n  - name: base\n    value: https://api.example\n  - secret: true\n    name: apiKey\n  - secret: true\n    name: password\n",
        ),
    ];
    Workspace::open(FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p.to_owned(), t.as_bytes().to_vec()))
            .collect(),
    })
    .unwrap()
}

fn input() -> SendInput {
    SendInput {
        environment: Some("Local".into()),
        vars: vec![("sessionToken".into(), SESSION.into())],
        secrets: vec![
            ("apiKey".into(), API_KEY.into()),
            ("password".into(), PASSWORD.into()),
        ],
    }
}

/// A response that echoes the secrets, as a misbehaving server would.
fn report() -> SendReport {
    let body = serde_json::json!({
        "error": "unauthorized",
        "detail": format!("key {API_KEY} with token {TOKEN} for {SESSION}"),
        "password": PASSWORD,
    })
    .to_string();
    serde_json::from_value(serde_json::json!({
        "version": 1, "request_path": "create.yml", "environment": "Local",
        "request": { "method": "POST", "url": "https://api.example/users?key=***", "headers": [["Authorization", format!("Bearer {TOKEN}")]], "body_size": 10 },
        "response": {
            "status": 401, "version": "HTTP/1.1", "headers": [["Content-Type", "application/json"], ["X-Echo", API_KEY]],
            "url": "https://api.example/users", "redirects": [], "body": { "encoding": "utf8", "data": body },
            "size": 10, "truncated": false, "timing": { "dns": null, "connect": null, "tls": null, "ttfb": 1 }, "total": 2
        },
        "variables": [], "unresolved": [], "error": null
    }))
    .unwrap()
}

#[wasm_bindgen_test]
async fn no_secret_value_reaches_the_model_from_the_browser() {
    let ws = workspace();
    let mut store = ResponseStore::new();
    let id = store.insert(report()).response_id.unwrap();
    let mut secrets = ai::secrets_in_scope(ws.collection(), &input());
    secrets.push(TOKEN.into());
    let config = AiConfig {
        provider: AiProviderKind::Anthropic,
        model: ai::DEFAULT_ANTHROPIC_MODEL.into(),
        base_url: None,
    };
    let transport = FetchTransport::new();
    let provider = Recording {
        inner: AnthropicProvider::new(&transport, &config, Some("sk-browser".into()))
            .unwrap()
            .in_browser(),
        sent: RefCell::new(Vec::new()),
        answer: "```json\n{\"text\": \"401: the key was refused.\"}\n```".into(),
    };
    let tasks = [
        AiTask::CreateRequest {
            description: format!("like Create, key {API_KEY}"),
            folder: String::new(),
        },
        AiTask::ExplainError {
            path: "create.yml".into(),
            response: id,
        },
    ];
    for task in &tasks {
        let context = AiContext::new(&ws, &store, task, &input()).unwrap();
        let _ = ai::run(task, &context, &provider, &secrets).await;
    }
    let sent = provider.sent.borrow();
    assert_eq!(sent.len(), 2);
    for raw in sent.iter() {
        assert!(
            raw.starts_with("POST https://api.anthropic.com/v1/messages"),
            "{raw}"
        );
        assert!(
            raw.contains("anthropic-dangerous-direct-browser-access: true"),
            "{raw}"
        );
        for secret in [API_KEY, PASSWORD, TOKEN, SESSION] {
            let json = serde_json::to_string(secret).unwrap();
            for form in [secret, &json[1..json.len() - 1]] {
                assert!(!raw.contains(form), "{secret} reached the model:\n{raw}");
            }
        }
    }
    assert!(
        sent[1].contains("401") && sent[1].contains("unauthorized"),
        "{}",
        sent[1]
    );
}
