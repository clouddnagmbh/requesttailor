// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Changing an envelope in a request without rewriting it: the SOAP version (the namespace
//! and the headers that go with it), the `SOAPAction`, and the WS-Security header. The body
//! text stays as the user wrote it except for the parts named here.

use rt_plugin_sdk::model::{Body, KeyValue, RawKind, RequestSpec};

use crate::envelope::{self, SoapVersion};
use crate::security::{self, Security};
use crate::xml;

/// What the SOAP tab shows and changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub version: SoapVersion,
    pub action: String,
    pub security: Option<Security>,
}

/// The text of a request's raw body.
pub fn body_text(spec: &RequestSpec) -> Option<&str> {
    match &spec.body {
        Body::Raw { data, .. } => Some(data),
        _ => None,
    }
}

fn header_value<'a>(spec: &'a RequestSpec, name: &str) -> Option<&'a str> {
    spec.headers
        .iter()
        .find(|h| h.enabled && h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// The action of a request: `SOAPAction` (quotes removed) or the `action` parameter of the
/// `Content-Type`.
pub fn action_of(spec: &RequestSpec) -> String {
    if let Some(a) = header_value(spec, "SOAPAction") {
        return a.trim().trim_matches('"').to_owned();
    }
    header_value(spec, "Content-Type")
        .and_then(|ct| {
            ct.split(';')
                .skip(1)
                .find_map(|p| p.trim().strip_prefix("action="))
        })
        .map(|a| a.trim().trim_matches('"').to_owned())
        .unwrap_or_default()
}

/// Reads the settings from a request whose body is an envelope.
pub fn read(spec: &RequestSpec) -> Option<Settings> {
    let text = body_text(spec)?;
    let env = envelope::parse(text)?;
    let security = read_security(text);
    Some(Settings {
        version: env.version,
        action: action_of(spec),
        security,
    })
}

fn read_security(text: &str) -> Option<Security> {
    let (start, end) = find_by_local(text, "Security")?;
    let block = &text[start..end];
    let username_token = block.contains("UsernameToken");
    let (user_var, password_var) = if username_token {
        security::username_token_vars(block)?
    } else {
        (security::USER_VAR.into(), security::PASSWORD_VAR.into())
    };
    let timestamp_ttl = match (security::expires(block), created(block)) {
        (Some(e), Some(c)) if e > c => Some((e - c) / 1000),
        _ if block.contains("Timestamp") => Some(security::DEFAULT_TTL),
        _ => None,
    };
    Some(Security {
        username_token,
        user_var,
        password_var,
        timestamp_ttl,
    })
}

fn created(block: &str) -> Option<u64> {
    let (_, rest) = block.split_once("Created>")?;
    let value = rest.split('<').next()?;
    security::parse_iso(value)
}

/// Whether the security header holds only what [`apply`] writes (a Timestamp and a
/// UsernameToken); anything else (a signature, a binary token) must not be rewritten.
fn is_simple_security(block: &str) -> bool {
    match xml_children(block) {
        Some(children) => children
            .iter()
            .all(|c| c == "Timestamp" || c == "UsernameToken"),
        None => false,
    }
}

fn xml_children(block: &str) -> Option<Vec<String>> {
    // The block alone is not namespace-complete when it relies on declarations of the
    // envelope, so list the child tags textually: tags directly below the first one.
    let mut depth = 0usize;
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        let end = rest.find('>')?;
        let tag = &rest[1..end];
        if tag.starts_with('?') || tag.starts_with('!') {
            rest = &rest[end + 1..];
            continue;
        }
        if let Some(close) = tag.strip_prefix('/') {
            let _ = close;
            depth = depth.saturating_sub(1);
        } else {
            let name = tag
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            if depth == 1 {
                out.push(name.rsplit(':').next().unwrap_or(name).to_owned());
            }
            if !tag.ends_with('/') {
                depth += 1;
            }
        }
        rest = &rest[end + 1..];
    }
    Some(out)
}

