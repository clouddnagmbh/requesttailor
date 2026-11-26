// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! HTTPS credentials per host in the app's secret store (key scheme: see the crate docs), on
//! top of the core's [`CredentialSource`] (re-exported here at its old path).

use rt_app::{AppError, SecretKey, SecretStore};
use tk_git::GitError;
pub use tk_git::credentials::*;

/// `SecretKey::collection` of every Git credential entry.
pub const COLLECTION: &str = "git:credentials";
const USERNAME: &str = "username";
const PASSWORD: &str = "password";

/// Credentials from a secret store, under [`key`].
pub struct StoreCredentials<'a, S>(pub &'a S);

impl<S: SecretStore> CredentialSource for StoreCredentials<'_, S> {
    fn get(&self, host: &str) -> Result<Option<Credentials>, GitError> {
        // The store's error travels through the core unchanged (`GitError::Host`).
        get(self.0, host).map_err(GitError::from)
    }
}

/// The secret-store key of `field` (`username` or `password`) for `host`.
pub fn key(host: &str, field: &str) -> SecretKey {
    SecretKey {
        collection: COLLECTION.to_owned(),
        environment: host.to_owned(),
        variable: field.to_owned(),
    }
}

fn check_host(host: &str) -> Result<(), AppError> {
    if host.is_empty() || host.contains(['/', '\\', '@', ' ']) {
        return Err(AppError::Git(format!("invalid host: {host:?}")));
    }
    Ok(())
}

pub fn get(store: &impl SecretStore, host: &str) -> Result<Option<Credentials>, AppError> {
    check_host(host)?;
    let Some(password) = store.get(&key(host, PASSWORD))? else {
        return Ok(None);
    };
    let username = store.get(&key(host, USERNAME))?.unwrap_or_default();
    Ok(Some(Credentials { username, password }))
}

pub fn has(store: &impl SecretStore, host: &str) -> Result<bool, AppError> {
    check_host(host)?;
    Ok(store.get(&key(host, PASSWORD))?.is_some())
}

pub fn set(
    store: &impl SecretStore,
    host: &str,
    username: &str,
    password: &str,
) -> Result<(), AppError> {
    check_host(host)?;
    store.set(&key(host, USERNAME), username)?;
    store.set(&key(host, PASSWORD), password)
}

pub fn delete(store: &impl SecretStore, host: &str) -> Result<(), AppError> {
    check_host(host)?;
    store.delete(&key(host, PASSWORD))?;
    store.delete(&key(host, USERNAME))
}
