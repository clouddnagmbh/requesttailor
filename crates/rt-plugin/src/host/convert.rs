// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Between the core's types and the plugin API's JSON types: masked requests and responses
//! for plugins, requests from plugins, and applying a `prepare` patch.

use base64::Engine as _;
use rt_core::transport::{HttpBody, HttpRequest, ResponseHead};

use super::mask::{MASK, Masker};
use crate::api::{PluginBody, PluginRequest, PluginResponse, RequestPatch};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// A body for a plugin: UTF-8 as `text`, anything else as `base64`, secrets masked. A body
/// cut at a limit in the middle of a UTF-8 character is passed as text without it.
pub fn body_to_plugin(
    content_type: Option<&str>,
    bytes: &[u8],
    truncated: bool,
    masker: &Masker,
) -> PluginBody {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(e) if truncated && e.error_len().is_none() => {
            std::str::from_utf8(&bytes[..e.valid_up_to()]).ok()
        }
        Err(_) => None,
    };
    PluginBody {
        content_type: content_type.map(|c| masker.text(c)),
        text: text.map(|t| masker.text(t)),
        base64: match text {
            Some(_) => None,
            None => Some(B64.encode(masker.bytes(bytes))),
        },
    }
}

/// The bytes of a body a plugin sent.
pub fn body_from_plugin(body: &PluginBody) -> Result<HttpBody, String> {
    let bytes = match (&body.text, &body.base64) {
        (Some(text), None) => text.clone().into_bytes(),
        (None, Some(b64)) => B64
            .decode(b64.trim())
            .map_err(|e| format!("invalid base64 body: {e}"))?,
        (None, None) => Vec::new(),
        (Some(_), Some(_)) => return Err("a body has either text or base64, not both".into()),
    };
    Ok(HttpBody {
        content_type: body.content_type.clone(),
        bytes: bytes.into(),
    })
}

/// The request as a plugin sees it: every secret value replaced by `***`.
pub fn request_to_plugin(request: &HttpRequest, masker: &Masker) -> PluginRequest {
    PluginRequest {
        method: request.method.clone(),
        url: masker.text(&request.url),
        headers: request
            .headers
            .iter()
            .map(|(n, v)| (n.clone(), masker.header(v)))
            .collect(),
        body: request
            .body
            .as_ref()
            .map(|b| body_to_plugin(b.content_type.as_deref(), &b.bytes, false, masker)),
    }
}

fn check_header(name: &str, value: &str) -> Result<(), String> {
    let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
    if name.is_empty() || !name.chars().all(token) {
        return Err(format!("invalid header name {name:?}"));
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(format!("invalid value for header {name}"));
    }
    Ok(())
}

/// A request a plugin wants sent (`http` host call). Only absolute `http(s)` URLs; redirects
/// are not followed, so every host the request reaches is checked against the permission.
pub fn request_from_plugin(request: &PluginRequest) -> Result<HttpRequest, String> {
    let method = request.method.trim().to_ascii_uppercase();
    if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("invalid method {:?}", request.method));
    }
    for (name, value) in &request.headers {
        check_header(name, value)?;
    }
    let url = url::Url::parse(&request.url).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("only http and https URLs, not {}", url.scheme()));
    }
    Ok(HttpRequest {
        method,
        url: url.to_string(),
        headers: request.headers.clone(),
        body: request.body.as_ref().map(body_from_plugin).transpose()?,
        options: rt_core::transport::SendOptions {
            timeout: Some(std::time::Duration::from_secs(30)),
            follow_redirects: false,
            max_redirects: 0,
            client_identity: None,
        },
    })
}

/// A response for a plugin, with the body cut at `limit` and secrets masked.
pub fn response_to_plugin(
    head: &ResponseHead,
    body: &[u8],
    truncated: bool,
    limit: usize,
    masker: &Masker,
) -> PluginResponse {
    let cut = body.len() > limit;
    let body = &body[..body.len().min(limit)];
    let content_type = head
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());
    PluginResponse {
        status: head.status,
        headers: head
            .headers
            .iter()
            .map(|(n, v)| (n.clone(), masker.header(v)))
            .collect(),
        url: masker.text(&head.url),
        body: body_to_plugin(content_type, body, truncated || cut, masker),
        truncated: truncated || cut,
    }
}

