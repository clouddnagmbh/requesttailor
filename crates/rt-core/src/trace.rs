// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Trace context for sends (wave 19, strand A): the W3C `traceparent` header
//! (`00-<32 hex trace id>-<16 hex span id>-<2 hex flags>`), made and read here, nothing else.
//! `tracestate`, baggage and vendor headers are never written or changed.
//!
//! **Off by default.** `send_with` calls [`inject`] after `prepare` and before the guard; it
//! sets the header only if the collection switched it on
//! (`extensions.requesttailor.trace.inject: true`, [`TraceCollectionConfig`]) **and** the
//! request has no `traceparent` of its own. With it off the send path is byte for byte the
//! one of wave 18 and costs nothing. The id is never derived from a secret.
//!
//! A response that carries a trace id (`traceparent`, `x-trace-id`) of its own is read by
//! [`from_response`]; its id is reported with `injected: false`.

use serde::{Deserialize, Serialize};

pub use crate::model::TraceCollectionConfig;

/// The setting `send_with` hands to [`inject`].
pub type TraceConfig = TraceCollectionConfig;

/// A parsed or made `traceparent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub flags: u8,
}

/// Why a `traceparent` could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceParseError {
    /// Not four `-` separated fields of the right widths, or not lower-case hex.
    Malformed,
    /// Version `ff` is invalid; this reader knows version `00`.
    Version,
    /// An all-zero trace or span id is invalid.
    Zero,
}

impl TraceContext {
    /// A fresh context from the system's random source: version 00, sampled flag set.
    pub fn new() -> Self {
        let mut bytes = [0u8; 24];
        // The same source OAuth uses (the browser's `crypto.getRandomValues` on wasm32).
        // Should the source fail, fall back to a clock-seeded mix rather than all zeros.
        if getrandom::getrandom(&mut bytes).is_err() {
            let seed = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64 | 1);
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = (seed.rotate_left((i as u32 * 7) % 64) >> (i % 8)) as u8 | 1;
            }
        }
        Self::from_bytes(bytes)
    }

    /// A context from 24 bytes (16 trace id, 8 span id); an all-zero half becomes `…01`, so
    /// the result is always valid.
    pub fn from_bytes(bytes: [u8; 24]) -> Self {
        let mut trace_id = [0u8; 16];
        let mut span_id = [0u8; 8];
        trace_id.copy_from_slice(&bytes[..16]);
        span_id.copy_from_slice(&bytes[16..]);
        if trace_id == [0u8; 16] {
            trace_id[15] = 1;
        }
        if span_id == [0u8; 8] {
            span_id[7] = 1;
        }
        Self {
            trace_id,
            span_id,
            flags: 1,
        }
    }

    /// Reads a `traceparent` value. Later versions with extra fields are read as version 00
    /// (the first four fields), as the specification asks.
    pub fn parse(value: &str) -> Result<Self, TraceParseError> {
        let mut fields = value.trim().split('-');
        let (Some(version), Some(trace), Some(span), Some(flags)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(TraceParseError::Malformed);
        };
        if version.len() != 2 || trace.len() != 32 || span.len() != 16 || flags.len() != 2 {
            return Err(TraceParseError::Malformed);
        }
        if version == "ff" {
            return Err(TraceParseError::Version);
        }
        if version == "00" && fields.next().is_some() {
            return Err(TraceParseError::Malformed);
        }
        let mut trace_id = [0u8; 16];
        let mut span_id = [0u8; 8];
        hex_into(trace, &mut trace_id)?;
        hex_into(span, &mut span_id)?;
        let mut flag = [0u8; 1];
        hex_into(flags, &mut flag)?;
        hex_byte(version)?;
        if trace_id == [0u8; 16] || span_id == [0u8; 8] {
            return Err(TraceParseError::Zero);
        }
        Ok(Self {
            trace_id,
            span_id,
            flags: flag[0],
        })
    }

    /// `00-<trace id>-<span id>-<flags>`.
    pub fn header_value(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            hex(&self.trace_id),
            hex(&self.span_id),
            self.flags
        )
    }

    /// The trace id as 32 lower-case hex characters.
    pub fn trace_id_hex(&self) -> String {
        hex(&self.trace_id)
    }

    /// The span id as 16 lower-case hex characters.
    pub fn span_id_hex(&self) -> String {
        hex(&self.span_id)
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn hex_byte(pair: &str) -> Result<u8, TraceParseError> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(TraceParseError::Malformed),
    };
    let b = pair.as_bytes();
    Ok(digit(b[0])? << 4 | digit(b[1])?)
}