/// Byte range of the first element whose local name is `local` (start of `<` to the end of
/// the closing tag, or of the self-closing tag).
pub fn find_by_local(text: &str, local: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(i) = text[from..].find('<') {
        let start = from + i;
        let tag_end = start + text[start..].find('>')?;
        let tag = &text[start + 1..tag_end];
        if !tag.starts_with(['/', '?', '!']) {
            let name = tag
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            if name.rsplit(':').next() == Some(local) {
                if tag.ends_with('/') {
                    return Some((start, tag_end + 1));
                }
                let mut search = tag_end + 1;
                let mut depth = 1usize;
                while let Some(j) = text[search..].find('<') {
                    let s = search + j;
                    let e = s + text[s..].find('>')?;
                    let inner = &text[s + 1..e];
                    let n = inner
                        .trim_start_matches('/')
                        .split(|c: char| c.is_whitespace() || c == '/')
                        .next()
                        .unwrap_or("");
                    if n.rsplit(':').next() == Some(local) {
                        if inner.starts_with('/') {
                            depth -= 1;
                            if depth == 0 {
                                return Some((start, e + 1));
                            }
                        } else if !inner.ends_with('/') {
                            depth += 1;
                        }
                    }
                    search = e + 1;
                }
                return None;
            }
        }
        from = tag_end + 1;
    }
    None
}

/// The prefix of the `Envelope` element (`soapenv`, `soap`, `""`).
fn envelope_prefix(text: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = text[from..].find('<') {
        let start = from + i;
        let end = start + text[start..].find('>')?;
        let tag = &text[start + 1..end];
        if !tag.starts_with(['?', '!', '/']) {
            let name = tag
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            return match name.split_once(':') {
                Some((p, "Envelope")) => Some(p.to_owned()),
                None if name == "Envelope" => Some(String::new()),
                _ => None,
            };
        }
        from = end + 1;
    }
    None
}

fn set_header(spec: &mut RequestSpec, name: &str, value: Option<String>) {
    spec.headers.retain(|h| !h.name.eq_ignore_ascii_case(name));
    if let Some(v) = value {
        spec.headers.push(KeyValue::new(name, v));
    }
}

/// An envelope with an empty body, or around an XML body that is not an envelope.
pub fn wrap(version: SoapVersion, existing: &str) -> String {
    let mut inner = String::new();
    if let Ok(root) = xml::parse(existing) {
        let _ = root;
        for line in existing
            .lines()
            .filter(|l| !l.trim_start().starts_with("<?xml"))
        {
            inner.push_str("    ");
            inner.push_str(line.trim_end());
            inner.push('\n');
        }
    }
    format!(
        "<soapenv:Envelope xmlns:soapenv=\"{}\">\n  <soapenv:Header/>\n  <soapenv:Body>\n{inner}  </soapenv:Body>\n</soapenv:Envelope>",
        version.namespace()
    )
}

