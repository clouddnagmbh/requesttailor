// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt inbox [--port N] [--status CODE] [--json]` (wave 18, strand E): the local webhook
//! inbox in the foreground. Loopback only, nothing is written to a file; one line per request
//! on stdout (`--json`: one `CapturedRequest` per line); the address goes to stderr. Stops with
//! Ctrl-C (exit 0); a port in use is exit 2. The times are UTC.

use std::io::Write;

use rt_app::inbox::{CapturedRequest, DEFAULT_PORT, Inbox, InboxEvent, InboxSettings};

pub const USAGE: &str = "  rt inbox [--port <PORT>] [--status <CODE>] [--json]\n";

pub const HELP: &str = "rt inbox listens on 127.0.0.1 (default port 47855) and prints every request it gets,\nanswering with a fixed reply (default 200, no body). It keeps requests in memory only\nand has no tunnel: to receive calls from the internet, point your own tool at this\nport. Stop with Ctrl-C.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub settings: InboxSettings,
    pub json: bool,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        settings: InboxSettings {
            port: DEFAULT_PORT,
            ..InboxSettings::default()
        },
        json: false,
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Short('p') | Long("port") => {
                args.settings.port = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("status") => {
                args.settings.status = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("json") => args.json = true,
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let inbox = Inbox::new();
    if let Err(e) = inbox.set_settings(args.settings.clone()) {
        let _ = writeln!(stderr, "rt: inbox: {e}");
        return crate::EXIT_USAGE;
    }
    let status = match inbox.start() {
        Ok(status) => status,
        Err(e) => {
            let _ = writeln!(stderr, "rt: inbox: {e}");
            return crate::EXIT_USAGE;
        }
    };
    let _ = writeln!(
        stderr,
        "Listening on {} (loopback only; requests are kept in memory). Stop with Ctrl-C.",
        status.url.unwrap_or_default()
    );
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
    let code = runtime.block_on(async {
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        let mut since = inbox.events(0).next;
        loop {
            tokio::select! {
                _ = &mut ctrl_c => break,
                () = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
            }
            let page = inbox.events(since);
            since = page.next;
            for event in page.events {
                match event {
                    InboxEvent::Request { request } => {
                        let line = if args.json {
                            serde_json::to_string(&*request).unwrap_or_default()
                        } else {
                            line(&request)
                        };
                        if writeln!(stdout, "{line}")
                            .and_then(|()| stdout.flush())
                            .is_err()
                        {
                            return crate::EXIT_OK; // the reader went away
                        }
                    }
                    InboxEvent::Dropped { count } if !args.json => {
                        let _ = writeln!(stderr, "{count} older requests dropped from the buffer");
                    }
                    _ => {}
                }
            }
        }
        crate::EXIT_OK
    });
    inbox.stop();
    code
}

/// `14:03:22 POST /hooks/orders?x=1 (application/json, 128 B)`.
fn line(request: &CapturedRequest) -> String {
    let secs = request.at.div_euclid(1000).rem_euclid(86_400);
    let content_type = request
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.split(';').next().unwrap_or("").trim().to_owned())
        .filter(|v| !v.is_empty());
    let what = match content_type {
        Some(ct) => format!("{ct}, {} B", request.size),
        None => format!("{} B", request.size),
    };
    format!(
        "{:02}:{:02}:{:02} {} {} ({what})",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
        request.method,
        request.target
    )
}
