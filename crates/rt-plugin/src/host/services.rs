// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! [`HostServices`] for the shells: `http` through any `rt_core` transport (native, `fetch`
//! or the companion), storage through a [`PluginStorage`] (a file per plugin natively, browser
//! storage in the web app, memory in tests).

use std::cell::RefCell;
use std::collections::BTreeMap;

use rt_core::services::{LocalBoxFuture, NoteLevel};
use rt_core::transport::{HttpRequest, Transport, collect};
use serde_json::Value;

use super::{HostResponse, HostServices, PLUGIN_BODY_LIMIT};

/// A plugin's private key-value store (`storage` permission), per plugin id.
pub trait PluginStorage {
    fn get<'a>(&'a self, plugin: &'a str, key: &'a str) -> LocalBoxFuture<'a, Option<Value>>;
    /// `None` deletes the key.
    fn set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()>;
}

/// Storage that lives as long as the value (tests, `--plugin` runs without a data folder).
#[derive(Debug, Default)]
pub struct MemoryStorage(RefCell<BTreeMap<(String, String), Value>>);

impl PluginStorage for MemoryStorage {
    fn get<'a>(&'a self, plugin: &'a str, key: &'a str) -> LocalBoxFuture<'a, Option<Value>> {
        let v = self
            .0
            .borrow()
            .get(&(plugin.to_owned(), key.to_owned()))
            .cloned();
        Box::pin(async move { v })
    }

    fn set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()> {
        let k = (plugin.to_owned(), key.to_owned());
        match value {
            Some(v) => self.0.borrow_mut().insert(k, v),
            None => self.0.borrow_mut().remove(&k),
        };
        Box::pin(async {})
    }
}

/// Where plugin log lines go.
pub type Logger = Box<dyn Fn(&str, NoteLevel, &str)>;

/// `HostServices` from a transport and a storage. `transport: None` answers every `http`
/// with an error (a shell without a network stack).
pub struct TransportServices<T, S> {
    transport: Option<T>,
    storage: S,
    logger: Option<Logger>,
}

impl<T: Transport, S: PluginStorage> TransportServices<T, S> {
    pub fn new(transport: Option<T>, storage: S) -> Self {
        Self {
            transport,
            storage,
            logger: None,
        }
    }

    pub fn with_logger(mut self, logger: Logger) -> Self {
        self.logger = Some(logger);
        self
    }
}

impl<T: Transport, S: PluginStorage> HostServices for TransportServices<T, S> {
    fn http<'a>(
        &'a self,
        request: HttpRequest,
    ) -> LocalBoxFuture<'a, Result<HostResponse, String>> {
        Box::pin(async move {
            let transport = self.transport.as_ref().ok_or("no network in this shell")?;
            let started = web_time::Instant::now();
            let response = transport.send(request).await.map_err(|e| e.to_string())?;
            let done = collect(response, PLUGIN_BODY_LIMIT, started)
                .await
                .map_err(|e| e.to_string())?;
            Ok((done.head, done.body.to_vec(), done.truncated))
        })
    }

    fn storage_get<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
    ) -> LocalBoxFuture<'a, Option<Value>> {
        self.storage.get(plugin, key)
    }

    fn storage_set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()> {
        self.storage.set(plugin, key, value)
    }

    fn log(&self, plugin: &str, level: NoteLevel, message: &str) {
        if let Some(logger) = &self.logger {
            logger(plugin, level, message);
        }
    }
}
