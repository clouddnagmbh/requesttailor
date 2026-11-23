// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Streamable HTTP (MCP 2025-03-26 and later) over the app's own [`Transport`]: one `POST`
//! per message, the answer as JSON or as an SSE stream, read until the response with the
//! request's id (the stream is not read to its end, servers may keep it open).

use std::time::Duration;

use bytes::Bytes;
use rt_core::transport::{BodyStream, HttpBody, HttpRequest, SendOptions, Transport};
use serde_json::Value;

use super::{McpError, keep_notification};

/// Answers larger than this are refused (an SSE stream that never sends our response).
const MAX_ANSWER: usize = 64 * 1024 * 1024;

pub(super) struct HttpWire {
    url: String,
    headers: Vec<(String, String)>,
    session: Option<String>,
    /// The negotiated version, sent as `MCP-Protocol-Version` after `initialize`.
    pub(super) protocol: Option<String>,
}

impl HttpWire {
    pub(super) fn new(url: String, headers: Vec<(String, String)>) -> Self {
        Self {
            url,
            headers,
            session: None,
            protocol: None,
        }
    }

    fn request(&self, method: &str, body: Option<Bytes>, timeout: Duration) -> HttpRequest {
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .filter(|(n, _)| !n.trim().is_empty())
            .cloned()
            .collect();
        let mut set = |name: &str, value: &str| {
            if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                headers.push((name.to_owned(), value.to_owned()));
            }
        };
        set("Accept", "application/json, text/event-stream");
        if let Some(s) = &self.session {
            set("Mcp-Session-Id", s);
        }
        if let Some(p) = &self.protocol {
            set("MCP-Protocol-Version", p);
        }
        HttpRequest {
            method: method.to_owned(),
            url: self.url.clone(),
            headers,
            body: body.map(|bytes| HttpBody {
                content_type: Some("application/json".into()),
                bytes,
            }),
            options: SendOptions {
                timeout: Some(timeout),
                ..SendOptions::default()
            },
        }
    }

    /// Sends one message. `id`: the answer to wait for (`None` for notifications).
    /// Notifications that come before the answer (in an event stream) go to `notifications`.
    pub(super) async fn exchange<T: Transport>(
        &mut self,
        transport: &T,
        message: &Value,
        id: Option<u64>,
        timeout: Duration,
        notifications: &mut Vec<Value>,
    ) -> Result<Option<Value>, McpError> {
        let body = Bytes::from(serde_json::to_vec(message).expect("JSON serializes"));
        let request = self.request("POST", Some(body), timeout);
        let response = transport.send(request).await.map_err(|e| match e {
            rt_core::transport::TransportError::Timeout => McpError::Timeout,
            other => McpError::Connect(other.to_string()),
        })?;
        let head = response.head;
        let mut body = response.body;
        let header = |name: &str| {
            head.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_owned())
        };
        if !(200..300).contains(&head.status) {
            let text = read_all(&mut body).await.unwrap_or_default();
            let text = String::from_utf8_lossy(&text);
            if head.status == 404 && self.session.is_some() {
                return Err(McpError::SessionExpired);
            }
            // A JSON-RPC error in the body says more than the status.
            if let Ok(v) = serde_json::from_str::<Value>(&text)
                && let Some(e) = v.get("error")
            {
                return Err(McpError::Rpc {
                    code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: e
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                });
            }
            return Err(McpError::Http {
                status: head.status,
                body: text.chars().take(500).collect(),
            });
        }
        if let Some(s) = header("mcp-session-id").filter(|s| !s.is_empty()) {
            self.session = Some(s);
        }
        let Some(id) = id else {
            return Ok(None);
        };
        let content_type = header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            return read_sse(&mut body, id, notifications).await.map(Some);
        }
        let bytes = read_all(&mut body).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| McpError::Protocol(format!("not JSON ({e})")))?;
        find_answer(value, id).map(Some).ok_or_else(|| {
            McpError::Protocol("the answer is not the response to the request".into())
        })
    }

    /// `DELETE` with the session id; errors (405: the server keeps sessions) are ignored.
    pub(super) async fn close<T: Transport>(self, transport: &T, timeout: Duration) {
        if self.session.is_none() {
            return;
        }
        let request = self.request("DELETE", None, timeout.min(Duration::from_secs(5)));
        if let Ok(response) = transport.send(request).await {
            let mut body = response.body;
            let _ = read_all(&mut body).await;
        }
    }
}

