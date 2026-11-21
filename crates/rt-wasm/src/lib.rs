// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-wasm`: The core compiled to WebAssembly.
//!
//! Scaffold only (wave 1). No functionality yet.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "rt-wasm");
    }
}
