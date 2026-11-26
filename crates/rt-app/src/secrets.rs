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

impl From<&SecretKey> for tk_secrets::SecretSlot {
    fn from(key: &SecretKey) -> Self {
        Self {
            space: key.collection.clone(),
            group: key.environment.clone(),
            name: key.variable.clone(),
        }
    }
}

/// OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux; iOS
/// Keychain). Native only. The storage lives in `tk_secrets::keychain` (seam K2); this module
/// binds it to the app's keys and service name.
#[cfg(not(target_arch = "wasm32"))]
pub mod keychain {
    use super::{SecretKey, SecretStore};
    use crate::AppError;
    use tk_secrets::SecretStore as _;

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

        fn kit(&self) -> tk_secrets::keychain::KeychainStore {
            tk_secrets::keychain::KeychainStore::new(self.service.clone())
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

    /// Keychain account for one secret: `<collection>/<environment>/<variable>`
    /// (see `tk_secrets::keychain::account`).
    pub fn account(key: &SecretKey) -> String {
        tk_secrets::keychain::account(&key.into())
    }

    impl SecretStore for KeychainStore {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            Ok(self.kit().get(&key.into())?)
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            Ok(self.kit().set(&key.into(), value)?)
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            Ok(self.kit().delete(&key.into())?)
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
            assert_eq!(
                account(&key("/Users/me/api", "Staging EU", "api key")),
                "/Users/me/api/Staging EU/api key"
            );
            assert_eq!(account(&key("/p", "a/b", "c")), "/p/a%2Fb/c");
        }

        #[test]
        fn default_store_uses_the_app_service() {
            assert_eq!(KeychainStore::default().service, "RequestTailor");
        }

        #[test]
        fn kit_errors_keep_the_apps_messages() {
            use tk_secrets::SecretError;
            assert_eq!(
                AppError::from(SecretError::Store("boom".into())),
                AppError::Shell("keychain: boom".into())
            );
            assert_eq!(
                AppError::from(SecretError::Unsupported(
                    "no OS keychain on this platform".into()
                )),
                AppError::Shell("no OS keychain on this platform".into())
            );
        }
    }
}
