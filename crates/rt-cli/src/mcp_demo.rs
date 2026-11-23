// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp-demo [--http] [--port N]`: the MCP demo server (`rt_app::mcp_demo`), offline and
//! with fake data, to try MCP clients against: stdio by default, Streamable HTTP on
//! `127.0.0.1` with `--http`. The app has the same server built in (`rt-demo://mcp`).

use std::io::Write;

use rt_app::mcp_demo::serve::{LocalServer, serve_stdio};

use crate::{EXIT_OK, EXIT_TRANSPORT};

/// Default port of `--http`.
pub const DEFAULT_PORT: u16 = 47850;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Streamable HTTP instead of stdio.
    pub http: bool,
    pub port: u16,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            http: false,
            port: DEFAULT_PORT,
        }
    }
}

pub const USAGE: &str = "  rt mcp-demo [--http] [--port <PORT>]\n";

pub fn help() -> String {
    format!(
        "\
rt mcp-demo runs the MCP demo server: fake data, offline, no account. It is the
server behind \"Try the demo server\" in the app, to try any MCP client with.
  Tools: echo, add, fake_weather, todo_add, todo_list, todo_done, uuid, image,
  fail, countdown. Resources: demo://readme, demo://cities.json, demo://todos.
  Prompts: summarize, weather_report. Lists are paginated.
  Without options it speaks MCP on stdin and stdout, for clients that start a
  command (\"command\": \"rt\", \"args\": [\"mcp-demo\"]).
      --http                Streamable HTTP on http://127.0.0.1:<PORT>/mcp until
                            stopped (Ctrl-C); loopback only
      --port <PORT>         Port for --http (default {DEFAULT_PORT}; 0 picks a free one)
  Saved MCP tests can also use the built-in address rt-demo://mcp, which needs
  no server at all (also in rt mcp-test).
"
    )
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("http") => args.http = true,
            Long("port") => {
                let v = parser.value().map_err(err)?.string().map_err(err)?;
                args.port = v
                    .parse()
                    .map_err(|_| format!("--port expects a port number, got {v:?}"))?;
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

pub fn run(args: Args, stderr: &mut dyn Write) -> i32 {
    if !args.http {
        let _ = writeln!(stderr, "rt: MCP demo server on stdio (end with Ctrl-D)");
        let stdin = std::io::stdin();
        return match serve_stdio(stdin.lock(), std::io::stdout()) {
            Ok(()) => EXIT_OK,
            Err(e) => {
                let _ = writeln!(stderr, "rt: {e}");
                EXIT_TRANSPORT
            }
        };
    }
    match LocalServer::start(args.port) {
        Ok(server) => {
            let _ = writeln!(
                stderr,
                "rt: MCP demo server on {} (Streamable HTTP, Ctrl-C to stop)",
                server.url()
            );
            server.wait();
            EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(stderr, "rt: cannot listen on 127.0.0.1:{}: {e}", args.port);
            EXIT_TRANSPORT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<Args>, String> {
        parse(&mut lexopt::Parser::from_args(args.iter().copied()))
    }

    #[test]
    fn arguments() {
        assert_eq!(parse_args(&[]).unwrap(), Some(Args::default()));
        assert_eq!(
            parse_args(&["--http", "--port", "48400"]).unwrap(),
            Some(Args {
                http: true,
                port: 48400
            })
        );
        assert_eq!(parse_args(&["--help"]).unwrap(), None);
        assert!(parse_args(&["--port", "x"]).is_err());
        assert!(parse_args(&["extra"]).is_err());
    }
}
