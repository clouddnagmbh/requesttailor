// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Native tests for plugins (not on wasm32): call the hooks through the same JSON dispatch
//! the `rt_call` export uses, and answer host calls with a [`MockHost`].
//!
//! ```
//! use rt_plugin_sdk::api::{Call, PrepareReply};
//! use rt_plugin_sdk::testing::{self, MockHost};
//! # struct MyPlugin;
//! # impl rt_plugin_sdk::Plugin for MyPlugin {}
//!
//! let host = MockHost::new();
//! let _guard = host.install();
//! let call = Call::Prepare { request: testing::request("GET", "https://example.com/"), context: testing::context("a.yml") };
//! let reply: Result<PrepareReply, String> = testing::call(&MyPlugin, &call);
//! assert!(reply.is_err()); // MyPlugin does not implement prepare
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rt_core::services::NoteLevel;
use rt_plugin::api::{
    Call, HostCall, Permissions, PluginBody, PluginContext, PluginRequest, PluginResponse,
};
use serde::de::DeserializeOwned;

use crate::Plugin;

/// Runs one call through [`handle`](crate::handle), exactly as the `rt_call` export does, and
/// decodes the reply: `Ok` with the hook's reply or `Err` with the error message.
pub fn call<T: DeserializeOwned>(
    plugin: &(impl Plugin + ?Sized),
    call: &Call,
) -> Result<T, String> {
    let json = serde_json::to_vec(call).expect("calls serialize");
    let reply: serde_json::Value =
        serde_json::from_slice(&crate::handle(plugin, &json)).expect("replies are JSON");
    if let Some(error) = reply.get("error") {
        return Err(error.as_str().unwrap_or_default().to_owned());
    }
    serde_json::from_value(reply["ok"].clone()).map_err(|e| format!("unexpected reply: {e}"))
}

/// A request with no headers and no body.
pub fn request(method: &str, url: &str) -> PluginRequest {
    PluginRequest {
        method: method.into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
    }
}

/// A context for the request file `request_path`, without environment and variables.
pub fn context(request_path: &str) -> PluginContext {
    PluginContext {
        request_path: request_path.into(),
        environment: None,
        variables: Vec::new(),
    }
}

/// A complete text response.
pub fn response(status: u16, content_type: &str, text: &str) -> PluginResponse {
    PluginResponse {
        status,
        headers: vec![("Content-Type".into(), content_type.into())],
        url: "https://example.com/".into(),
        body: PluginBody {
            content_type: Some(content_type.into()),
            text: Some(text.into()),
            base64: None,
        },
        truncated: false,
    }
}

type HttpHandler = Box<dyn FnMut(&PluginRequest, bool) -> Result<PluginResponse, String>>;

#[derive(Default)]
struct State {
    permissions: Permissions,
    request_host: Option<String>,
    now: u64,
    logs: Vec<(NoteLevel, String)>,
    storage: BTreeMap<String, serde_json::Value>,
    http: Option<HttpHandler>,
    calls: Vec<HostCall>,
}

/// A host for native tests: answers the host calls of the current thread like the real host,
/// including the permission checks. Clone it to keep a handle for assertions.
#[derive(Clone, Default)]
pub struct MockHost(Rc<RefCell<State>>);

impl MockHost {
    /// No permissions, clock at 0, empty storage, no HTTP handler.
    pub fn new() -> Self {
        Self::default()
    }

    /// The permissions the user granted (usually the manifest's).
    pub fn granted(self, permissions: Permissions) -> Self {
        self.0.borrow_mut().permissions = permissions;
        self
    }

    /// The host of the request the hook runs for, matched by the `{request}` network entry.
    pub fn request_host(self, host: &str) -> Self {
        self.0.borrow_mut().request_host = Some(host.to_owned());
        self
    }

    pub fn now(self, unix_ms: u64) -> Self {
        self.0.borrow_mut().now = unix_ms;
        self
    }

    /// Answers `http` host calls that pass the network check.
    pub fn on_http(
        self,
        handler: impl FnMut(&PluginRequest, bool) -> Result<PluginResponse, String> + 'static,
    ) -> Self {
        self.0.borrow_mut().http = Some(Box::new(handler));
        self
    }

    /// Makes this the host of the current thread until the guard drops.
    pub fn install(&self) -> HostGuard {
        let previous = CURRENT.with(|c| c.replace(Some(self.clone())));
        HostGuard { previous }
    }

    pub fn logs(&self) -> Vec<(NoteLevel, String)> {
        self.0.borrow().logs.clone()
    }

    pub fn storage(&self) -> BTreeMap<String, serde_json::Value> {
        self.0.borrow().storage.clone()
    }

    /// Every host call so far, in order.
    pub fn calls(&self) -> Vec<HostCall> {
        self.0.borrow().calls.clone()
    }