/// Whether a patch would send the mask (a plugin only ever saw `***` in place of a secret).
pub fn patch_sends_mask(patch: &RequestPatch) -> Option<&'static str> {
    if patch.url.as_deref().is_some_and(|u| u.contains(MASK)) {
        return Some("url");
    }
    if patch
        .set_headers
        .iter()
        .any(|(n, v)| n.contains(MASK) || v.contains(MASK))
    {
        return Some("set_headers");
    }
    if let Some(body) = &patch.body {
        let in_text = body.text.as_deref().is_some_and(|t| t.contains(MASK));
        let in_b64 = body
            .base64
            .as_deref()
            .and_then(|b| B64.decode(b.trim()).ok())
            .is_some_and(|b| b.windows(MASK.len()).any(|w| w == MASK.as_bytes()));
        if in_text || in_b64 {
            return Some("body");
        }
    }
    None
}

/// Applies `patch` to the real (unmasked) request, in the order of `RequestPatch`: `url`,
/// `remove_headers`, `set_headers`, `body`.
pub fn apply_patch(request: &mut HttpRequest, patch: &RequestPatch) -> Result<(), String> {
    if let Some(field) = patch_sends_mask(patch) {
        return Err(format!(
            "patch refused: its {field} contains {MASK}, which would send the mask"
        ));
    }
    if let Some(url) = &patch.url {
        let parsed = url::Url::parse(url).map_err(|e| format!("patch has an invalid URL: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("patch URL must be http or https".into());
        }
        request.url = url.clone();
    }
    for name in &patch.remove_headers {
        request
            .headers
            .retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    }
    for (name, value) in &patch.set_headers {
        check_header(name, value)?;
        match request
            .headers
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            Some(i) => {
                request.headers[i] = (name.clone(), value.clone());
                let mut k = i + 1;
                while k < request.headers.len() {
                    if request.headers[k].0.eq_ignore_ascii_case(name) {
                        request.headers.remove(k);
                    } else {
                        k += 1;
                    }
                }
            }
            None => request.headers.push((name.clone(), value.clone())),
        }
    }
    if let Some(body) = &patch.body {
        request.body = Some(body_from_plugin(body)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::transport::{HttpVersion, Timing};

    #[test]
    fn response_bodies_are_cut_and_masked() {
        let head = ResponseHead {
            status: 200,
            version: HttpVersion::Http11,
            headers: vec![
                ("Content-Type".into(), "text/plain".into()),
                ("X-Echo".into(), "Bearer tok".into()),
            ],
            url: "https://a.test/?k=tok".into(),
            redirects: Vec::new(),
            timing: Timing::default(),
        };
        let m = Masker::new(&["tok".into()]);
        let r = response_to_plugin(&head, "héllo tok".as_bytes(), false, 2, &m);
        assert!(r.truncated);
        assert_eq!(r.body.text.as_deref(), Some("h"), "cut inside é");
        let r = response_to_plugin(&head, b"echo tok", false, 100, &m);
        assert_eq!(r.body.text.as_deref(), Some("echo ***"));
        assert_eq!(r.headers[1].1, "Bearer ***");
        assert_eq!(r.url, "https://a.test/?k=***");
        assert_eq!(r.body.content_type.as_deref(), Some("text/plain"));
        let r = response_to_plugin(&head, b"\xff\xfe", false, 100, &m);
        assert_eq!(r.body.base64.as_deref(), Some("//4="));
    }

    #[test]
    fn requests_from_plugins_are_checked() {
        let req = |method: &str, url: &str, headers: Vec<(String, String)>| PluginRequest {
            method: method.into(),
            url: url.into(),
            headers,
            body: Some(PluginBody {
                content_type: None,
                text: None,
                base64: Some("aGk=".into()),
            }),
        };
        let ok = request_from_plugin(&req("get", "https://a.test/x", vec![])).unwrap();
        assert_eq!(ok.method, "GET");
        assert_eq!(&ok.body.unwrap().bytes[..], b"hi");
        assert!(!ok.options.follow_redirects);
        assert!(request_from_plugin(&req("G T", "https://a.test/", vec![])).is_err());
        assert!(request_from_plugin(&req("GET", "data:text/plain,x", vec![])).is_err());
        let bad = vec![("X".to_owned(), "a\r\nInjected: 1".to_owned())];
        assert!(request_from_plugin(&req("GET", "https://a.test/", bad)).is_err());
    }
}
