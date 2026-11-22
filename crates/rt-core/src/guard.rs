// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Send guard (wave 10): a check a distribution adds in front of every send, on every
//! surface. The builds of this repository add none, so nothing here changes their behaviour.
//!
//! Every send goes through [`crate::send::send_with`] or [`crate::stream::open`]: the desktop
//! app, the web app, `rt send`, `rt run`, the runner, and `rt mcp`. When the shell passes a
//! guard in [`crate::services::Services::guard`], both ask it once the request is final (after
//! scripts, `prepare` and plugin hooks) and before its first byte goes out; requests built by
//! scripts (`rt.sendRequest`) are asked too. A [`GuardVerdict::Refuse`] stops the send with
//! [`crate::send::SendError::Blocked`]; nothing reaches the network. Requests for OAuth 2.0
//! tokens go to the identity provider the collection names and are not asked.
//!
//! The guard sees a [`SendCheck`]: who sends ([`Caller`]), what (method, URL with secret values
//! masked), whether it writes ([`effect`]) and whether the collection marks the environment
//! as production. It never sees a secret value, a header value or a body.
//!
//! How each shell takes a guard: `rt_cli::run_distribution` (CLI and `rt mcp`), the
//! [`GuardHandle`] managed in the desktop builder's state, and the `guard` field of the web
//! shell's send services (`crates/rt-wasm`).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::model::{Body, Protocol, RequestSpec};

/// Who asks to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Caller {
    /// The desktop or web app (a person clicked Send, or the runner in the app).
    #[default]
    App,
    /// `rt send`, `rt run`: a person at a terminal or a pipeline.
    Cli,
    /// A coding agent through `rt mcp`.
    Agent,
}

/// Which request of a send is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose {
    /// The request the user (or agent) sends, or a stream it opens.
    Main,
    /// A request a script builds with `rt.sendRequest`.
    Script,
}

/// Whether a request changes something on the server. Writing: every method other than
/// GET, HEAD, OPTIONS (and TRACE); a GraphQL body with a `mutation`; every gRPC call (the
/// file does not say whether it writes); a WebSocket with messages to send. SSE and
/// WebSocket without messages only listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Read,
    Write,
}

/// What the guard is asked about. Owned and serializable, so a guard may run anywhere (the
/// web shell passes it to JavaScript as JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendCheck {
    pub caller: Caller,
    pub purpose: Purpose,
    /// Name of the collection.
    pub collection: String,
    /// Path of the request in the collection (for [`Purpose::Script`]: the request whose
    /// script sends).
    pub request_path: String,
    pub environment: Option<String>,
    pub method: String,
    /// Resolved URL, secret values masked.
    pub url: String,
    /// For [`Purpose::Main`] from the request as written (see [`effect`]); for
    /// [`Purpose::Script`] from the method.
    pub effect: Effect,
    /// The collection's agent policy marks the environment as production.
    pub production: bool,
}

/// The guard's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum GuardVerdict {
    Allow,
    /// Not sent. `rule` names the rule (for the audit log), `reason` is shown to the user or
    /// agent; neither may contain a secret value.
    Refuse {
        rule: String,
        reason: String,
    },
}

/// The check a distribution adds. Synchronous and cheap: it runs before every send.
pub trait SendGuard {
    fn check(&self, check: &SendCheck) -> GuardVerdict;
}

impl<G: SendGuard + ?Sized> SendGuard for Arc<G> {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        (**self).check(check)
    }
}

impl<G: SendGuard + ?Sized> SendGuard for Box<G> {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        (**self).check(check)
    }
}

/// A guard shared between threads, or none: what the CLI arguments and the desktop state
/// carry. Compares by identity.
#[derive(Clone, Default)]
pub struct GuardHandle(Option<Arc<dyn SendGuard + Send + Sync>>);

impl GuardHandle {
    pub fn new(guard: Arc<dyn SendGuard + Send + Sync>) -> Self {
        Self(Some(guard))
    }

    pub fn none() -> Self {
        Self(None)
    }

    pub fn get(&self) -> Option<&dyn SendGuard> {
        self.0.as_deref().map(|g| g as &dyn SendGuard)
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

impl std::fmt::Debug for GuardHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "GuardHandle(Some)"
        } else {
            "GuardHandle(None)"
        })
    }
}

impl PartialEq for GuardHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for GuardHandle {}

