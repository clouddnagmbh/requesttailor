// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The secret rule of sharing and copying, in one place: what a request may carry out of the
//! app. Everything that looks like a credential and is not already a `{{template}}` becomes
//! the placeholder `{{secret:<name>}}`; the values of secret variables never get here at all
//! (the model holds none). [`crate::secret_scan`] is the second net over the result.

use rt_core::model::{
    Auth, Body, KeyValue, MultipartPart, OAuth2, OAuth2Flow, Param, Protocol, RequestSpec, Variable,
};

/// Words that name a credential when they are part of a header, parameter, field, variable or
/// JSON key (case-insensitive).
const SENSITIVE_PARTS: [&str; 10] = [
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "signature",
    "cookie",
    "jwt",
    "bearer",
];

/// Names that are credentials as a whole.
const SENSITIVE_NAMES: [&str; 6] = ["authorization", "auth", "sig", "pwd", "sid", "sessionid"];

/// Whether a name (header, query parameter, form field, JSON key, variable) holds a
/// credential: `Authorization`, `Cookie`, `Proxy-Authorization`, and names containing `key`,
/// `token`, `secret` or `password` (and a few more of the same kind).
pub fn is_sensitive_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    lower == "proxy-authorization"
        || SENSITIVE_NAMES.contains(&lower.as_str())
        || SENSITIVE_PARTS.iter().any(|p| lower.contains(p))
}

/// Whether `value` carries no literal: it is empty, or only `{{templates}}` and the words of
/// an authentication scheme (`Bearer {{token}}`). Such a value is safe as it is.
pub fn is_clean(value: &str) -> bool {
    let mut rest = String::new();
    let mut s = value;
    while let Some(start) = s.find("{{") {
        rest.push_str(&s[..start]);
        match s[start..].find("}}") {
            Some(end) => s = &s[start + end + 2..],
            None => {
                rest.push_str(&s[start..]);
                s = "";
            }
        }
    }
    rest.push_str(s);
    let rest = rest.trim();
    rest.is_empty()
        || ["bearer", "basic", "token", "digest", "apikey"]
            .iter()
            .any(|w| rest.eq_ignore_ascii_case(w))
}

/// `Authorization` -> `authorization`, `X-Api-Key` -> `x_api_key`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_owned();
    if out.is_empty() {
        "secret".to_owned()
    } else {
        out
    }
}

pub fn placeholder(slug: &str) -> String {
    format!("{{{{secret:{slug}}}}}")
}

/// A request without credentials, and the names of the placeholders that stand in for them.
#[derive(Debug, Clone)]
pub struct Sanitized {
    pub spec: RequestSpec,
    pub placeholders: Vec<String>,
}

struct Scrub {
    placeholders: Vec<String>,
}

impl Scrub {
    fn note(&mut self, slug: String) -> String {
        let p = placeholder(&slug);
        if !self.placeholders.contains(&slug) {
            self.placeholders.push(slug);
        }
        p
    }

    /// `value` as it is when clean, else the placeholder named `slug`.
    fn value(&mut self, value: &str, slug_name: &str) -> String {
        if is_clean(value) {
            value.to_owned()
        } else {
            self.note(slug(slug_name))
        }
    }

    fn pair(&mut self, kv: &mut KeyValue) {
        if is_sensitive_name(&kv.name) {
            kv.value = self.value(&kv.value, &kv.name);
        }
        kv.value = self.text(&kv.value);
    }

    /// Free text (a header value, a body): JSON keys and XML tags that name a credential.
    fn text(&mut self, text: &str) -> String {
        let json = self.json(text);
        self.xml(&json)
    }

