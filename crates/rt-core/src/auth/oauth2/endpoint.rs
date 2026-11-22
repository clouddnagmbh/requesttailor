// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Requests to the token endpoint (RFC 6749 §4, §6) and the device authorization endpoint
//! (RFC 8628 §3.1), with client authentication, extra parameters and response parsing.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value};

use super::{Clock, Resolved};
use crate::model::{CredentialsPlacement, OAuth2Step, ParamPlacement};
use crate::services::{DeviceCodePrompt, StoredToken};
use crate::transport::{HttpBody, HttpRequest, SendOptions, Transport, collect};

/// Token responses are small; anything larger is not a token response.
const RESPONSE_LIMIT: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// RFC 3986 unreserved characters stay, everything else is percent-encoded.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(super) fn percent(s: &str) -> String {
    utf8_percent_encode(s, COMPONENT).to_string()
}

/// `url` with `name=value` appended to its query (before a fragment).
pub(super) fn with_query(url: &str, name: &str, value: &str) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let sep = match base.find('?') {
        None => "?",
        Some(_) if base.ends_with('?') || base.ends_with('&') => "",
        Some(_) => "&",
    };
    let mut out = format!("{base}{sep}{}={}", percent(name), percent(value));
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

/// An `application/x-www-form-urlencoded` body.
#[derive(Debug, Default)]
pub(super) struct Form(Vec<(String, String)>);

impl Form {
    pub fn add(&mut self, name: &str, value: &str) {
        self.0.push((name.to_owned(), value.to_owned()));
    }

    pub fn add_nonempty(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.add(name, value);
        }
    }

    fn encode(&self) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(self.0.iter())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EndpointError {
    /// An OAuth error response (RFC 6749 §5.2).
    OAuth {
        code: String,
        description: String,
        status: u16,
    },
    Other(String),
}

impl std::fmt::Display for EndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointError::OAuth {
                code,
                description,
                status,
            } if description.is_empty() => write!(f, "{code} (HTTP {status})"),
            EndpointError::OAuth {
                code,
                description,
                status,
            } => write!(f, "{code}: {description} (HTTP {status})"),
            EndpointError::Other(m) => f.write_str(m),
        }
    }
}

/// POSTs `form` to `url` with the client's authentication and the extra parameters of
/// `step`; returns the fields of the (JSON or form-encoded) response.
async fn post<T: Transport>(
    r: &Resolved,
    url: &str,
    step: OAuth2Step,
    mut form: Form,
    transport: &T,
) -> Result<Map<String, Value>, EndpointError> {
    let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
    // RFC 6749 §2.3.1: HTTP Basic with the form-encoded id and secret, or both in the body.
    // A client without a secret (public client) identifies itself with `client_id` only.
    if r.placement == CredentialsPlacement::BasicAuthHeader && !r.client_secret.is_empty() {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let pair = format!("{}:{}", enc(&r.client_id), enc(&r.client_secret));
        headers.push((
            "Authorization".to_owned(),
            format!("Basic {}", STANDARD.encode(pair)),
        ));
    } else {
        form.add_nonempty("client_id", &r.client_id);
        if r.placement == CredentialsPlacement::Body {
            form.add_nonempty("client_secret", &r.client_secret);
        }
    }
    let mut url = url.to_owned();
    for p in r.params(step) {
        match p.placement {
            ParamPlacement::Header => headers.push((p.name.clone(), p.value.clone())),
            ParamPlacement::Query => url = with_query(&url, &p.name, &p.value),
            ParamPlacement::Body => form.add(&p.name, &p.value),
        }
    }
    let request = HttpRequest {
        method: "POST".to_owned(),
        url,
        headers,
        body: Some(HttpBody {
            content_type: Some("application/x-www-form-urlencoded".to_owned()),
            bytes: Bytes::from(form.encode()),
        }),
        options: SendOptions {
            timeout: Some(TIMEOUT),
            ..SendOptions::default()
        },
    };
    let started = web_time::Instant::now();
    let response = transport
        .send(request)
        .await
        .map_err(|e| EndpointError::Other(e.to_string()))?;
    let completed = collect(response, RESPONSE_LIMIT, started)
        .await
        .map_err(|e| EndpointError::Other(e.to_string()))?;
    let status = completed.head.status;
    let fields = parse(&completed.body);
    if let Some(fields) = &fields
        && let Some(code) = fields.get("error").and_then(Value::as_str)
    {
        return Err(EndpointError::OAuth {
            code: code.to_owned(),
            description: text(fields, "error_description").unwrap_or_default(),
            status,
        });
    }
    if !(200..300).contains(&status) {
        let body = String::from_utf8_lossy(&completed.body);
        let snippet: String = body.trim().chars().take(200).collect();
        return Err(EndpointError::Other(if snippet.is_empty() {
            format!("the endpoint answered HTTP {status}")
        } else {
            format!("the endpoint answered HTTP {status}: {snippet}")
        }));
    }
    fields.ok_or_else(|| {
        EndpointError::Other(format!(
            "the endpoint answered HTTP {status} without a JSON object"
        ))
    })
}

