// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The last line of defence of `rt mcp`: every secret value the process knows is replaced
//! by `***` in every message before it leaves (tool results, errors, approval prompts) and
//! in every audit line.
//!
//! Reports are already masked by the core (`rt_core::send`); this catches what the core
//! cannot know, e.g. a server that echoes a token back in its response body.
//!
//! What counts as secret: every `--var` value, values of secret variables read from the
//! keychain or a secret manager, and secret variables with a value in a file. Values
//! shorter than [`MIN_LEN`] characters are not replaced here (they would turn ordinary text
//! into `***` everywhere); the core still masks them where it knows them by name.
//!
//! Forms: the raw value and its percent-encoded forms (RFC 3986 and form encoding). Masking
//! works on parsed JSON strings, so JSON escaping does not hide a value.

use serde_json::Value;

use rt_core::send::MASK;

/// Shortest value masked at the boundary.
pub const MIN_LEN: usize = 4;

#[derive(Debug, Clone, Default)]
pub struct Masker {
    /// All forms of all secrets, longest first.
    forms: Vec<String>,
}

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

impl Masker {
    /// Adds a secret value (ignored if shorter than [`MIN_LEN`]).
    pub fn add(&mut self, secret: &str) {
        if secret.chars().count() < MIN_LEN {
            return;
        }
        for form in [
            secret.to_owned(),
            percent_encode(secret, false),
            percent_encode(secret, true),
            percent_encode(secret, false).to_lowercase(),
        ] {
            if !self.forms.contains(&form) {
                self.forms.push(form);
            }
        }
        self.forms
            .sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    }

    pub fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }

    /// `text` with every secret replaced by `***`.
    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for form in &self.forms {
            if out.contains(form.as_str()) {
                out = out.replace(form.as_str(), MASK);
            }
        }
        out
    }

    /// Masks every string and object key in `value`.
    pub fn value(&self, value: &mut Value) {
        if self.forms.is_empty() {
            return;
        }
        match value {
            Value::String(s) => {
                if self.forms.iter().any(|f| s.contains(f.as_str())) {
                    *s = self.text(s);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.value(v)),
            Value::Object(map) => {
                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    let masked = self.text(&key);
                    if masked != key
                        && let Some(v) = map.remove(&key)
                    {
                        map.insert(masked, v);
                    }
                }
                map.values_mut().for_each(|v| self.value(v));
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// The JSON line for `message`, masked; a final pass over the serialized text catches
    /// anything the structural pass could not see.
    pub fn line(&self, message: &Value) -> String {
        let mut message = message.clone();
        self.value(&mut message);
        let line = serde_json::to_string(&message).expect("JSON values serialize");
        if self.forms.iter().any(|f| line.contains(f.as_str())) {
            self.text(&line)
        } else {
            line
        }
    }
}

/// Keys in an `auth` object whose values are credentials.
const AUTH_SECRET_KEYS: &[&str] = &[
    "password",
    "token",
    "value",
    "client_secret",
    "secret_access_key",
    "session_token",
    "assertion",
    "access_token",
    "refresh_token",
    "passphrase",
];

/// Header names whose values are credentials.
const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
];

/// Whether `s` is only `{{variable}}` placeholders (and text around them that cannot be a
/// credential on its own, like `Bearer `).
fn only_placeholders(s: &str) -> bool {
    let mut rest = s.trim();
    for prefix in ["Bearer ", "Basic ", "Token "] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r.trim();
        }
    }
    if rest.is_empty() {
        return true;
    }
    while let Some(start) = rest.strip_prefix("{{") {
        let Some(end) = start.find("}}") else {
            return false;
        };
        rest = start[end + 2..].trim();
    }
    rest.is_empty()
}

fn mask_auth(auth: &mut Value) {
    match auth {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s)
                        if AUTH_SECRET_KEYS.contains(&key.as_str()) && !only_placeholders(s) =>
                    {
                        *s = MASK.to_owned();
                    }
                    _ => mask_auth(v),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask_auth),
        _ => {}
    }
}