/// Methods that only read (RFC 9110 "safe" methods).
const READING_METHODS: &[&str] = &["GET", "HEAD", "OPTIONS", "TRACE"];

/// Whether `method` only reads.
pub fn method_effect(method: &str) -> Effect {
    let method = method.trim().to_ascii_uppercase();
    if READING_METHODS.contains(&method.as_str()) {
        Effect::Read
    } else {
        Effect::Write
    }
}

/// What sending `spec` does (rules at [`Effect`]).
pub fn effect(spec: &RequestSpec) -> Effect {
    match &spec.protocol {
        Protocol::Grpc(_) => return Effect::Write,
        Protocol::WebSocket(ws) => {
            return if ws.messages.is_empty() {
                Effect::Read
            } else {
                Effect::Write
            };
        }
        Protocol::Http => {}
    }
    let method = spec.method.trim().to_ascii_uppercase();
    let reading = READING_METHODS.contains(&method.as_str());
    if let Body::GraphQl { query, .. } = &spec.body {
        // GraphQL goes as POST (or GET); the operation decides.
        if method == "POST" || reading {
            return if graphql_has_mutation(query) {
                Effect::Write
            } else {
                Effect::Read
            };
        }
        return Effect::Write;
    }
    if reading { Effect::Read } else { Effect::Write }
}

/// Whether a GraphQL document defines a `mutation` operation: the keyword `mutation` as a
/// name outside every selection set (`{}`) and argument list (`()`), ignoring comments,
/// strings, `$variables` and `@directives`. A document with several operations counts as
/// writing if any of them is a mutation (`operationName` may pick any of them).
pub fn graphql_has_mutation(query: &str) -> bool {
    let bytes = query.as_bytes();
    let (mut i, mut braces, mut parens) = (0usize, 0i32, 0i32);
    while i < bytes.len() {
        match bytes[i] {
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                }
            }
            b'"' if bytes[i..].starts_with(b"\"\"\"") => {
                i += 3;
                while i < bytes.len() && !bytes[i..].starts_with(b"\"\"\"") {
                    i += if bytes[i..].starts_with(b"\\\"\"\"") {
                        4
                    } else {
                        1
                    };
                }
                i += 3;
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' && bytes[i] != b'\n' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' => braces += 1,
            b'}' => braces -= 1,
            b'(' => parens += 1,
            b')' => parens -= 1,
            b'$' | b'@' => {
                // The name that follows is a variable or a directive.
                i += 1;
                while i < bytes.len() && is_name(bytes[i]) {
                    i += 1;
                }
                continue;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && is_name(bytes[i]) {
                    i += 1;
                }
                if braces == 0 && parens == 0 && &bytes[start..i] == b"mutation" {
                    return true;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn is_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape() {
        let check = SendCheck {
            caller: Caller::Agent,
            purpose: Purpose::Main,
            collection: "Shop".into(),
            request_path: "orders/create.yml".into(),
            environment: Some("PRD".into()),
            method: "POST".into(),
            url: "https://prd.example/orders?key=***".into(),
            effect: Effect::Write,
            production: true,
        };
        let v = serde_json::to_value(&check).unwrap();
        assert_eq!(v["caller"], "agent");
        assert_eq!(v["purpose"], "main");
        assert_eq!(v["effect"], "write");
        assert_eq!(
            serde_json::to_value(GuardVerdict::Refuse {
                rule: "r".into(),
                reason: "no".into()
            })
            .unwrap(),
            serde_json::json!({"decision": "refuse", "rule": "r", "reason": "no"})
        );
        assert_eq!(
            serde_json::to_value(GuardVerdict::Allow).unwrap(),
            serde_json::json!({"decision": "allow"})
        );
    }

    #[test]
    fn handles_compare_by_identity() {
        struct No;
        impl SendGuard for No {
            fn check(&self, _: &SendCheck) -> GuardVerdict {
                GuardVerdict::Allow
            }
        }
        let a = GuardHandle::new(Arc::new(No));
        assert_eq!(a, a.clone());
        assert_ne!(a, GuardHandle::new(Arc::new(No)));
        assert_eq!(GuardHandle::none(), GuardHandle::default());
        assert!(a.get().is_some() && GuardHandle::none().get().is_none());
    }

    #[test]
    fn methods() {
        assert_eq!(method_effect("get"), Effect::Read);
        assert_eq!(method_effect("PATCH"), Effect::Write);
    }
}