    /// `"password": "x"` -> `"password": "{{secret:password}}"`, formatting untouched.
    fn json(&mut self, text: &str) -> String {
        if !text.contains('"') {
            return text.to_owned();
        }
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        let mut copied = 0;
        while i < bytes.len() {
            if bytes[i] != b'"' {
                i += 1;
                continue;
            }
            let Some(end) = string_end(bytes, i) else {
                break;
            };
            let key = &text[i + 1..end];
            i = end + 1;
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) != Some(&b':') || !is_sensitive_name(key) {
                continue;
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) != Some(&b'"') {
                continue;
            }
            let Some(vend) = string_end(bytes, j) else {
                break;
            };
            let value = &text[j + 1..vend];
            if !is_clean(value) {
                out.push_str(&text[copied..j + 1]);
                out.push_str(&self.note(slug(key)));
                copied = vend;
            }
            i = vend + 1;
        }
        out.push_str(&text[copied..]);
        out
    }

    /// `<password>x</password>` -> `<password>{{secret:password}}</password>`.
    fn xml(&mut self, text: &str) -> String {
        if !text.contains('<') {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('<') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('>') else {
                break;
            };
            let tag = &after[..close];
            let plain = !tag.is_empty()
                && !tag.starts_with(['/', '?', '!'])
                && !tag.ends_with('/')
                && !tag.contains(char::is_whitespace);
            let local = tag.rsplit(':').next().unwrap_or(tag);
            if plain && is_sensitive_name(local) {
                let body_start = open + 1 + close + 1;
                let closing = format!("</{tag}>");
                if let Some(end) = rest[body_start..].find(&closing) {
                    let inner = &rest[body_start..body_start + end];
                    if !inner.contains('<') && !is_clean(inner) {
                        out.push_str(&rest[..body_start]);
                        out.push_str(&self.note(slug(local)));
                        rest = &rest[body_start + end..];
                        continue;
                    }
                }
            }
            out.push_str(&rest[..open + 1 + close + 1]);
            rest = &rest[open + 1 + close + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Credentials in a URL: `user:pass@` and query values named like a credential.
    fn url(&mut self, url: &str) -> String {
        let (main, fragment) = match url.find('#') {
            Some(i) => (&url[..i], &url[i..]),
            None => (url, ""),
        };
        let (before_query, query) = match main.find('?') {
            Some(i) => (&main[..i], Some(&main[i + 1..])),
            None => (main, None),
        };
        let mut out = self.userinfo(before_query);
        if let Some(query) = query {
            out.push('?');
            let pairs: Vec<String> = query
                .split('&')
                .map(|pair| match pair.split_once('=') {
                    Some((name, value)) if is_sensitive_name(&decode(name)) => {
                        format!("{name}={}", self.value(value, &decode(name)))
                    }
                    _ => pair.to_owned(),
                })
                .collect();
            out.push_str(&pairs.join("&"));
        }
        out.push_str(fragment);
        out
    }

    fn userinfo(&mut self, base: &str) -> String {
        let Some(scheme_end) = base.find("://") else {
            return base.to_owned();
        };
        let start = scheme_end + 3;
        let authority_end = base[start..].find('/').map_or(base.len(), |i| start + i);
        let authority = &base[start..authority_end];
        let Some(at) = authority.rfind('@') else {
            return base.to_owned();
        };
        let userinfo = &authority[..at];
        let Some((user, pass)) = userinfo.split_once(':') else {
            return base.to_owned();
        };
        let pass = self.value(pass, "password");
        format!(
            "{}{user}:{pass}{}",
            &base[..start],
            &base[start + userinfo.len()..]
        )
    }

    fn param(&mut self, p: &mut Param) {
        if is_sensitive_name(&p.name) {
            p.value = self.value(&p.value, &p.name);
        }
    }

    fn part(&mut self, p: &mut MultipartPart) {
        if is_sensitive_name(&p.name) && p.kind == rt_core::model::PartKind::Text {
            for v in &mut p.values {
                *v = self.value(v, &p.name);
            }
        }
        if p.kind == rt_core::model::PartKind::Text {
            for v in &mut p.values {
                *v = self.text(v);
            }
        }
    }

    fn oauth2(&mut self, o: &mut OAuth2) {
        o.client_secret = self.value(&o.client_secret, "client_secret");
        match &mut o.flow {
            OAuth2Flow::Password { password, .. } => *password = self.value(password, "password"),
            OAuth2Flow::Saml2Bearer { assertion } => {
                *assertion = self.value(assertion, "assertion")
            }
            _ => {}
        }
        for param in &mut o.parameters {
            if is_sensitive_name(&param.name) {
                param.value = self.value(&param.value, &param.name);
            }
        }
    }

    fn auth(&mut self, auth: &mut Auth) {
        match auth {
            Auth::Basic { password, .. } => *password = self.value(password, "password"),
            Auth::Bearer { token } => *token = self.value(token, "token"),
            Auth::ApiKey { key, value, .. } => {
                let name = if key.trim().is_empty() || !is_clean(key) {
                    "api_key"
                } else {
                    key
                };
                *value = self.value(value, name);
            }
            Auth::OAuth2(o) => self.oauth2(o),
            Auth::AwsSigV4(a) => {
                a.access_key_id = self.value(&a.access_key_id, "access_key_id");
                a.secret_access_key = self.value(&a.secret_access_key, "secret_access_key");
                a.session_token = self.value(&a.session_token, "session_token");
            }
            Auth::Inherit | Auth::None | Auth::Unsupported { .. } => {}
        }
    }

    fn variable(&mut self, v: &mut Variable) {
        let literal = v.value.as_deref().is_some_and(|x| !is_clean(x));
        if v.secret || (is_sensitive_name(&v.name) && literal) {
            v.secret = true;
            v.value = None;
        }
    }
}

/// Index of the closing quote of the JSON string that opens at `open`.
fn string_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Percent-decoding enough to read a parameter name.
fn decode(name: &str) -> String {
    let b = name.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The request without anything that names a credential and is not a `{{template}}`.
pub fn sanitize(spec: &RequestSpec) -> Sanitized {
    let mut s = Scrub {
        placeholders: Vec::new(),
    };
    let mut spec = spec.clone();
    spec.seq = None;
    spec.url = s.url(&spec.url);
    for h in &mut spec.headers {
        s.pair(h);
    }
    for p in &mut spec.params {
        s.param(p);
    }
    match &mut spec.body {
        Body::None | Body::File { .. } => {}
        Body::Raw { data, .. } => *data = s.text(data),
        Body::FormUrlEncoded { fields } => {
            for f in fields {
                s.pair(f);
            }
        }
        Body::Multipart { parts } => {
            for p in parts {
                s.part(p);
            }
        }
        Body::GraphQl { variables, .. } => *variables = s.text(variables),
    }
    s.auth(&mut spec.auth);
    for v in &mut spec.vars {
        s.variable(v);
    }
    match &mut spec.protocol {
        Protocol::Http => {}
        Protocol::Grpc(g) => {
            for m in &mut g.messages {
                m.data = s.text(&m.data);
            }
        }
        Protocol::WebSocket(w) => {
            for m in &mut w.messages {
                m.data = s.text(&m.data);
            }
        }
    }
    Sanitized {
        spec,
        placeholders: s.placeholders,
    }
}

/// The names `{{name}}` of the templates in `text`, in order of appearance (`secret:x` as
/// `secret:x`).
pub fn template_names(text: &str, out: &mut Vec<String>) {
    let mut s = text;
    while let Some(start) = s.find("{{") {
        let after = &s[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty() && !name.contains('\n') && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
        s = &after[end + 2..];
    }
}

/// Every name used in a (sanitized) request.
pub fn used_names(spec: &RequestSpec) -> Vec<String> {
    let mut out = Vec::new();
    let json = serde_json::to_value(spec).unwrap_or(serde_json::Value::Null);
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => template_names(s, out),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(&json, &mut out);
    out
}