fn hex_into(text: &str, out: &mut [u8]) -> Result<(), TraceParseError> {
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = hex_byte(&text[i * 2..i * 2 + 2])?;
    }
    Ok(())
}

/// What the report says about the trace of a send (`SendReport.trace`, `Exchange.trace`).
/// Never carries a secret value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceRef {
    /// 32 lower-case hex characters.
    pub trace_id: String,
    /// 16 lower-case hex characters; empty when the id came from the response.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub span_id: String,
    /// The app put the `traceparent` on the request (`false`: the request had one of its own
    /// or the id came from the response).
    pub injected: bool,
}

/// Called by `send_with` after `prepare` and before the guard: puts a `traceparent` on the
/// request if `config.inject` is on and the request has none, and returns what to report.
/// Off: touches nothing and answers `None`. A `traceparent` the request already has stays as
/// it is (reported with `injected: false`; an unreadable one is left alone and not reported).
/// No other header is ever written.
pub fn inject(
    request: &mut crate::transport::HttpRequest,
    config: &TraceConfig,
) -> Option<TraceRef> {
    if !config.inject {
        return None;
    }
    if let Some((_, own)) = request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("traceparent"))
    {
        return TraceContext::parse(own).ok().map(|c| TraceRef {
            trace_id: c.trace_id_hex(),
            span_id: c.span_id_hex(),
            injected: false,
        });
    }
    let context = TraceContext::new();
    request
        .headers
        .push(("traceparent".to_owned(), context.header_value()));
    Some(TraceRef {
        trace_id: context.trace_id_hex(),
        span_id: context.span_id_hex(),
        injected: true,
    })
}

/// The trace id a response carries (`traceparent`, else `x-trace-id` with 32 hex characters),
/// used when the request had none. Reported with `injected: false`.
pub fn from_response(headers: &[(String, String)]) -> Option<TraceRef> {
    let find = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.trim())
    };
    if let Some(c) = find("traceparent").and_then(|v| TraceContext::parse(v).ok()) {
        return Some(TraceRef {
            trace_id: c.trace_id_hex(),
            span_id: c.span_id_hex(),
            injected: false,
        });
    }
    let id = find("x-trace-id")?.to_ascii_lowercase();
    let valid = id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && id.bytes().any(|b| b != b'0');
    valid.then_some(TraceRef {
        trace_id: id,
        span_id: String::new(),
        injected: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_contexts_are_valid_and_round_trip() {
        let context = TraceContext::new();
        let text = context.header_value();
        assert_eq!(text.len(), 55);
        assert_eq!(TraceContext::parse(&text), Ok(context));
    }

    #[test]
    fn zero_ids_and_bad_versions_are_refused() {
        let zero = format!("00-{}-{}-01", "0".repeat(32), "1".repeat(16));
        assert_eq!(TraceContext::parse(&zero), Err(TraceParseError::Zero));
        let version = format!("ff-{}-{}-01", "1".repeat(32), "1".repeat(16));
        assert_eq!(TraceContext::parse(&version), Err(TraceParseError::Version));
        assert_eq!(
            TraceContext::parse("00-short-1-01"),
            Err(TraceParseError::Malformed)
        );
    }

    #[test]
    fn the_specification_example_parses() {
        let c =
            TraceContext::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").unwrap();
        assert_eq!(c.trace_id_hex(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(c.span_id_hex(), "00f067aa0ba902b7");
        assert_eq!(c.flags, 1);
    }
}
