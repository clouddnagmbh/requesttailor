// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The error a hook returns. The host shows its message to the user; for `prepare` it stops
//! the send (`API.md`, "Extension points").

use std::fmt;

/// A message for the user. Every `std::error::Error` converts into it, so `?` works on
/// `serde_json`, base64 and UTF-8 errors; for plain text use [`Error::new`] or [`bail!`].
///
/// It deliberately does not implement `std::error::Error` itself (like `anyhow::Error`),
/// which is what allows the blanket conversion.
///
/// [`bail!`]: crate::bail
#[derive(Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }

    pub fn into_message(self) -> String {
        self.0
    }
}

impl<E: std::error::Error> From<E> for Error {
    fn from(e: E) -> Self {
        Self(e.to_string())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What every hook and host call returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Returns early with an [`Error`] built like `format!`.
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return ::core::result::Result::Err($crate::Error::new(::std::format!($($arg)*)))
    };
}
