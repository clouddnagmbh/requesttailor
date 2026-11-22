// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret values never reach a plugin: everything handed over is masked with `***` first,
//! the same way `rt_core::send` masks its report (raw, percent-encoded and Debug-escaped
//! forms, HTTP Basic credentials that contain a secret).

use base64::Engine as _;

pub use rt_core::send::MASK;

/// Masks a fixed set of secret values.
#[derive(Debug, Clone, Default)]
pub struct Masker {
    /// The secrets in every form that is replaced, longest first (so a secret that contains
    /// another one is masked as a whole).
    forms: Vec<String>,
    secrets: Vec<String>,
}

impl Masker {
    pub fn new(secrets: &[String]) -> Self {
        let mut forms: Vec<String> = Vec::new();
        for secret in secrets.iter().filter(|s| !s.is_empty()) {
            forms.push(secret.clone());
            forms.push(secret.escape_debug().to_string());
            forms.push(percent_encode(secret, false));
            forms.push(percent_encode(secret, true));
        }
        forms.retain(|f| !f.is_empty());
        forms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        forms.dedup();
        let mut secrets: Vec<String> = secrets.iter().filter(|s| !s.is_empty()).cloned().collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Self { forms, secrets }
    }

    pub fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }

    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for form in &self.forms {
            if out.contains(form.as_str()) {
                out = out.replace(form.as_str(), MASK);
            }
        }
        out
    }

    /// Whether `text` contains a secret in any of its forms.
    pub fn hits(&self, text: &str) -> bool {
        self.forms.iter().any(|f| text.contains(f.as_str())) || self.basic_hit(text)
    }

    /// A header value: like [`Masker::text`], and `Basic ***` for Basic credentials that
    /// contain a secret (base64 hides it from substring matching).
    pub fn header(&self, value: &str) -> String {
        if self.basic_hit(value) {
            let scheme = value.split_once(' ').map_or("Basic", |(s, _)| s);
            return format!("{scheme} {MASK}");
        }
        self.text(value)
    }

    fn basic_hit(&self, value: &str) -> bool {
        if let Some((scheme, credentials)) = value.split_once(' ')
            && scheme.eq_ignore_ascii_case("basic")
            && let Ok(decoded) =
                base64::engine::general_purpose::STANDARD.decode(credentials.trim())
        {
            let decoded = String::from_utf8_lossy(&decoded);
            return self.secrets.iter().any(|s| decoded.contains(s.as_str()));
        }
        false
    }

    /// Bytes that are not UTF-8: every raw occurrence of a secret is replaced.
    pub fn bytes(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        for secret in &self.secrets {
            out = replace_bytes(&out, secret.as_bytes(), MASK.as_bytes());
        }
        out
    }
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// RFC 3986 encoding of everything but unreserved characters; `plus` encodes space as `+`.
fn percent_encode(s: &str, plus: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' if plus => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_every_form() {
        let m = Masker::new(&["p@ss word".into(), "tok".into(), String::new()]);
        assert_eq!(m.text("a=p%40ss%20word&b=tok"), "a=***&b=***");
        assert_eq!(m.text("a=p%40ss+word"), "a=***");
        assert_eq!(m.text("nothing"), "nothing");
        let basic = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("user:p@ss word")
        );
        assert_eq!(m.header(&basic), "Basic ***");
        assert!(m.hits(&basic));
        assert_eq!(m.header("Bearer tok"), "Bearer ***");
        assert_eq!(m.bytes(b"\xff tok \xfe"), b"\xff *** \xfe");
        assert!(Masker::new(&[]).is_empty());
    }
}
