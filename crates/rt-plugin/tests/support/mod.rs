// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Test plugins written in WAT, and fake host services that count what reaches them.
//!
//! Every fixture has the full ABI of `API.md` (`memory`, `rt_alloc`, `rt_dealloc`, `rt_call`,
//! import `rt.host`) and a manifest in the `rt-plugin` custom section. What `rt_call` does is
//! chosen with [`Body`].

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll};

use rt_core::services::{LocalBoxFuture, NoteLevel};
use rt_core::transport::{HttpRequest, HttpVersion, ResponseHead, Timing};
use rt_plugin::host::{HostResponse, HostServices};

/// What the fixture's `rt_call` does.
pub enum Body {
    /// Returns this reply, whatever the call.
    Fixed(String),
    /// Makes this host call (JSON) and returns the host's reply as its own reply.
    HostCall(String),
    /// Makes the host call `{"op":"storage-set","key":"seen","value":<the call>}`, so the
    /// test can read exactly what the plugin was given, and returns the host's reply.
    Spy,
    /// Loops forever (fuel).
    Loop,
    /// Traps.
    Trap,
    /// Grows its memory by this many 64 KiB pages; traps if that fails.
    Grow(u32),
    /// Like `HostCall(r#"{"op":"now"}"#)`, but `rt_dealloc` traps once it was called more
    /// than this many times (to check who frees which buffer).
    FreeAtMost(u32),
}

const A: u32 = 1024;
const B: u32 = 32768;

/// WAT string literal content for `bytes`.
fn wat_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'"' | b'\\' => out.push_str(&format!("\\{:02x}", b)),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{:02x}", b)),
        }
    }
    out
}

/// The WAT source of a fixture with `manifest` (JSON), `body` and `pages` of initial memory.
pub fn wat_source(manifest: &str, body: &Body, pages: u32) -> String {
    let (a, b): (&str, &str) = match body {
        Body::Fixed(reply) => (reply, ""),
        Body::HostCall(call) => (call, ""),
        Body::FreeAtMost(_) => (r#"{"op":"now"}"#, ""),
        Body::Spy => (r#"{"op":"storage-set","key":"seen","value":"#, "}"),
        _ => ("", ""),
    };
    let (la, lb) = (a.len() as u32, b.len() as u32);
    let call = match body {
        Body::Fixed(_) => format!("(i64.or (i64.shl (i64.const {A}) (i64.const 32)) (i64.const {la}))"),
        Body::HostCall(_) | Body::FreeAtMost(_) => {
            format!("(call $host (i32.const {A}) (i32.const {la}))")
        }
        Body::Spy => format!(
            "(local.set $buf (call $alloc (i32.add (i32.const {}) (local.get $len))))
             (memory.copy (local.get $buf) (i32.const {A}) (i32.const {la}))
             (memory.copy (i32.add (local.get $buf) (i32.const {la})) (local.get $ptr) (local.get $len))
             (memory.copy (i32.add (i32.add (local.get $buf) (i32.const {la})) (local.get $len))
                          (i32.const {B}) (i32.const {lb}))
             (call $host (local.get $buf) (i32.add (i32.const {}) (local.get $len)))",
            la + lb,
            la + lb
        ),
        Body::Loop => "(loop $l (br $l)) (unreachable)".to_owned(),
        Body::Trap => "(unreachable)".to_owned(),
        Body::Grow(n) => format!(
            "(if (i32.eq (memory.grow (i32.const {n})) (i32.const -1)) (then (unreachable))) (i64.const 0)"
        ),
    };
    let free_limit = match body {
        Body::FreeAtMost(n) => *n as i32,
        _ => -1,
    };
    format!(
        r#"(module
  (import "rt" "host" (func $host (param i32 i32) (result i64)))
  (memory (export "memory") {pages})
  (global $bump (mut i32) (i32.const 65536))
  (global $frees (mut i32) (i32.const 0))
  (data (i32.const {A}) "{a}")
  (data (i32.const {B}) "{b}")
  (func $alloc (export "rt_alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get $len)))
    (block $ok
      (loop $grow
        (br_if $ok (i32.le_u (global.get $bump) (i32.mul (memory.size) (i32.const 65536))))
        (if (i32.eq (memory.grow (i32.const 1)) (i32.const -1)) (then (unreachable)))
        (br $grow)))
    (local.get $p))
  (func (export "rt_dealloc") (param i32 i32)
    (global.set $frees (i32.add (global.get $frees) (i32.const 1)))
    (if (i32.gt_u (global.get $frees) (i32.const {free_limit})) (then (unreachable))))
  (func (export "rt_call") (param $ptr i32) (param $len i32) (result i64)
    (local $buf i32)
    {call})
  (@custom "rt-plugin" "{manifest}")
)"#,
        a = wat_string(a.as_bytes()),
        b = wat_string(b.as_bytes()),
        manifest = wat_string(manifest.as_bytes()),
    )
}

/// A plugin module with `manifest` (JSON) and `body`.
pub fn plugin(manifest: &serde_json::Value, body: Body) -> Vec<u8> {
    plugin_with_pages(manifest, body, 2)
}

pub fn plugin_with_pages(manifest: &serde_json::Value, body: Body, pages: u32) -> Vec<u8> {
    wat::parse_str(wat_source(&manifest.to_string(), &body, pages)).expect("fixture is valid WAT")
}

/// A manifest with `id`, the given permissions and contributions.
pub fn manifest(
    id: &str,
    permissions: serde_json::Value,
    contributes: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": format!("Test {id}"),
        "version": "1.0.0",
        "api": 1,
        "permissions": permissions,
        "contributes": contributes,
    })
}

