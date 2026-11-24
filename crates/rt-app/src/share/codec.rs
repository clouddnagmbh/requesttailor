// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The text of a link: `1.` + base64url(deflate(JSON)). Pure Rust (`miniz_oxide`), the same
//! on every target.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::{FRAGMENT_KEY, SHARE_VERSION, ShareError, SharePayload, TEXT_PREFIX};

/// The longest payload text [`open`] reads.
pub const MAX_INPUT_LEN: usize = 128 * 1024;
/// The largest JSON a payload may inflate to (a guard against a crafted link).
const MAX_JSON_LEN: usize = 4 * 1024 * 1024;

/// `1.<base64url>` for a payload.
pub fn encode(payload: &SharePayload) -> Result<String, ShareError> {
    let json = serde_json::to_vec(payload).map_err(|e| ShareError::Invalid(e.to_string()))?;
    let packed = miniz_oxide::deflate::compress_to_vec(&json, 10);
    Ok(format!(
        "{SHARE_VERSION}.{}",
        URL_SAFE_NO_PAD.encode(packed)
    ))
}

/// The payload text inside a pasted link, fragment or `rt:` text; `None` when the text is
/// none of those. Reading it makes no request and runs nothing.
pub fn find_payload(input: &str) -> Option<&str> {
    let text = input.trim();
    if let Some(rest) = text.strip_prefix(TEXT_PREFIX) {
        return take(rest);
    }
    let marker = format!("{FRAGMENT_KEY}=");
    let fragment = match text.find('#') {
        Some(i) => &text[i + 1..],
        None => text.strip_prefix(&marker).map(|_| text)?,
    };
    // The fragment may hold more than `rt=…`; the key starts it or follows an `&`.
    fragment
        .split('&')
        .find_map(|part| part.strip_prefix(&marker))
        .and_then(take)
}

/// The first word of `text` if it looks like `<digits>.<base64url>`.
fn take(text: &str) -> Option<&str> {
    let word = text.split_whitespace().next()?;
    let (version, body) = word.split_once('.')?;
    (!version.is_empty()
        && version.bytes().all(|b| b.is_ascii_digit())
        && !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then_some(word)
}

/// Whether `input` is a link, fragment or `rt:` text of this app (before decoding it).
pub fn looks_shared(input: &str) -> bool {
    find_payload(input).is_some()
}

/// Reads a pasted link, a fragment (`#rt=…`) or an `rt:` payload.
pub fn decode(input: &str) -> Result<SharePayload, ShareError> {
    if input.len() > MAX_INPUT_LEN {
        return Err(ShareError::Invalid(
            "that text is too long to be a link".into(),
        ));
    }
    let not_ours = || ShareError::Invalid("this is not a RequestTailor link".to_owned());
    let word = find_payload(input).ok_or_else(not_ours)?;
    let (version, body) = word.split_once('.').ok_or_else(not_ours)?;
    let version: u32 = version.parse().map_err(|_| not_ours())?;
    if version != SHARE_VERSION {
        return Err(ShareError::Version(version));
    }
    let packed = URL_SAFE_NO_PAD
        .decode(body)
        .map_err(|_| ShareError::Invalid("the link is damaged (not base64)".into()))?;
    let json = miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, MAX_JSON_LEN)
        .map_err(|_| ShareError::Invalid("the link is damaged (cannot be unpacked)".into()))?;
    let payload: SharePayload = serde_json::from_slice(&json)
        .map_err(|e| ShareError::Invalid(format!("the link does not hold a request: {e}")))?;
    if payload.version != SHARE_VERSION {
        return Err(ShareError::Version(payload.version));
    }
    Ok(payload)
}
