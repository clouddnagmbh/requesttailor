// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Pairing codes and tokens of `rt agent`. Pure logic, the clock is passed in.
//!
//! - A code is 8 characters from Crockford's base32 alphabet (40 bits), shown as `ABCD-EFGH`.
//!   It is valid once, for [`CODE_TTL`]; after [`MAX_FAILURES`] wrong attempts it is burned.
//!   Whenever a code is used, expires or is burned, a new one is made (and printed).
//! - A token is 32 random bytes (base64url), bound to the origin that paired. Tokens live
//!   in memory only: restarting `rt agent` revokes them all.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use web_time::Instant;

pub const CODE_TTL: Duration = Duration::from_secs(10 * 60);
pub const MAX_FAILURES: u32 = 5;
const CODE_LEN: usize = 8;
/// Crockford base32: no I, L, O, U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Fills `buf` from the operating system's random source.
fn random(buf: &mut [u8]) {
    // Failing to get randomness is not recoverable for a security token.
    getrandom::getrandom(buf).expect("operating system random source");
}

fn new_code() -> String {
    let mut bytes = [0u8; CODE_LEN];
    random(&mut bytes);
    // 256 is a multiple of 32, so `% 32` keeps the distribution uniform.
    bytes
        .iter()
        .map(|b| ALPHABET[usize::from(b % 32)] as char)
        .collect()
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    random(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compares without an early exit, so timing does not reveal how much matched.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What a pairing attempt led to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// The new token, if the code was right.
    pub token: Option<String>,
    /// A code replaced the previous one (used, expired or burned); print it.
    pub new_code: Option<String>,
    /// Why the attempt failed, for the log.
    pub reason: Option<&'static str>,
}

#[derive(Debug)]
pub struct Pairing {
    code: String,
    issued: Instant,
    failures: u32,
    /// (origin, token)
    tokens: Vec<(String, String)>,
}

impl Pairing {
    pub fn new(now: Instant) -> Self {
        Self {
            code: new_code(),
            issued: now,
            failures: 0,
            tokens: Vec::new(),
        }
    }

    /// The current code for display, e.g. `ABCD-EFGH`.
    pub fn code(&self) -> String {
        format!("{}-{}", &self.code[..4], &self.code[4..])
    }

    /// The current code as it goes into a pairing link (no dash).
    pub fn link_code(&self) -> &str {
        &self.code
    }

    fn rotate(&mut self, now: Instant) -> String {
        self.code = new_code();
        self.issued = now;
        self.failures = 0;
        self.code()
    }

    /// Checks `code` for `origin`; on success a token is issued and the code is replaced.
    pub fn attempt(&mut self, origin: &str, code: &str, now: Instant) -> Attempt {
        if now.duration_since(self.issued) >= CODE_TTL {
            return Attempt {
                token: None,
                new_code: Some(self.rotate(now)),
                reason: Some("code expired"),
            };
        }
        // Users may type the code with a dash, spaces or in lower case.
        let typed: String = code
            .chars()
            .filter(|c| !matches!(c, '-' | ' '))
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if constant_time_eq(typed.as_bytes(), self.code.as_bytes()) {
            let token = new_token();
            self.tokens.push((origin.to_owned(), token.clone()));
            return Attempt {
                token: Some(token),
                new_code: Some(self.rotate(now)),
                reason: None,
            };
        }
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            return Attempt {
                token: None,
                new_code: Some(self.rotate(now)),
                reason: Some("too many wrong codes; code replaced"),
            };
        }
        Attempt {
            token: None,
            new_code: None,
            reason: Some("wrong code"),
        }
    }

    /// Whether `token` was issued to `origin`.
    pub fn token_valid(&self, origin: &str, token: &str) -> bool {
        // Check every entry, so timing does not depend on which one matches.
        self.tokens.iter().fold(false, |found, (o, t)| {
            found | (o == origin && constant_time_eq(t.as_bytes(), token.as_bytes()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_tokens_look_right() {
        let p = Pairing::new(Instant::now());
        let code = p.code();
        assert_eq!(code.len(), 9);
        assert_eq!(&code[4..5], "-");
        assert!(p.link_code().bytes().all(|b| ALPHABET.contains(&b)));
        let a = new_token();
        assert_eq!(a.len(), 43);
        assert_ne!(a, new_token());
        assert_ne!(new_code(), new_code());
    }

    #[test]
    fn right_code_pairs_once() {
        let now = Instant::now();
        let mut p = Pairing::new(now);
        let code = p.code().to_lowercase().replace('-', " ");
        let ok = p.attempt("http://a", &code, now);
        let token = ok.token.expect("paired");
        assert!(ok.new_code.is_some());
        assert!(p.token_valid("http://a", &token));
        assert!(!p.token_valid("http://b", &token), "bound to the origin");
        assert!(!p.token_valid("http://a", "x"));
        // Single use.
        let again = p.attempt("http://a", &code, now);
        assert_eq!(again.token, None);
        assert_eq!(again.reason, Some("wrong code"));
    }

    #[test]
    fn wrong_codes_burn_the_code() {
        let now = Instant::now();
        let mut p = Pairing::new(now);
        let code = p.code();
        for _ in 1..MAX_FAILURES {
            let a = p.attempt("http://a", "0000-0000", now);
            assert_eq!((a.token, a.new_code), (None, None));
        }
        let last = p.attempt("http://a", "0000-0000", now);
        assert!(last.new_code.is_some());
        assert_eq!(p.attempt("http://a", &code, now).token, None);
    }

    #[test]
    fn codes_expire() {
        let start = Instant::now();
        let mut p = Pairing::new(start);
        let code = p.code();
        let late = p.attempt("http://a", &code, start + CODE_TTL);
        assert_eq!(late.token, None);
        assert_eq!(late.reason, Some("code expired"));
        let fresh = p.code();
        assert!(
            p.attempt("http://a", &fresh, start + CODE_TTL)
                .token
                .is_some()
        );
    }

    #[test]
    fn constant_time_eq_compares() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
