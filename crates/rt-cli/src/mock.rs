// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mock [<collection>] [--port <PORT>] [--env NAME]`: a mock server answering with the
//! saved examples of the collection's HTTP requests (wave 8, strand F).
//!
//! - Routes come from the HTTP requests that have `examples` (`rt_format::examples`): the
//!   method and the path of the request's URL. Variables are resolved with the collection
//!   and, with `--env`, the environment (secrets have no value in the files and stay
//!   placeholders). A path segment `:name`, `{name}` or one with an unresolved `{{variable}}`
//!   matches any segment; the scheme, host and a leading `{{baseUrl}}` are not part of the
//!   path.
//! - When several examples fit, the one whose saved request matches best wins: literal path
//!   segments and path parameters of the example's URL, its query parameters, headers and
//!   body (JSON compared as JSON). Ties go to the first example in file order.
//! - No example fits: 404 with a JSON body listing the routes. CORS headers on every answer,
//!   preflights (`OPTIONS` with `Access-Control-Request-Method`) are answered with 204, like
//!   `rt-testserver`, so the web app can call the mock.
//! - Loopback (`127.0.0.1`) unless `--host` says otherwise.
//!
//! Matching: `mock/router.rs` (pure, unit-tested); the HTTP server: `mock/server.rs`.

mod router;
mod server;

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

pub use router::{Answer, Mock, MockRequest, RouteInfo};

/// Port unless `--port` says otherwise.
pub const DEFAULT_PORT: u16 = 4010;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// A folder of the collection (its root is the nearest folder with `opencollection.yml`
    /// at or above it). Default: the current folder.
    pub collection: PathBuf,
    pub port: u16,
    pub host: IpAddr,
    pub environment: Option<String>,
    /// `--help`: print [`help`] and exit.
    pub help: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            collection: PathBuf::from("."),
            port: DEFAULT_PORT,
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            environment: None,
            help: false,
        }
    }
}

pub fn help() -> String {
    format!(
        "\
rt mock: serve the saved examples of a collection's requests

Usage:
  rt mock [<collection>] [options]

Answers HTTP requests with the examples saved in the collection's request
files (`examples`: request and response). A request matches an example by
method and path: the path of the request's URL, where `:name`, `{{name}}` and
unresolved {{{{variables}}}} match any segment, and scheme, host and a leading
{{{{baseUrl}}}} are left out. When several examples fit, the one whose saved request
matches best (path, query, headers, body) wins. Unmatched requests get 404
with a JSON list of the routes. Every answer allows CORS.

<collection> is the collection folder or any folder inside it (default: the
current folder); the whole collection is served.

Options:
  -p, --port <PORT>     Listen on PORT (default {DEFAULT_PORT}; 0 picks a free one)
      --host <ADDR>     Listen on ADDR (default 127.0.0.1, this machine only)
  -e, --env <NAME>      Resolve {{{{variables}}}} with the environment NAME
  -h, --help            Show this help

The first line on stdout is `rt mock: listening on http://<addr>:<port>`,
followed by the routes. Each request is logged to stderr. Stop with Ctrl-C.
Exit code {usage} when the collection cannot be read or the port is taken.
",
        usage = crate::EXIT_USAGE,
    )
}

/// Parses the arguments after `rt mock`. `Ok(None)` is never returned (help is a flag, so
/// `rt mock --help` prints this command's help, not the global one).
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| format!("mock: {e}");
    let mut args = Args::default();
    let mut collection: Option<PathBuf> = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => args.help = true,
            Short('p') | Long("port") => {
                args.port = parser.value().map_err(err)?.parse().map_err(err)?
            }
            Long("host") => args.host = parser.value().map_err(err)?.parse().map_err(err)?,
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Value(v) if collection.is_none() => collection = Some(PathBuf::from(v)),
            other => return Err(format!("mock: {}", other.unexpected())),
        }
    }
    if let Some(c) = collection {
        args.collection = c;
    }
    Ok(Some(args))
}

/// The collection root at or above `dir`.
pub(crate) fn collection_root(dir: &Path) -> Result<PathBuf, String> {
    let dir =
        std::fs::canonicalize(dir).map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    if dir.is_file() {
        return Err(format!(
            "{} is a file, not a collection folder",
            dir.display()
        ));
    }
    dir.ancestors()
        .find(|d| d.join(rt_format::COLLECTION_FILE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "no {} in {} or any folder above",
                rt_format::COLLECTION_FILE,
                dir.display()
            )
        })
}

/// Reads the collection and builds the routes.
pub fn load(args: &Args) -> Result<Mock, String> {
    let root = collection_root(&args.collection)?;
    let doc = crate::load_collection(&root)?;
    Mock::new(&doc, args.environment.as_deref())
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if args.help {
        let _ = stdout.write_all(help().as_bytes());
        return crate::EXIT_OK;
    }
    let mock = match load(&args) {
        Ok(m) => m,
        Err(e) => {
            let _ = writeln!(stderr, "rt mock: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "rt mock: cannot start runtime: {e}");
            return crate::EXIT_TRANSPORT;
        }
    };
    let addr = SocketAddr::new(args.host, args.port);
    let listener = match runtime.block_on(tokio::net::TcpListener::bind(addr)) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(stderr, "rt mock: cannot listen on {addr}: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let local = listener.local_addr().unwrap_or(addr);
    let _ = stdout.write_all(banner(&mock, local).as_bytes());
    let _ = stdout.flush();
    if !args.host.is_loopback() {
        let _ = writeln!(
            stderr,
            "rt mock: listening on {} - reachable from other machines",
            args.host
        );
    }
    let result = runtime.block_on(server::serve(listener, std::sync::Arc::new(mock)));
    match result {
        Ok(()) => crate::EXIT_OK,
        Err(e) => {
            let _ = writeln!(stderr, "rt mock: {e}");
            crate::EXIT_TRANSPORT
        }
    }
}

/// What `rt mock` prints at start: the address, then one line per route.
pub fn banner(mock: &Mock, addr: SocketAddr) -> String {
    let host = match addr.ip() {
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    let routes = mock.routes();
    let mut out = format!("rt mock: listening on http://{host}:{}\n", addr.port());
    if routes.is_empty() {
        out.push_str(
            "  no request in this collection has saved examples; every request gets 404\n",
        );
    }
    for r in routes {
        out.push_str(&format!(
            "  {:<7} {}  ({}: {})\n",
            r.method,
            r.path,
            r.request,
            r.examples.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests;
