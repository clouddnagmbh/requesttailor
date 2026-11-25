// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Finding the next page in a response and building the request for it. Nothing here sends.

use base64::Engine as _;
use rt_core::model::{Body, KeyValue, RawKind, RequestSpec};
use rt_core::paging::{CursorParam, CursorPlace, PagingRule};
use rt_core::send::{ReportBody, SendReport};
use serde_json::Value;
use serde_json_path::JsonPath;
use url::Url;

/// What the response says about the next page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Next {
    /// No further page.
    None,
    /// A URL (absolute, or relative to the response).
    Url(String),
    /// A cursor value to send back.
    Cursor(Value),
}

/// The response body as text (a binary body is read lossily).
pub(super) fn body_text(report: &SendReport) -> String {
    match report.response.as_ref().map(|r| &r.body) {
        Some(ReportBody::Utf8(text)) => text.clone(),
        Some(ReportBody::Base64(data)) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// The `rel="next"` target of the `Link` headers (RFC 8288), if there is one.
pub(super) fn link_next(headers: &[(String, String)]) -> Option<String> {
    for (name, value) in headers {
        if !name.eq_ignore_ascii_case("link") {
            continue;
        }
        for entry in split_links(value) {
            let entry = entry.trim();
            let Some(rest) = entry.strip_prefix('<') else {
                continue;
            };
            let Some((target, params)) = rest.split_once('>') else {
                continue;
            };
            let is_next = params.split(';').any(|param| {
                let Some((key, val)) = param.split_once('=') else {
                    return false;
                };
                key.trim().eq_ignore_ascii_case("rel")
                    && val
                        .trim()
                        .trim_matches('"')
                        .split_ascii_whitespace()
                        .any(|rel| rel.eq_ignore_ascii_case("next"))
            });
            if is_next {
                return Some(target.trim().to_owned());
            }
        }
    }
    None
}

/// The entries of a `Link` header value: split at commas outside `<…>` and quotes.
fn split_links(value: &str) -> Vec<&str> {
    let (mut out, mut start) = (Vec::new(), 0);
    let (mut in_angle, mut in_quote) = (false, false);
    for (i, c) in value.char_indices() {
        match c {
            '<' if !in_quote => in_angle = true,
            '>' if !in_quote => in_angle = false,
            '"' if !in_angle => in_quote = !in_quote,
            ',' if !in_angle && !in_quote => {
                out.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&value[start..]);
    out
}

fn first_at(body: &str, path: &str) -> Result<Option<Value>, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| "the page is not JSON".to_owned())?;
    let compiled = JsonPath::parse(path).map_err(|e| format!("{path} is not a JSONPath: {e}"))?;
    Ok(compiled.query(&value).first().cloned())
}

/// What `rule` finds in the response `report`. `Err` says why the rule does not apply to this
/// response (then the run ends there, with the reason as a note).
pub(super) fn next_of(rule: &PagingRule, report: &SendReport) -> Result<Next, String> {
    let Some(response) = &report.response else {
        return Ok(Next::None);
    };
    match rule {
        PagingRule::LinkHeader => Ok(link_next(&response.headers).map_or(Next::None, Next::Url)),
        PagingRule::JsonUrl { next } => match first_at(&body_text(report), next)? {
            None | Some(Value::Null) => Ok(Next::None),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(Next::None),
            Some(Value::String(s)) => Ok(Next::Url(s.trim().to_owned())),
            Some(_) => Err(format!("{next} is not a URL")),
        },
        PagingRule::Cursor { next, .. } => match first_at(&body_text(report), next)? {
            None | Some(Value::Null) => Ok(Next::None),
            Some(Value::String(s)) if s.is_empty() => Ok(Next::None),
            Some(v @ (Value::String(_) | Value::Number(_))) => Ok(Next::Cursor(v)),
            Some(_) => Err(format!("{next} is not a cursor")),
        },
    }
}

/// `scheme://host:port` of a URL, with the default port filled in.
pub(super) fn origin(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        url.port_or_known_default().unwrap_or(0)
    )
}

/// The absolute next URL: `target` resolved against `base`, with braces encoded (a template
/// in a URL a server sends is never resolved). `Err` for a URL that is not http(s).
pub(super) fn resolve(base: &str, target: &str) -> Result<Url, String> {
    let base = Url::parse(base).map_err(|_| "the page URL is not a URL".to_owned())?;
    let joined = base
        .join(target)
        .map_err(|_| "the next page is not a URL".to_owned())?;
    if !matches!(joined.scheme(), "http" | "https") {
        return Err(format!("the next page uses {}", joined.scheme()));
    }
    let encoded = joined.as_str().replace('{', "%7B").replace('}', "%7D");
    Url::parse(&encoded).map_err(|_| "the next page is not a URL".to_owned())
}

/// The text of a cursor value.
pub(super) fn cursor_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `url` with the query parameter `name` set to `value` (replaced, or appended). A fragment and
/// `{{templates}}` in the rest of the URL stay.
fn set_query(url: &str, name: &str, value: &str) -> String {
    let (rest, fragment) = match url.split_once('#') {
        Some((r, f)) => (r, Some(f)),
        None => (url, None),
    };
    let (base, query) = match rest.split_once('?') {
        Some((b, q)) => (b, q),
        None => (rest, ""),
    };
    let pair = format!("{}={}", encode_component(name), encode_component(value));
    let mut replaced = false;
    let mut pairs: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let key = p.split_once('=').map_or(p, |(k, _)| k);
            if !replaced && (key == name || key == encode_component(name)) {
                replaced = true;
                pair.clone()
            } else {
                p.to_owned()
            }
        })
        .collect();
    if !replaced {
        pairs.push(pair);
    }
    let mut out = format!("{base}?{}", pairs.join("&"));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn set_json_field(text: &str, name: &str, value: &Value) -> Result<String, String> {
    let mut root: Value = if text.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(text)
            .map_err(|_| "the body is not plain JSON, so the cursor cannot go in".to_owned())?
    };
    let mut cur = &mut root;
    let parts: Vec<&str> = name.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        let Some(object) = cur.as_object_mut() else {
            return Err(format!("the body has no object at {name}"));
        };
        if i + 1 == parts.len() {
            object.insert((*part).to_owned(), value.clone());
            break;
        }
        cur = object
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// `spec` for the page after `cursor`.
pub(super) fn with_cursor(
    spec: &RequestSpec,
    param: &CursorParam,
    cursor: &Value,
) -> Result<RequestSpec, String> {
    let mut spec = spec.clone();
    match param.place {
        CursorPlace::Query => {
            spec.url = set_query(&spec.url, &param.name, &cursor_text(cursor));
        }
        CursorPlace::Header => {
            let value = cursor_text(cursor);
            match spec
                .headers
                .iter_mut()
                .find(|h| h.name.eq_ignore_ascii_case(&param.name))
            {
                Some(h) => {
                    h.value = value;
                    h.enabled = true;
                }
                None => spec.headers.push(KeyValue::new(param.name.clone(), value)),
            }
        }
        CursorPlace::Body => match &mut spec.body {
            Body::Raw {
                kind: RawKind::Json,
                data,
            } => *data = set_json_field(data, &param.name, cursor)?,
            Body::GraphQl { variables, .. } => {
                *variables = set_json_field(variables, &param.name, cursor)?;
            }
            Body::FormUrlEncoded { fields } => {
                let value = cursor_text(cursor);
                match fields.iter_mut().find(|f| f.name == param.name) {
                    Some(f) => {
                        f.value = value;
                        f.enabled = true;
                    }
                    None => fields.push(KeyValue::new(param.name.clone(), value)),
                }
            }
            _ => return Err("the cursor can go in a JSON, GraphQL or form body only".to_owned()),
        },
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: &str) -> Vec<(String, String)> {
        vec![("Link".into(), v.into())]
    }

    #[test]
    fn link_headers() {
        assert_eq!(
            link_next(&h(
                r#"<https://a/x?p=2>; rel="next", <https://a/x?p=9>; rel="last""#
            )),
            Some("https://a/x?p=2".into())
        );
        assert_eq!(
            link_next(&h(
                r#"<https://a/p1>; rel="prev", <https://a/p3,x>; rel=next"#
            )),
            Some("https://a/p3,x".into())
        );
        assert_eq!(
            link_next(&h(r#"</rel>; title="a, b"; rel="prev next""#)),
            Some("/rel".into())
        );
        assert_eq!(link_next(&h(r#"<https://a/p1>; rel="prev""#)), None);
        assert_eq!(link_next(&[]), None);
        // Two header lines.
        let two = vec![
            ("link".to_owned(), r#"<https://a/1>; rel="prev""#.to_owned()),
            ("LINK".to_owned(), r#"<https://a/3>; rel="next""#.to_owned()),
        ];
        assert_eq!(link_next(&two), Some("https://a/3".into()));
    }

    #[test]
    fn urls_resolve_and_braces_are_encoded() {
        let u = resolve("https://a.test/list?page=1", "?page=2").unwrap();
        assert_eq!(u.as_str(), "https://a.test/list?page=2");
        let u = resolve("https://a.test/x/y", "/z").unwrap();
        assert_eq!(u.as_str(), "https://a.test/z");
        let u = resolve("https://a.test/", "https://b.test/{{secret}}").unwrap();
        assert!(!u.as_str().contains('{'), "{u}");
        assert_ne!(origin(&u), origin(&Url::parse("https://a.test").unwrap()));
        assert_eq!(
            origin(&Url::parse("https://a.test").unwrap()),
            origin(&Url::parse("https://a.test:443/x").unwrap())
        );
        assert!(resolve("https://a.test/", "ftp://a.test/x").is_err());
    }

    #[test]
    fn query_cursors_replace_or_append() {
        assert_eq!(
            set_query("https://a/x", "after", "c 2"),
            "https://a/x?after=c%202"
        );
        assert_eq!(
            set_query("https://a/x?limit=5&after=c1#top", "after", "c2"),
            "https://a/x?limit=5&after=c2#top"
        );
        assert_eq!(
            set_query("{{base}}/x?q={{term}}", "after", "c2"),
            "{{base}}/x?q={{term}}&after=c2"
        );
    }

    #[test]
    fn body_cursors() {
        let mut spec = RequestSpec {
            body: Body::Raw {
                kind: RawKind::Json,
                data: r#"{"q":"a"}"#.into(),
            },
            ..RequestSpec::default()
        };
        let param = CursorParam {
            place: CursorPlace::Body,
            name: "page.after".into(),
        };
        let out = with_cursor(&spec, &param, &Value::String("c2".into())).unwrap();
        let Body::Raw { data, .. } = out.body else {
            panic!()
        };
        let v: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(v["page"]["after"], "c2");
        assert_eq!(v["q"], "a");
        spec.body = Body::None;
        assert!(with_cursor(&spec, &param, &Value::Null).is_err());
    }
}
