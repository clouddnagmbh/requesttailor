// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret values, outside the collection files (product principle 3). In the files a secret
//! variable has a name and `secret: true`, never a value.
//!
//! Native shells store values in the OS keychain (`keychain` module, native only). The web
//! shell stores them encrypted in browser storage, implemented in TypeScript
//! (`apps/web/src/secrets/store.ts`), and passes values in `SendInput::secrets`.
//!
//! Owner: wave 3, strand D.

use serde::{Deserialize, Serialize};

use crate::AppError;

/// Identifies one secret value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretKey {
    /// Stable id of the collection chosen by the shell (desktop: canonical root path;
    /// web: a random id stored with the directory handle).
    pub collection: String,
    pub environment: String,
    pub variable: String,
}

pub trait SecretStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError>;
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError>;
    fn delete(&self, key: &SecretKey) -> Result<(), AppError>;
}

/// OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux; iOS
/// Keychain). Native only.
#[cfg(not(target_arch = "wasm32"))]
pub mod keychain {
    use super::{SecretKey, SecretStore};
    use crate::AppError;

    /// Stores each value under `service` with account `<collection>/<environment>/<variable>`.
    #[derive(Debug, Clone)]
    pub struct KeychainStore {
        pub service: String,
    }

    impl KeychainStore {
        pub fn new(service: impl Into<String>) -> Self {
            Self {
                service: service.into(),
            }
        }
    }

    impl Default for KeychainStore {
        /// The app's store: service `"RequestTailor"`.
        fn default() -> Self {
            Self::new(SERVICE)
        }
    }

    /// Service name of the app's keychain entries.
    pub const SERVICE: &str = "RequestTailor";

    /// Keychain account for one secret: `<collection>/<environment>/<variable>`.
    ///
    /// `%` and `/` in the environment and variable names are percent-encoded, so the last two
    /// `/`-separated segments are always environment and variable and the mapping is
    /// injective even though the collection (a path) contains `/`.
    pub fn account(key: &SecretKey) -> String {
        fn escape(s: &str) -> String {
            s.replace('%', "%25").replace('/', "%2F")
        }
        format!(
            "{}/{}/{}",
            key.collection,
            escape(&key.environment),
            escape(&key.variable)
        )
    }

    impl SecretStore for KeychainStore {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            os::get(&self.service, &account(key))
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            os::set(&self.service, &account(key), value)
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            os::delete(&self.service, &account(key))
        }
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux"
    ))]
    mod os {
        use crate::AppError;
        use keyring::{Entry, Error};

        fn entry(service: &str, account: &str) -> Result<Entry, AppError> {
            Entry::new(service, account).map_err(shell)
        }

        fn shell(e: Error) -> AppError {
            AppError::Shell(format!("keychain: {e}"))
        }

        pub fn get(service: &str, account: &str) -> Result<Option<String>, AppError> {
            match entry(service, account)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(Error::NoEntry) => Ok(None),
                Err(e) => Err(shell(e)),
            }
        }

        pub fn set(service: &str, account: &str, value: &str) -> Result<(), AppError> {
            entry(service, account)?.set_password(value).map_err(shell)
        }

        pub fn delete(service: &str, account: &str) -> Result<(), AppError> {
            match entry(service, account)?.delete_credential() {
                Ok(()) | Err(Error::NoEntry) => Ok(()),
                Err(e) => Err(shell(e)),
            }
        }
    }

    /// Native targets without a supported keychain: refuse rather than store values elsewhere.
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux"
    )))]
    mod os {
        use crate::AppError;

        fn unsupported() -> AppError {
            AppError::Shell("no OS keychain on this platform".into())
        }

        pub fn get(_: &str, _: &str) -> Result<Option<String>, AppError> {
            Err(unsupported())
        }

        pub fn set(_: &str, _: &str, _: &str) -> Result<(), AppError> {
            Err(unsupported())
        }

        pub fn delete(_: &str, _: &str) -> Result<(), AppError> {
            Err(unsupported())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn key(collection: &str, environment: &str, variable: &str) -> SecretKey {
            SecretKey {
                collection: collection.into(),
                environment: environment.into(),
                variable: variable.into(),
            }
        }

        #[test]
        fn account_is_collection_environment_variable() {
            assert_eq!(
                account(&key("abc123", "Local", "token")),
                "abc123/Local/token"
            );
        }

        #[test]
        fn account_keeps_paths_and_spaces() {
            assert_eq!(
                account(&key("/Users/me/api", "Staging EU", "api key")),
                "/Users/me/api/Staging EU/api key"
            );
        }

        #[test]
        fn accounts_differ_per_environment_and_variable() {
            let a = account(&key("c", "Local", "token"));
            assert_ne!(a, account(&key("c", "PRD", "token")));
            assert_ne!(a, account(&key("c", "Local", "password")));
            assert_ne!(a, account(&key("d", "Local", "token")));
        }

        #[test]
        fn slashes_in_names_cannot_collide() {
            let a = account(&key("/p/a", "b", "c"));
            let b = account(&key("/p", "a/b", "c"));
            let c = account(&key("/p", "a", "b/c"));
            assert_ne!(a, b);
            assert_ne!(a, c);
            assert_ne!(b, c);
            assert_eq!(b, "/p/a%2Fb/c");
            assert_ne!(account(&key("/p", "a%2Fb", "c")), b);
        }

        #[test]
        fn default_store_uses_the_app_service() {
            assert_eq!(KeychainStore::default().service, "RequestTailor");
        }

        /// Talks to the real OS keychain, which CI runners do not offer. Run locally with
        /// `cargo test -p rt-app keychain -- --ignored`.
        #[test]
        #[ignore = "needs a real OS keychain"]
        fn real_keychain_round_trip() {
            let store = KeychainStore::new("RequestTailor-test");
            let k = key("rt-app-test", "Local", "token");
            store.delete(&k).unwrap();
            assert_eq!(store.get(&k).unwrap(), None);
            store.set(&k, "s3cr3t-value").unwrap();
            assert_eq!(store.get(&k).unwrap().as_deref(), Some("s3cr3t-value"));
            store.set(&k, "changed").unwrap();
            assert_eq!(store.get(&k).unwrap().as_deref(), Some("changed"));
            store.delete(&k).unwrap();
            assert_eq!(store.get(&k).unwrap(), None);
            // Deleting a missing entry is not an error.
            store.delete(&k).unwrap();
        }
    }
}