/// A request spec (`RequestSpec` as JSON) as agents may see it: secret variables without
/// value, credentials typed into auth or into credential headers as `***`. Placeholders
/// (`{{name}}`) stay, so the agent sees which variable is used.
pub fn spec_for_agent(spec: &mut Value) {
    if let Some(Value::Array(vars)) = spec.get_mut("vars") {
        for var in vars {
            if var.get("secret") == Some(&Value::Bool(true))
                && let Some(obj) = var.as_object_mut()
            {
                obj.insert("value".into(), Value::Null);
            }
        }
    }
    if let Some(auth) = spec.get_mut("auth") {
        mask_auth(auth);
    }
    if let Some(Value::Array(headers)) = spec.get_mut("headers") {
        for h in headers {
            let secret = h
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| SECRET_HEADERS.contains(&n.to_ascii_lowercase().as_str()));
            if secret
                && let Some(Value::String(v)) = h.get_mut("value")
                && !only_placeholders(v)
            {
                *v = MASK.to_owned();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_raw_and_encoded_forms_in_strings_and_keys() {
        let mut m = Masker::default();
        m.add("s3cr3t token/1");
        m.add("abc"); // too short
        assert!(!m.is_empty());
        let v = json!({
            "url": "https://h/?t=s3cr3t%20token%2F1&u=s3cr3t+token%2F1",
            "body": "{\"echo\":\"s3cr3t token/1\"}",
            "s3cr3t token/1": [1, "x s3cr3t token/1 y", null],
            "short": "abc",
            "lower": "s3cr3t%20token%2f1",
        });
        let line = m.line(&v);
        assert!(!line.contains("s3cr3t"), "{line}");
        let back: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(back["url"], "https://h/?t=***&u=***");
        assert_eq!(back["body"], "{\"echo\":\"***\"}");
        assert_eq!(back["***"][1], "x *** y");
        assert_eq!(back["short"], "abc");
        assert_eq!(back["lower"], "***");
    }

    #[test]
    fn json_escaping_does_not_hide_a_secret() {
        let mut m = Masker::default();
        m.add("quo\"te\\back");
        let line = m.line(&json!({"a": "x quo\"te\\back y"}));
        assert!(!line.contains("quo\\\"te"), "{line}");
        assert!(line.contains("x *** y"), "{line}");
    }

    #[test]
    fn longest_secret_wins() {
        let mut m = Masker::default();
        m.add("abcd");
        m.add("abcdefgh");
        assert_eq!(m.text("abcdefgh abcd"), "*** ***");
    }

    #[test]
    fn specs_show_placeholders_but_no_credentials() {
        let mut spec = json!({
            "headers": [
                {"name": "Authorization", "value": "Bearer {{token}}", "enabled": true},
                {"name": "X-Api-Key", "value": "literal-key-123", "enabled": true},
                {"name": "Accept", "value": "application/json", "enabled": true},
            ],
            "auth": {"basic": {"username": "me", "password": "hunter22"}},
            "vars": [
                {"name": "pin", "value": "1234", "secret": true, "enabled": true},
                {"name": "id", "value": "7", "secret": false, "enabled": true},
            ],
        });
        spec_for_agent(&mut spec);
        assert_eq!(spec["headers"][0]["value"], "Bearer {{token}}");
        assert_eq!(spec["headers"][1]["value"], "***");
        assert_eq!(spec["headers"][2]["value"], "application/json");
        assert_eq!(spec["auth"]["basic"]["password"], "***");
        assert_eq!(spec["auth"]["basic"]["username"], "me");
        assert_eq!(spec["vars"][0]["value"], Value::Null);
        assert_eq!(spec["vars"][1]["value"], "7");
        let mut t = json!({"auth": {"bearer": {"token": "{{a}}{{b}}"}}});
        spec_for_agent(&mut t);
        assert_eq!(t["auth"]["bearer"]["token"], "{{a}}{{b}}");
    }
}
