// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-testserver [--port N]`: runs the test server until killed. Default port 47811.
//! `rt-testserver --mcp-stdio`: an MCP server on stdin/stdout instead (wave 8, `mcp`).

use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut port = rt_testserver::DEFAULT_PORT;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mcp-stdio" => {
                rt_testserver::mcp::serve_stdio();
                return;
            }
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            _ => usage(),
        }
    }
    let addr = rt_testserver::spawn(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap_or_else(|e| {
            eprintln!("rt-testserver: cannot bind port {port}: {e}");
            std::process::exit(1);
        });
    // Scripts wait for this line before starting browser tests.
    println!("rt-testserver listening on http://{addr}");
    std::future::pending::<()>().await;
}

fn usage() -> ! {
    eprintln!("usage: rt-testserver [--port N] | rt-testserver --mcp-stdio");
    std::process::exit(2);
}
