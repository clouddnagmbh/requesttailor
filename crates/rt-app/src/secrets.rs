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

    impl SecretStore for KeychainStore {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            let _ = key;
            todo!("wave 3, strand D")
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            let _ = (key, value);
            todo!("wave 3, strand D")
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            let _ = key;
            todo!("wave 3, strand D")
        }
    }
}
