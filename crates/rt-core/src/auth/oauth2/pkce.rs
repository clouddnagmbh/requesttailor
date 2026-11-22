// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! PKCE (RFC 7636) and random values for `state`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::Digest as _;

use super::Clock;
use crate::model::PkceMethod;

/// `bytes` random bytes as base64url without padding: only characters PKCE allows
/// (`[A-Za-z0-9-_]`). 48 bytes give a 64 character verifier (RFC 7636: 43 to 128).
pub(super) fn random_string(clock: &dyn Clock, bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    clock.fill_random(&mut buf)?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The `code_challenge` for `verifier`.
pub(super) fn challenge(method: PkceMethod, verifier: &str) -> String {
    match method {
        PkceMethod::S256 => URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes())),
        PkceMethod::Plain => verifier.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_appendix_b() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge(PkceMethod::S256, verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(challenge(PkceMethod::Plain, verifier), verifier);
    }
}