async fn next_chunk(body: &mut BodyStream) -> Option<Result<Bytes, McpError>> {
    std::future::poll_fn(|cx| futures_core::Stream::poll_next(body.as_mut(), cx))
        .await
        .map(|r| r.map_err(|e| McpError::Connect(e.to_string())))
}

async fn read_all(body: &mut BodyStream) -> Result<Vec<u8>, McpError> {
    let mut out = Vec::new();
    while let Some(chunk) = next_chunk(body).await {
        out.extend_from_slice(&chunk?);
        if out.len() > MAX_ANSWER {
            return Err(McpError::Protocol("the answer is too large".into()));
        }
    }
    Ok(out)
}

/// The response with `id` in a JSON answer (a message or a batch).
fn find_answer(value: Value, id: u64) -> Option<Value> {
    let is_ours = |m: &Value| {
        m.get("id").and_then(Value::as_u64) == Some(id)
            && (m.get("result").is_some() || m.get("error").is_some())
    };
    match value {
        Value::Array(items) => items.into_iter().find(is_ours),
        m if is_ours(&m) => Some(m),
        _ => None,
    }
}

/// A server's notification (a message with a method and without an id).
fn is_notification(value: &Value) -> bool {
    value.get("method").is_some_and(Value::is_string) && value.get("id").is_none()
}

/// Reads SSE events until one carries the response with `id`; notifications on the way go to
/// `notifications`.
async fn read_sse(
    body: &mut BodyStream,
    id: u64,
    notifications: &mut Vec<Value>,
) -> Result<Value, McpError> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut parser = SseParser::default();
    loop {
        // Complete lines first; a chunk may end in the middle of one.
        while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(data) = parser.line(line.trim_end_matches(['\n', '\r']))
                && let Ok(value) = serde_json::from_str::<Value>(&data)
            {
                if is_notification(&value) {
                    keep_notification(notifications, value);
                } else if let Some(answer) = find_answer(value, id) {
                    return Ok(answer);
                }
            }
        }
        match next_chunk(body).await {
            Some(chunk) => {
                buffer.extend_from_slice(&chunk?);
                if buffer.len() > MAX_ANSWER {
                    return Err(McpError::Protocol("an SSE event is too large".into()));
                }
            }
            None => {
                // The stream ended: a last event without its blank line still counts.
                let rest = String::from_utf8_lossy(&buffer).into_owned();
                if !rest.is_empty() {
                    parser.line(&rest);
                }
                if let Some(data) = parser.line("")
                    && let Ok(value) = serde_json::from_str::<Value>(&data)
                    && let Some(answer) = find_answer(value, id)
                {
                    return Ok(answer);
                }
                return Err(McpError::Protocol(
                    "the event stream ended without the response".into(),
                ));
            }
        }
    }
}

/// Server-sent events: `data:` lines joined with `\n`, an event ends at a blank line.
#[derive(Default)]
struct SseParser {
    data: Vec<String>,
}

impl SseParser {
    /// Feeds one line (without its line break); returns the data of a finished event.
    fn line(&mut self, line: &str) -> Option<String> {
        if line.is_empty() {
            if self.data.is_empty() {
                return None;
            }
            return Some(std::mem::take(&mut self.data).join("\n"));
        }
        if let Some(rest) = line.strip_prefix("data") {
            match rest.strip_prefix(':') {
                Some(v) => self.data.push(v.strip_prefix(' ').unwrap_or(v).to_owned()),
                None if rest.is_empty() => self.data.push(String::new()),
                None => {}
            }
        }
        // `event:`, `id:`, `retry:` and comments (`:`) do not matter here.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_events() {
        let mut p = SseParser::default();
        assert_eq!(p.line(": comment"), None);
        assert_eq!(p.line("event: message"), None);
        assert_eq!(p.line("data: {\"a\":"), None);
        assert_eq!(p.line("data:1}"), None);
        assert_eq!(p.line(""), Some("{\"a\":\n1}".into()));
        assert_eq!(p.line(""), None);
    }

    #[test]
    fn answers_are_found_by_id() {
        let v = serde_json::json!([{"jsonrpc": "2.0", "method": "x"}, {"id": 2, "result": {}}]);
        assert!(find_answer(v.clone(), 2).is_some());
        assert!(find_answer(v, 3).is_none());
        assert!(find_answer(serde_json::json!({"id": 1, "method": "ping"}), 1).is_none());
    }
}