/// Applies `settings` to the request: the envelope's version and security header, and the
/// `Content-Type`/`SOAPAction` headers. A body that is no envelope is wrapped first.
pub fn apply(spec: &mut RequestSpec, settings: &Settings, now_ms: u64) -> Result<(), String> {
    let existing = body_text(spec).unwrap_or("").to_owned();
    let mut text = if envelope::parse(&existing).is_some() {
        existing
    } else {
        wrap(settings.version, &existing)
    };
    let current = envelope::parse(&text)
        .map(|e| e.version)
        .unwrap_or(settings.version);
    if current != settings.version {
        text = text.replace(current.namespace(), settings.version.namespace());
    }
    let prefix = envelope_prefix(&text).ok_or("the body is not a SOAP envelope")?;
    // The security header.
    if let Some((s, e)) = find_by_local(&text, "Security") {
        if !is_simple_security(&text[s..e]) {
            if settings.security.is_some() {
                return Err("the security header holds more than a UsernameToken and a Timestamp (a signature or a token?); edit it in the body".into());
            }
        } else {
            let line_start = text[..s]
                .rfind('\n')
                .map(|i| i + 1)
                .filter(|&i| text[i..s].trim().is_empty())
                .unwrap_or(s);
            let line_end = if text[e..].starts_with('\n') {
                e + 1
            } else {
                e
            };
            text.replace_range(line_start..line_end, "");
        }
    }
    if let Some(sec) = &settings.security {
        if sec.username_token || sec.timestamp_ttl.is_some() {
            let pfx = if prefix.is_empty() {
                "soapenv".to_owned()
            } else {
                prefix.clone()
            };
            let block = security::header(settings.version, &pfx, sec, now_ms);
            let indented: String = block.lines().map(|l| format!("    {l}\n")).collect();
            let qual = |n: &str| {
                if prefix.is_empty() {
                    n.to_owned()
                } else {
                    format!("{prefix}:{n}")
                }
            };
            match find_by_local(&text, "Header") {
                Some((s, e)) if text[s..e].ends_with("/>") => {
                    let open = qual("Header");
                    text.replace_range(s..e, &format!("<{open}>\n{indented}  </{open}>"));
                }
                Some((s, _)) => {
                    let open_end = s + text[s..].find('>').map_or(0, |i| i + 1);
                    text.insert_str(open_end, &format!("\n{}", indented.trim_end_matches('\n')));
                }
                None => {
                    let (bs, _) = find_by_local(&text, "Body").ok_or("the envelope has no Body")?;
                    let open = qual("Header");
                    text.insert_str(bs, &format!("<{open}>\n{indented}  </{open}>\n  "));
                }
            }
        }
    }
    let kind = match &spec.body {
        Body::Raw { kind, .. } => *kind,
        _ => RawKind::Xml,
    };
    spec.body = Body::Raw {
        kind: if kind == RawKind::Json {
            RawKind::Xml
        } else {
            kind
        },
        data: text,
    };
    spec.method = "POST".into();
    set_header(
        spec,
        "Content-Type",
        Some(settings.version.content_type(&settings.action)),
    );
    set_header(
        spec,
        "SOAPAction",
        settings.version.action_header(&settings.action),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(body: &str) -> RequestSpec {
        RequestSpec {
            body: Body::Raw {
                kind: RawKind::Xml,
                data: body.into(),
            },
            ..RequestSpec::default()
        }
    }

    const ENV: &str = "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\">\n  <soapenv:Header/>\n  <soapenv:Body>\n    <Ping/>\n  </soapenv:Body>\n</soapenv:Envelope>";

    #[test]
    fn switches_version_and_adds_and_removes_security() {
        let mut s = spec(ENV);
        let settings = Settings {
            version: SoapVersion::V12,
            action: "urn:ping".into(),
            security: Some(Security {
                timestamp_ttl: Some(60),
                ..Security::default()
            }),
        };
        apply(&mut s, &settings, 0).unwrap();
        let text = body_text(&s).unwrap();
        assert!(text.contains("http://www.w3.org/2003/05/soap-envelope"));
        assert!(
            text.contains("wsse:UsernameToken")
                && text.contains("<wsu:Created>1970-01-01T00:00:00Z")
        );
        assert!(envelope::parse(text).is_some(), "{text}");
        assert_eq!(read(&s).unwrap(), settings);
        assert_eq!(
            header_value(&s, "Content-Type"),
            Some("application/soap+xml; charset=utf-8; action=\"urn:ping\"")
        );
        assert_eq!(header_value(&s, "SOAPAction"), None);
        // Back to 1.1 without security.
        let back = Settings {
            version: SoapVersion::V11,
            action: "urn:ping".into(),
            security: None,
        };
        apply(&mut s, &back, 0).unwrap();
        let text = body_text(&s).unwrap();
        assert!(!text.contains("Security"), "{text}");
        assert_eq!(header_value(&s, "SOAPAction"), Some("\"urn:ping\""));
        assert_eq!(read(&s).unwrap(), back);
    }

    #[test]
    fn a_signature_is_never_rewritten() {
        let text = ENV.replace(
            "<soapenv:Header/>",
            "<soapenv:Header><wsse:Security xmlns:wsse=\"urn:s\"><ds:Signature xmlns:ds=\"urn:d\"/></wsse:Security></soapenv:Header>",
        );
        let mut s = spec(&text);
        let settings = Settings {
            version: SoapVersion::V11,
            action: String::new(),
            security: Some(Security::default()),
        };
        assert!(
            apply(&mut s, &settings, 0)
                .unwrap_err()
                .contains("signature")
        );
        assert_eq!(body_text(&s), Some(text.as_str()));
    }

    #[test]
    fn wraps_a_plain_body() {
        let mut s = spec("<Ping/>");
        apply(
            &mut s,
            &Settings {
                version: SoapVersion::V11,
                action: String::new(),
                security: None,
            },
            0,
        )
        .unwrap();
        assert!(body_text(&s).unwrap().contains("    <Ping/>"));
        assert!(envelope::parse(body_text(&s).unwrap()).is_some());
    }
}
