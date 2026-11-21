// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt agent`: the companion service. The web app hands it requests the browser may not send
//! (CORS, internal hosts) and it sends them with the native transport.
//!
//! It is a local HTTP server that sends arbitrary requests, so it is built as a security
//! boundary; the rules are documented in `crates/rt-cli/README.md` ("Companion") and each one
//! is tested in `tests/agent.rs`. Wire types: `rt_core::transport::companion`.

mod pairing;
mod server;

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};

pub use pairing::{CODE_TTL, MAX_FAILURES};

/// Port of the companion unless `--port` says otherwise. The web app looks here.
pub const DEFAULT_PORT: u16 = 47812;
/// Origins allowed when no `--allow-origin` is given: the web app's dev server.
pub const DEFAULT_ORIGINS: [&str; 2] = ["http://127.0.0.1:5173", "http://localhost:5173"];
/// Largest accepted `/v1/send` request (JSON, body as base64).
pub const MAX_SEND_REQUEST: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentArgs {
    pub port: u16,
    /// Normalized (`scheme://host[:port]`), as browsers send them in `Origin`.
    pub origins: Vec<String>,
}

/// Parses the arguments after `agent`. `Ok(None)` means `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<AgentArgs>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut port = DEFAULT_PORT;
    let mut origins = Vec::new();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("port") => port = parser.value().map_err(err)?.parse().map_err(err)?,
            Long("allow-origin") => {
                let value = parser.value().map_err(err)?.string().map_err(err)?;
                let origin = normalize_origin(&value)?;
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    if origins.is_empty() {
        origins = DEFAULT_ORIGINS.iter().map(|o| (*o).to_owned()).collect();
    }
    Ok(Some(AgentArgs { port, origins }))
}

/// `https://App.Example:443/` → `https://app.example`: the form browsers send in `Origin`.
pub fn normalize_origin(value: &str) -> Result<String, String> {
    let bad = |why: &str| format!("--allow-origin {value:?}: {why}");
    let url = url::Url::parse(value).map_err(|e| bad(&e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(bad("must start with http:// or https://"));
    }
    if url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad("an origin is scheme://host[:port], without path"));
    }
    Ok(url.origin().ascii_serialization())
}

/// Runs the companion until the process is stopped. Returns an exit code on startup errors.
pub fn run(args: AgentArgs, stderr: &mut dyn Write) -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "rt: cannot start runtime: {e}");
            return crate::EXIT_TRANSPORT;
        }
    };
    let local = tokio::task::LocalSet::new();
    let result = local.block_on(&runtime, async {
        // Loopback only, never 0.0.0.0: nothing outside this machine can reach the companion.
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let agent = server::Agent::new(listener.local_addr()?.port(), args.origins)
            .map_err(std::io::Error::other)?;
        agent.print_banner();
        server::serve(listener, agent).await
    });
    match result {
        Ok(()) => crate::EXIT_OK,
        Err(e) => {
            let _ = writeln!(
                stderr,
                "rt: agent: cannot listen on port {}: {e}",
                args.port
            );
            crate::EXIT_USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<AgentArgs>, String> {
        parse(&mut lexopt::Parser::from_args(args.iter().copied()))
    }

    #[test]
    fn defaults() {
        let a = parse_args(&[]).unwrap().unwrap();
        assert_eq!(a.port, DEFAULT_PORT);
        assert_eq!(a.origins, DEFAULT_ORIGINS);
    }

    #[test]
    fn flags_replace_the_default_origins() {
        let a = parse_args(&[
            "--port",
            "1234",
            "--allow-origin",
            "https://App.Example:443/",
            "--allow-origin=http://localhost:5187",
            "--allow-origin",
            "https://app.example",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(a.port, 1234);
        assert_eq!(a.origins, ["https://app.example", "http://localhost:5187"]);
        assert_eq!(parse_args(&["--help"]), Ok(None));
    }

    #[test]
    fn rejects_bad_flags() {
        for bad in [
            &["--port", "x"][..],
            &["--port", "70000"],
            &["--allow-origin", "*"],
            &["--allow-origin", "null"],
            &["--allow-origin", "file:///tmp"],
            &["--allow-origin", "http://a.example/path"],
            &["--allow-origin", "http://a.example?x"],
            &["--allow-origin", "http://user@a.example"],
            &["--allow-origin"],
            &["extra"],
        ] {
            assert!(parse_args(bad).is_err(), "{bad:?}");
        }
    }
}