/// Host services that record every call and answer `http` with 200 `ok`.
#[derive(Default, Clone)]
pub struct Fake {
    pub http: Rc<RefCell<Vec<HttpRequest>>>,
    pub storage: Rc<RefCell<BTreeMap<(String, String), serde_json::Value>>>,
    pub logs: Rc<RefCell<Vec<(String, NoteLevel, String)>>>,
    /// Body of the `http` answers.
    pub answer: Rc<RefCell<Vec<u8>>>,
    /// More headers of the `http` answers (e.g. `Set-Cookie`).
    pub answer_headers: Rc<RefCell<Vec<(String, String)>>>,
}

impl Fake {
    pub fn seen(&self, plugin: &str) -> serde_json::Value {
        self.storage
            .borrow()
            .get(&(plugin.to_owned(), "seen".to_owned()))
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }
}

impl HostServices for Fake {
    fn http<'a>(
        &'a self,
        request: HttpRequest,
    ) -> LocalBoxFuture<'a, Result<HostResponse, String>> {
        let url = request.url.clone();
        self.http.borrow_mut().push(request);
        let body = self.answer.borrow().clone();
        let mut headers = vec![("content-type".to_owned(), "text/plain".to_owned())];
        headers.extend(self.answer_headers.borrow().iter().cloned());
        Box::pin(async move {
            Ok((
                ResponseHead {
                    status: 200,
                    version: HttpVersion::Http11,
                    headers,
                    url,
                    redirects: Vec::new(),
                    timing: Timing::default(),
                },
                if body.is_empty() {
                    b"ok".to_vec()
                } else {
                    body
                },
                false,
            ))
        })
    }

    fn storage_get<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
    ) -> LocalBoxFuture<'a, Option<serde_json::Value>> {
        let v = self
            .storage
            .borrow()
            .get(&(plugin.to_owned(), key.to_owned()))
            .cloned();
        Box::pin(async move { v })
    }

    fn storage_set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<serde_json::Value>,
    ) -> LocalBoxFuture<'a, ()> {
        let k = (plugin.to_owned(), key.to_owned());
        match value {
            Some(v) => self.storage.borrow_mut().insert(k, v),
            None => self.storage.borrow_mut().remove(&k),
        };
        Box::pin(async {})
    }

    fn log(&self, plugin: &str, level: NoteLevel, message: &str) {
        self.logs
            .borrow_mut()
            .push((plugin.to_owned(), level, message.to_owned()));
    }
}

/// The fake futures are always ready, so a busy poll with a no-op waker is enough.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

pub fn request(url: &str) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
        options: Default::default(),
    }
}
