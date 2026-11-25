// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One egress call, end to end ([`request`]). Every shell calls this one function with its own
//! transport, host list, guard and secret values; nothing else sends on behalf of an extension.
//!
//! The order is fixed and every step only ever refuses:
//!
//! 1. the request itself: `GET` or `POST`, an `http(s)` URL without credentials, a body up to
//!    [`MAX_BODY`], headers without line breaks ([`EgressError::Limit`]);
//! 2. the send guard is asked with `Purpose::Egress` ([`EgressError::Blocked`]), before anyone is
//!    asked about the host;
//! 3. the host must be on the device's list ([`EgressError::NotAllowed`]; the UI asks the user
//!    once and calls `allow`);
//! 4. `{{secret:<name>}}` in a header value is resolved from the values the shell found (never
//!    sent over plain `http` to a host other than loopback);
//! 5. the transport sends without following redirects and with [`TIMEOUT_MS`];
//! 6. the answer is cut at [`MAX_RESPONSE`] and masked with every secret value of the call.
//!
//! Nothing here touches the history, the response store or an exchange observer: an egress
//! call leaves no trace but the guard's own log.

use std::time::Duration;

use bytes::Bytes;
use rt_core::guard::{Caller, Effect, GuardVerdict, Purpose, SendCheck, SendGuard};
use rt_core::model::Environment;
use rt_core::transport::{
    HttpBody, HttpRequest, SendOptions as WireOptions, Transport, TransportError, collect,
};
use url::Url;

use super::hosts::HostList;
use super::{
    EgressError, EgressMethod, EgressRequest, EgressResponse, MAX_BODY, MAX_RESPONSE, TIMEOUT_MS,
};
use crate::AppError;

/// Extra bytes read beyond [`MAX_RESPONSE`], so a secret value cut by the limit is still
/// masked before the answer is cut.
const SLACK: usize = 4096;

/// What a call needs besides the request.
pub struct Context<'a> {
    pub hosts: &'a HostList,
    /// The distribution's send guard (none in the builds of this repository).
    pub guard: Option<&'a dyn SendGuard>,
    pub caller: Caller,
    /// Name of the collection the call speaks for (`""` for none), for the guard.
    pub collection: &'a str,
    pub environment: Option<&'a str>,
    /// `(name, value)` for the `{{secret:<name>}}` references of the request, as
    /// [`lookup_secrets`] found them.
    pub secrets: &'a [(String, String)],
}

/// The names the headers of `request` refer to as `{{secret:<name>}}`, each once.
pub fn secret_names(request: &EgressRequest) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, value) in &request.headers {
        for name in references(value) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Every `{{secret:<name>}}` in `text`, in order.
fn references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{secret:") {
        let after = &rest[start + "{{secret:".len()..];
        let Some(end) = after.find("}}") else { break };
        out.push(after[..end].trim().to_owned());
        rest = &after[end + 2..];
    }
    out
}

/// The values of the secret variables `names` of `collection`'s environments. With
/// `environment` only that environment is looked at; without, the first environment (in file
/// order) that has a value wins. A name without a value is left out (the call then refuses).
/// `get(environment, variable)` reads the shell's store.
pub fn lookup_secrets(
    names: &[String],
    environments: &[Environment],
    environment: Option<&str>,
    mut get: impl FnMut(&str, &str) -> Result<Option<String>, AppError>,
) -> Result<Vec<(String, String)>, AppError> {
    let mut found = Vec::new();
    for name in names {
        'envs: for env in environments {
            if environment.is_some_and(|e| e != env.name) {
                continue;
            }
            if !env
                .variables
                .iter()
                .any(|v| &v.name == name && v.secret && v.enabled)
            {
                continue;
            }
            if let Some(value) = get(&env.name, name)? {
                found.push((name.clone(), value));
                break 'envs;
            }
        }
    }
    Ok(found)
}

fn limit(message: impl Into<String>) -> EgressError {
    EgressError::Limit {
        message: message.into(),
    }
}