/// A JSON object, or form fields (some providers answer `a=b&c=d` without `Accept`).
fn parse(body: &[u8]) -> Option<Map<String, Value>> {
    if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(body) {
        return Some(map);
    }
    let text = std::str::from_utf8(body).ok()?.trim();
    if text.is_empty() || text.starts_with('<') || !text.contains('=') || text.contains(' ') {
        return None;
    }
    Some(
        url::form_urlencoded::parse(text.as_bytes())
            .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
            .collect(),
    )
}

fn text(fields: &Map<String, Value>, name: &str) -> Option<String> {
    fields
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A number, or a number in a string (seen in the wild).
fn number(fields: &Map<String, Value>, name: &str) -> Option<u64> {
    match fields.get(name)? {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A token request; the response becomes a [`StoredToken`] obtained now.
pub(super) async fn token<T: Transport>(
    r: &Resolved,
    url: &str,
    step: OAuth2Step,
    form: Form,
    transport: &T,
    clock: &dyn Clock,
) -> Result<StoredToken, EndpointError> {
    let fields = post(r, url, step, form, transport).await?;
    let access_token = text(&fields, "access_token")
        .ok_or_else(|| EndpointError::Other("the token response has no access_token".to_owned()))?;
    let now = clock.now_ms();
    Ok(StoredToken {
        access_token,
        refresh_token: text(&fields, "refresh_token"),
        id_token: text(&fields, "id_token"),
        token_type: text(&fields, "token_type"),
        scope: text(&fields, "scope"),
        expires_at: number(&fields, "expires_in").map(|s| now + (s as i64).saturating_mul(1000)),
        obtained_at: now,
    })
}

pub(super) struct DeviceAuthorization {
    pub device_code: String,
    pub prompt: DeviceCodePrompt,
    /// Seconds between polls.
    pub interval: u64,
}

/// RFC 8628 §3.1 and §3.2. The extra parameters of the authorization step apply.
pub(super) async fn device_authorization<T: Transport>(
    r: &Resolved,
    url: &str,
    form: Form,
    transport: &T,
) -> Result<DeviceAuthorization, EndpointError> {
    let fields = post(r, url, OAuth2Step::Authorization, form, transport).await?;
    let missing = |name: &str| EndpointError::Other(format!("the response has no {name}"));
    Ok(DeviceAuthorization {
        device_code: text(&fields, "device_code").ok_or_else(|| missing("device_code"))?,
        prompt: DeviceCodePrompt {
            user_code: text(&fields, "user_code").ok_or_else(|| missing("user_code"))?,
            verification_uri: text(&fields, "verification_uri")
                .or_else(|| text(&fields, "verification_url"))
                .ok_or_else(|| missing("verification_uri"))?,
            verification_uri_complete: text(&fields, "verification_uri_complete"),
            expires_in: number(&fields, "expires_in").unwrap_or(600),
        },
        interval: number(&fields, "interval").unwrap_or(5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_appending() {
        assert_eq!(with_query("https://a/b", "k", "v w"), "https://a/b?k=v%20w");
        assert_eq!(
            with_query("https://a/b?x=1", "k", "v"),
            "https://a/b?x=1&k=v"
        );
        assert_eq!(with_query("https://a/b?", "k", "v"), "https://a/b?k=v");
        assert_eq!(with_query("https://a/b#f", "k", "v"), "https://a/b?k=v#f");
    }

    #[test]
    fn responses_as_json_or_form() {
        let json = parse(br#"{"access_token":"a","expires_in":60}"#).unwrap();
        assert_eq!(text(&json, "access_token").as_deref(), Some("a"));
        assert_eq!(number(&json, "expires_in"), Some(60));
        let form = parse(b"access_token=a%2Bb&expires_in=3600&scope=x+y").unwrap();
        assert_eq!(text(&form, "access_token").as_deref(), Some("a+b"));
        assert_eq!(number(&form, "expires_in"), Some(3600));
        assert_eq!(text(&form, "scope").as_deref(), Some("x y"));
        assert!(parse(b"<html>error</html>").is_none());
        assert!(parse(b"Bad gateway").is_none());
        assert!(parse(b"").is_none());
    }
}
