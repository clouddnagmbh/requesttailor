// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Exchange observers (wave 17): a general extension point that hands every finished send to
//! code a distribution (or a test) adds. It is how an extension learns what came back
//! without owning the transport: record traffic, keep a searchable history, watch for
//! differences.
//!
//! An [`Exchange`] is built **after** the send and **after** masking: its report carries no
//! secret value, not in the URL, headers, cookies or the response body (the same value list
//! as the report, plus header values of `Authorization`, `Cookie`, `Set-Cookie`,
//! `Proxy-Authorization` and of headers whose name contains `key`, `token` or `secret`).
//! The response body is cut at [`EXCHANGE_BODY_LIMIT`].
//!
//! Observers are called synchronously and must be quick: hand the exchange to a channel and
//! return. A slow observer must never slow the send down. The builds of this repository have
//! none. A collection switches delivery off with `extensions.requesttailor.observe.enabled:
//! false` ([`crate::model::ObserveCollectionConfig`]).
//!
//! Where observers hang: `send_with` ([`crate::services::Services::observers`]; app,
//! `rt send`, `rt run`, runner), `rt agent` (every `/v1/send`, `collection` is `None`),
//! `rt mcp` (caller [`Caller::Agent`]), and in the UI `AppExtension.onExchange`.
//!
//! Owner: wave 17 strand A. The types here are the interface.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::guard::Caller;
use crate::send::SendReport;

/// Largest response body an [`Exchange`] carries (1 MiB); longer bodies are cut and
/// [`Exchange::truncated`] is set.
pub const EXCHANGE_BODY_LIMIT: usize = 1024 * 1024;

/// One finished send, masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// Who sent.
    pub caller: Caller,
    /// The collection (root path or id as the shell knows it); `None` for `rt agent`, which
    /// sends what the web app hands it.
    pub collection: Option<String>,
    /// The request path in the collection; `None` when the send had none (`rt agent`).
    pub request_path: Option<String>,
    pub environment: Option<String>,
    /// Unix milliseconds when the send finished.
    pub at: i64,
    /// The report with every secret value masked, including the response body (cut at
    /// [`EXCHANGE_BODY_LIMIT`]). Duration and [`crate::transport::Timing`] are in
    /// `report.response`.
    pub report: SendReport,
    /// The response body was cut at [`EXCHANGE_BODY_LIMIT`].
    #[serde(default)]
    pub truncated: bool,
}

/// Receives every [`Exchange`]. Quick and non-blocking; see the module docs.
pub trait ExchangeObserver {
    fn observe(&self, exchange: &Exchange);
}

/// The observers of a shell, shared between threads. Cheap to clone; compares by identity of
/// its members. Empty by default.
#[derive(Clone, Default)]
pub struct ObserverSet(Vec<Arc<dyn ExchangeObserverSync>>);

/// An observer that can be shared between threads (what [`ObserverSet`] holds).
pub trait ExchangeObserverSync: ExchangeObserver + Send + Sync {}

impl<T: ExchangeObserver + Send + Sync> ExchangeObserverSync for T {}

impl ObserverSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// A set of the given observers.
    pub fn of(observers: Vec<Arc<dyn ExchangeObserverSync>>) -> Self {
        Self(observers)
    }

    /// Adds an observer.
    pub fn with(mut self, observer: Arc<dyn ExchangeObserverSync>) -> Self {
        self.0.push(observer);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Hands `exchange` to every observer, in the order they were added.
    pub fn notify(&self, exchange: &Exchange) {
        for observer in &self.0 {
            observer.observe(exchange);
        }
    }
}

/// What the desktop builder manages as Tauri state (`builder().manage(ObserverHandle)`), the
/// same shape the CLI takes in `Distribution.observers`.
pub type ObserverHandle = ObserverSet;

impl std::fmt::Debug for ObserverSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObserverSet({})", self.0.len())
    }
}

impl PartialEq for ObserverSet {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().zip(&other.0).all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

impl Eq for ObserverSet {}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Count(Mutex<usize>);

    impl ExchangeObserver for Count {
        fn observe(&self, _: &Exchange) {
            *self.0.lock().unwrap() += 1;
        }
    }

    fn exchange() -> Exchange {
        Exchange {
            caller: Caller::Cli,
            collection: None,
            request_path: Some("a.yml".into()),
            environment: None,
            at: 0,
            report: crate::send::SendReport {
                version: crate::send::REPORT_VERSION,
                request_path: "a.yml".into(),
                environment: None,
                request: None,
                response: None,
                variables: Vec::new(),
                unresolved: Vec::new(),
                error: None,
                notes: Vec::new(),
                tests: Vec::new(),
                assertions: Vec::new(),
                logs: Vec::new(),
                var_changes: Vec::new(),
                flow: None,
            },
            truncated: false,
        }
    }

    #[test]
    fn notifies_every_observer_in_order() {
        let a = Arc::new(Count(Mutex::new(0)));
        let b = Arc::new(Count(Mutex::new(0)));
        let set = ObserverSet::new().with(a.clone()).with(b.clone());
        assert_eq!(set.len(), 2);
        set.notify(&exchange());
        assert_eq!((*a.0.lock().unwrap(), *b.0.lock().unwrap()), (1, 1));
        assert!(ObserverSet::new().is_empty());
    }
}
