// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-odata-testserver [--port N]`: runs the OData test server until killed. Default port 47830.

use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut port = rt_odata_testserver::DEFAULT_PORT;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            _ => usage(),
        }
    }
    let addr = rt_odata_testserver::spawn(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .unwrap_or_else(|e| {
            eprintln!("rt-odata-testserver: cannot bind port {port}: {e}");
            std::process::exit(1);
        });
    // Scripts wait for this line before starting browser tests.
    println!("rt-odata-testserver listening on http://{addr}");
    std::future::pending::<()>().await;
}

fn usage() -> ! {
    eprintln!("usage: rt-odata-testserver [--port N]");
    std::process::exit(2);
}