/// Replaces `{{secret:<name>}}` in `value`; `Err` names the first reference without a value.
fn resolve(value: &str, secrets: &[(String, String)]) -> Result<String, String> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("{{secret:") {
        let after = &rest[start + "{{secret:".len()..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        let Some((_, secret)) = secrets.iter().find(|(n, _)| n == name) else {
            return Err(name.to_owned());
        };
        out.push_str(&rest[..start]);
        out.push_str(secret);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn masked(text: &str, secrets: &[String]) -> String {
    crate::workspace::mask(text, secrets)
}

fn transport_error(e: TransportError, secrets: &[String]) -> EgressError {
    EgressError::Transport {
        message: masked(&e.to_string(), secrets),
    }
}

/// Makes the call. `transport` is the shell's own (native stack, `fetch`, companion).
pub async fn request<T: Transport>(
    transport: &T,
    ctx: &Context<'_>,
    request: &EgressRequest,
) -> Result<EgressResponse, EgressError> {
    // 1. The request itself.
    if request.body.as_ref().is_some_and(|b| b.len() > MAX_BODY) {
        return Err(limit(format!(
            "the body is over the limit of {} KiB",
            MAX_BODY / 1024
        )));
    }
    if request.method == EgressMethod::Get && request.body.is_some() {
        return Err(limit("a GET request has no body"));
    }
    let url = Url::parse(&request.url).map_err(|_| limit("the URL is not valid"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(limit("only http and https URLs are allowed"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(limit(
            "credentials do not belong in the URL; use a header with {{secret:<name>}}",
        ));
    }
    let Some(host) = url.host_str().map(str::to_owned) else {
        return Err(limit("the URL has no host"));
    };
    for (name, value) in &request.headers {
        let bad = |s: &str| s.chars().any(|c| matches!(c, '\r' | '\n' | '\0'));
        if name.trim().is_empty() || name.contains(':') || bad(name) || bad(value) {
            return Err(limit("a header is not valid"));
        }
    }

    let secrets: Vec<String> = ctx
        .secrets
        .iter()
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
        .collect();

    // 2. The guard, for every call and before the per-host question (a refusing guard means
    // the user is never asked).
    if let Some(guard) = ctx.guard {
        let check = SendCheck {
            caller: ctx.caller,
            purpose: Purpose::Egress,
            collection: ctx.collection.to_owned(),
            // The extension that asks (there is no request of a collection).
            request_path: request.extension.clone(),
            environment: ctx.environment.map(str::to_owned),
            method: match request.method {
                EgressMethod::Get => "GET".into(),
                EgressMethod::Post => "POST".into(),
            },
            url: masked(&request.url, &secrets),
            effect: match request.method {
                EgressMethod::Get => Effect::Read,
                EgressMethod::Post => Effect::Write,
            },
            production: false,
        };
        if let GuardVerdict::Refuse { rule, reason } = guard.check(&check) {
            return Err(EgressError::Blocked { rule, reason });
        }
    }

    // 3. The host is allowed on this device.
    if !ctx.hosts.allows(&host, url.port(), url.scheme()) {
        return Err(EgressError::NotAllowed {
            host,
            port: url.port(),
        });
    }

    // 4. Secrets in headers.
    let mut headers = Vec::with_capacity(request.headers.len());
    let mut uses_secret = false;
    for (name, value) in &request.headers {
        uses_secret |= value.contains("{{secret:");
        match resolve(value, ctx.secrets) {
            Ok(resolved) => headers.push((name.clone(), resolved)),
            Err(missing) => {
                return Err(limit(format!(
                    "the secret \"{missing}\" has no value on this device"
                )));
            }
        }
    }
    if uses_secret && url.scheme() == "http" && !is_loopback(&host) {
        return Err(limit(
            "a secret is not sent over plain http to a host other than this device",
        ));
    }
    // 5. The wire.
    let wire = HttpRequest {
        method: match request.method {
            EgressMethod::Get => "GET".into(),
            EgressMethod::Post => "POST".into(),
        },
        url: url.to_string(),
        headers,
        body: request.body.as_ref().map(|b| HttpBody {
            content_type: None,
            bytes: Bytes::from(b.clone().into_bytes()),
        }),
        options: WireOptions {
            timeout: Some(Duration::from_millis(TIMEOUT_MS)),
            follow_redirects: false,
            max_redirects: 0,
            client_identity: None,
        },
    };
    let started = web_time::Instant::now();
    let response = transport
        .send(wire)
        .await
        .map_err(|e| transport_error(e, &secrets))?;
    let done = collect(response, MAX_RESPONSE + SLACK, started)
        .await
        .map_err(|e| transport_error(e, &secrets))?;

    // 6. The answer, cut and masked.
    let text = String::from_utf8_lossy(&done.body);
    let mut body = masked(&text, &secrets);
    let mut truncated = done.truncated;
    if body.len() > MAX_RESPONSE {
        let mut cut = MAX_RESPONSE;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        body.truncate(cut);
        truncated = true;
    }
    let headers = done
        .head
        .headers
        .iter()
        .map(|(n, v)| (n.clone(), masked(v, &secrets)))
        .collect();
    Ok(EgressResponse {
        status: done.head.status,
        headers,
        body,
        truncated,
        took_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}
