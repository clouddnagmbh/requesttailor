// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! For tests: the server on its own runtime ([`TestServer`]), a tiny blocking HTTP/1.1
//! client over `TcpStream` ([`send`], one connection per request, `Connection: close`) and a
//! cookie jar. No dependencies beyond std, so tests of the plugin (which run its hooks on the
//! test thread) can talk to the server synchronously.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

/// A running server with its own tokio runtime; stops when dropped.
pub struct TestServer {
    pub addr: SocketAddr,
    _runtime: tokio::runtime::Runtime,
}

impl TestServer {
    /// Starts a server with fresh data on a random port ([`crate::spawn_local`]).
    pub fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let addr = runtime.block_on(crate::spawn_local());
        Self {
            addr,
            _runtime: runtime,
        }
    }

    /// `http://127.0.0.1:<port><path>`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        crate::odata::header(&self.headers, name)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.text()))
    }
}

/// Sends one request to an `http://` URL and reads the whole response.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<Response> {
    let invalid = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, m.to_owned());
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| invalid("only http:// URLs"))?;
    let (authority, target) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, "/".to_owned()),
    };
    // Spaces (as in `$filter=a eq 1`) are not allowed on the request line.
    let target = target.replace(' ', "%20");
    let target = if target.starts_with('?') {
        format!("/{target}")
    } else {
        target
    };
    let mut stream = TcpStream::connect(authority)?;
    let mut head =
        format!("{method} {target} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
    for (n, v) in headers {
        if n.eq_ignore_ascii_case("host")
            || n.eq_ignore_ascii_case("connection")
            || n.eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw).ok_or_else(|| invalid("malformed HTTP response"))
}

fn parse_response(raw: &[u8]) -> Option<Response> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            Some((n.trim().to_owned(), v.trim().to_owned()))
        })
        .collect();
    let mut body = raw[end + 4..].to_vec();
    let find = |name: &str| crate::odata::header(&headers, name);
    if find("Transfer-Encoding").is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
        body = dechunk(&body)?;
    } else if let Some(len) = find("Content-Length").and_then(|l| l.parse::<usize>().ok()) {
        body.truncate(len);
    }
    Some(Response {
        status,
        headers,
        body,
    })
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(data.get(..size)?);
        data = data.get(size + 2..)?;
    }
}

/// Cookies by name, as a browser keeps them for one host (paths and expiry ignored).
#[derive(Debug, Clone, Default)]
pub struct CookieJar(BTreeMap<String, String>);

impl CookieJar {
    /// The `Cookie` header value, if there are cookies.
    pub fn header(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| {
            self.0
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    /// Stores the `Set-Cookie` headers of a response.
    pub fn store(&mut self, headers: &[(String, String)]) {
        for (n, v) in headers {
            if n.eq_ignore_ascii_case("set-cookie")
                && let Some((name, value)) = v.split(';').next().and_then(|c| c.split_once('='))
            {
                self.0
                    .insert(name.trim().to_owned(), value.trim().to_owned());
            }
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// Standard base64 (for `Authorization: Basic`).
pub fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `Authorization` header value for the S/4-style services.
pub fn s4_basic_auth() -> String {
    format!(
        "Basic {}",
        base64(&format!("{}:{}", crate::S4_USER, crate::S4_PASS))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        assert_eq!(base64(""), "");
        assert_eq!(base64("f"), "Zg==");
        assert_eq!(base64("fo"), "Zm8=");
        assert_eq!(base64("foo"), "Zm9v");
        assert_eq!(base64("RT_USER:RT_PASS"), "UlRfVVNFUjpSVF9QQVNT");
    }

    #[test]
    fn responses_and_chunks() {
        let r = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, b"abc");
        let mut jar = CookieJar::default();
        jar.store(&[("Set-Cookie".into(), "A=1; path=/".into())]);
        assert_eq!(jar.header().as_deref(), Some("A=1"));
    }
}