    fn answer(&self, call: HostCall) -> Result<serde_json::Value, String> {
        use serde_json::{Value, to_value};
        self.0.borrow_mut().calls.push(call.clone());
        let storage_allowed = self.0.borrow().permissions.storage;
        let denied_storage = || Err("permission denied: storage".to_owned());
        match call {
            HostCall::Log { level, message } => {
                self.0.borrow_mut().logs.push((level, message));
                Ok(Value::Null)
            }
            HostCall::Now => Ok(self.0.borrow().now.into()),
            HostCall::StorageGet { key } if storage_allowed => Ok(self
                .0
                .borrow()
                .storage
                .get(&key)
                .cloned()
                .unwrap_or(Value::Null)),
            HostCall::StorageSet { key, value } if storage_allowed => {
                self.0.borrow_mut().storage.insert(key, value);
                Ok(Value::Null)
            }
            HostCall::StorageDelete { key } if storage_allowed => {
                self.0.borrow_mut().storage.remove(&key);
                Ok(Value::Null)
            }
            HostCall::StorageGet { .. }
            | HostCall::StorageSet { .. }
            | HostCall::StorageDelete { .. } => denied_storage(),
            HostCall::Http {
                request,
                credentials,
                session: _,
            } => {
                let allowed = {
                    let state = self.0.borrow();
                    host_of(&request.url).is_some_and(|host| {
                        network_allows(
                            &state.permissions.network,
                            host,
                            state.request_host.as_deref(),
                        )
                    })
                };
                if !allowed {
                    return Err("permission denied: network".to_owned());
                }
                // Take the handler out so it may make host calls itself.
                let handler = self.0.borrow_mut().http.take();
                let Some(mut handler) = handler else {
                    return Err("MockHost: no HTTP handler (use on_http)".to_owned());
                };
                let response = handler(&request, credentials);
                self.0.borrow_mut().http = Some(handler);
                Ok(to_value(response?).expect("responses serialize"))
            }
        }
    }
}

/// Uninstalls the [`MockHost`] (restores the previous one) when dropped.
pub struct HostGuard {
    previous: Option<MockHost>,
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<MockHost>> = const { RefCell::new(None) };
}

/// Native `rt.host`: the installed [`MockHost`], or an error reply.
pub(crate) fn host(request: &[u8]) -> crate::Result<Vec<u8>> {
    let host = CURRENT.with(|c| c.borrow().clone());
    let reply = match host {
        None => serde_json::json!({
            "error": "no host: native builds answer host calls only with testing::MockHost"
        }),
        Some(host) => match serde_json::from_slice::<HostCall>(request) {
            Err(e) => serde_json::json!({ "error": format!("invalid host call: {e}") }),
            Ok(call) => match host.answer(call) {
                Ok(ok) => serde_json::json!({ "ok": ok }),
                Err(error) => serde_json::json!({ "error": error }),
            },
        },
    };
    Ok(serde_json::to_vec(&reply)?)
}

/// The host part of a URL (lowercase), without userinfo and port.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The `network` rules of API.md.
fn network_allows(entries: &[String], host: String, request_host: Option<&str>) -> bool {
    entries.iter().any(|entry| match entry.as_str() {
        "*" => true,
        "{request}" => request_host.is_some_and(|r| r.eq_ignore_ascii_case(&host)),
        e => match e.strip_prefix("*.") {
            Some(suffix) => host
                .strip_suffix(&suffix.to_ascii_lowercase())
                .is_some_and(|sub| sub.ends_with('.') && sub.len() > 1),
            None => e.eq_ignore_ascii_case(&host),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_rules_follow_api_md() {
        let rules = |e: &[&str]| e.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!network_allows(&[], "a.com".into(), None));
        assert!(network_allows(&rules(&["*"]), "a.com".into(), None));
        assert!(network_allows(&rules(&["a.com"]), "a.com".into(), None));
        assert!(!network_allows(&rules(&["a.com"]), "b.a.com".into(), None));
        assert!(network_allows(&rules(&["*.a.com"]), "b.a.com".into(), None));
        assert!(!network_allows(&rules(&["*.a.com"]), "a.com".into(), None));
        assert!(!network_allows(&rules(&["*.a.com"]), "ba.com".into(), None));
        assert!(network_allows(
            &rules(&["{request}"]),
            "api.x".into(),
            Some("API.x")
        ));
        assert!(!network_allows(
            &rules(&["{request}"]),
            "api.x".into(),
            None
        ));
    }

    #[test]
    fn host_of_strips_userinfo_port_and_path() {
        assert_eq!(
            host_of("https://u:p@Example.com:8443/x?y").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("http://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(host_of("no-scheme"), None);
    }
}
