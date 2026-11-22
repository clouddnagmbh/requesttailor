// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Helpers for the data in calls: bodies (text or base64), headers, importer files.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rt_plugin::api::{PluginBody, PluginFile};

use crate::Result;

/// A text body.
pub fn text_body(content_type: Option<&str>, text: impl Into<String>) -> PluginBody {
    PluginBody {
        content_type: content_type.map(str::to_owned),
        text: Some(text.into()),
        base64: None,
    }
}

/// A binary body (sent as base64).
pub fn binary_body(content_type: Option<&str>, bytes: &[u8]) -> PluginBody {
    PluginBody {
        content_type: content_type.map(str::to_owned),
        text: None,
        base64: Some(base64_encode(bytes)),
    }
}

/// The bytes of a body, whether it came as text or base64.
pub fn body_bytes(body: &PluginBody) -> Result<Vec<u8>> {
    match (&body.text, &body.base64) {
        (Some(text), _) => Ok(text.as_bytes().to_vec()),
        (None, Some(b64)) => Ok(STANDARD.decode(b64)?),
        (None, None) => Ok(Vec::new()),
    }
}

/// The value of the first header named `name` (case-insensitive).
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The content of an importer file.
pub fn file_bytes(file: &PluginFile) -> Result<Vec<u8>> {
    Ok(STANDARD.decode(&file.base64)?)
}

/// The content of an importer file as UTF-8 text.
pub fn file_text(file: &PluginFile) -> Result<String> {
    Ok(String::from_utf8(file_bytes(file)?)?)
}

/// An importer file from text (for tests).
pub fn text_file(name: &str, text: &str) -> PluginFile {
    PluginFile {
        name: name.to_owned(),
        base64: base64_encode(text.as_bytes()),
    }
}

/// Standard base64, as the API uses it.
pub fn base64_encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_decode_from_text_and_base64() {
        assert_eq!(body_bytes(&text_body(None, "hé")).unwrap(), "hé".as_bytes());
        assert_eq!(
            body_bytes(&binary_body(None, &[0, 255])).unwrap(),
            vec![0, 255]
        );
        assert_eq!(
            body_bytes(&PluginBody::default()).unwrap(),
            Vec::<u8>::new()
        );
        let bad = PluginBody {
            base64: Some("!!".into()),
            ..PluginBody::default()
        };
        assert!(body_bytes(&bad).is_err());
    }

    #[test]
    fn files_and_headers() {
        assert_eq!(file_text(&text_file("a.txt", "x\ny")).unwrap(), "x\ny");
        let headers = vec![("Content-Type".to_owned(), "text/plain".to_owned())];
        assert_eq!(header(&headers, "content-type"), Some("text/plain"));
        assert_eq!(header(&headers, "accept"), None);
    }
}
