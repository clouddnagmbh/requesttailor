// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send` on a session request (wave 8, strand A): WebSocket and gRPC requests, and HTTP
//! requests with `--sse`.
//!
//! The session is opened like a send (`rt_core::stream::open`, same services as `rt send`),
//! the selected message (or `--message <title>`) is sent once the session is open, and then:
//!
//! - by default, the session is closed after the first received message (gRPC: when the
//!   call ends), or after 10 s;
//! - `--duration <s>`: closed after that many seconds;
//! - `--until-close`: runs until the server ends it.
//!
//! The output is the masked `StreamLog` as JSON, plus `error` when the session could not be
//! opened. Exit codes as for `rt send`: 0 when the session opened (whatever its close code),
//! 1 for a transport error (also an `error` event), 2 when the request or the message is not
//! found, 3 when the request could not be prepared.

use std::time::Duration;

use rt_core::model::{MessageVariant, Protocol, RequestSpec};
use rt_core::prepare::PrepareInput;
use rt_core::send::{SendError, SendOptions};
use rt_core::stream::{Command, Frame, StreamEvent, StreamLog};
use rt_core::transport::native::NativeTransport;
use rt_format::CollectionDoc;
use serde::Serialize;

use crate::{DirFiles, EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, EXIT_USAGE, SendArgs};

/// How long the default mode waits for the first received message.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(10);

/// Session options of `rt send`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionArgs {
    /// `--sse`: read an HTTP request's response as Server-Sent Events.
    pub sse: bool,
    /// `--message <title>`: the message variant to send (default: the selected one).
    pub message: Option<String>,
    pub until: Until,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Until {
    /// The first received message, at most [`DEFAULT_WAIT`].
    #[default]
    FirstMessage,
    /// `--duration <s>`.
    Duration(Duration),
    /// `--until-close`.
    Close,
}

/// Whether `rt send` opens `spec` as a session.
pub fn is_session(spec: &RequestSpec, args: &SessionArgs) -> bool {
    !spec.protocol.is_http() || args.sse
}

/// What `rt send` prints for a session request.
#[derive(Debug, Clone, Serialize)]
pub struct SessionOutput {
    #[serde(flatten)]
    pub log: StreamLog,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SendError>,
}

impl SessionOutput {
    fn failed(request_path: &str, args: &SendArgs, kind: &str, error: SendError) -> Self {
        Self {
            log: StreamLog {
                version: rt_core::stream::LOG_VERSION,
                request_path: request_path.to_owned(),
                environment: args.environment.clone(),
                kind: kind.to_owned(),
                ..Default::default()
            },
            error: Some(error),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match &self.error {
            Some(SendError::Transport(_)) => EXIT_TRANSPORT,
            Some(SendError::NotFound(_)) => EXIT_USAGE,
            Some(_) => EXIT_PREPARE,
            None if self
                .log
                .events
                .iter()
                .any(|e| matches!(e, StreamEvent::Error { .. })) =>
            {
                EXIT_TRANSPORT
            }
            None => EXIT_OK,
        }
    }

    pub fn render(&self, pretty: bool) -> String {
        let mut out = if pretty {
            serde_json::to_string_pretty(self)
        } else {
            serde_json::to_string(self)
        }
        .expect("output serializes");
        out.push('\n');
        out
    }
}

fn kind_of(spec: &RequestSpec) -> &'static str {
    match spec.protocol {
        Protocol::WebSocket(_) => "websocket",
        Protocol::Grpc(_) => "grpc",
        Protocol::Http => "sse",
    }
}

fn messages_of(spec: &RequestSpec) -> &[MessageVariant] {
    match &spec.protocol {
        Protocol::WebSocket(ws) => &ws.messages,
        Protocol::Grpc(grpc) => &grpc.messages,
        Protocol::Http => &[],
    }
}

/// Opens the session, sends the message, waits as `args.session.until` says, closes.
pub async fn run_session(
    doc: &CollectionDoc,
    request_path: &str,
    args: &SendArgs,
    files: &DirFiles,
    transport: &NativeTransport,
) -> SessionOutput {
    let Some(context) = doc.model().request(request_path) else {
        return SessionOutput::failed(
            request_path,
            args,
            "",
            SendError::NotFound(request_path.to_owned()),
        );
    };
    let spec = &context.request.spec;
    let kind = kind_of(spec);
    let message = match &args.session.message {
        Some(title) => match messages_of(spec).iter().find(|m| &m.title == title) {
            Some(m) => Some(m.clone()),
            None => {
                return SessionOutput::failed(
                    request_path,
                    args,
                    kind,
                    SendError::NotFound(format!("message {title:?} in {request_path}")),
                );
            }
        },
        None => rt_core::stream::selected_message(messages_of(spec)).cloned(),
    };
    let collection =
        match crate::secrets::resolve(doc.model(), args.environment.as_deref(), &args.vars).await {
            Ok(c) => c,
            Err(e) => return SessionOutput::failed(request_path, args, kind, SendError::Auth(e)),
        };
    let cli = match crate::services::CliServices::new(args, files.root()) {
        Ok(s) => s,
        Err(e) => return SessionOutput::failed(request_path, args, kind, SendError::Plugin(e)),
    };
    let context = collection.request(request_path).expect("same collection");
    // Wave 8, strand B: gRPC messages are encoded with the request's descriptors.
    let grpc = match &context.request.spec.protocol {
        Protocol::Grpc(g) => {
            match crate::grpc::descriptors(&collection, request_path, g, args, files, transport)
                .await
            {
                Ok(d) => Some((d, g.method.clone())),
                Err(e) => return SessionOutput::failed(request_path, args, kind, e),
            }
        }
        _ => None,
    };
    let input = PrepareInput {
        environment: args.environment.as_deref(),
        globals: Vec::new(),
        runtime: args.vars.clone(),
    };
    let options = SendOptions {
        iteration: None,
        body_limit: args.body_limit,
    };
    let mut session = match rt_core::stream::open(
        &context,
        &input,
        files,
        &options,
        &cli.services(),
        transport,
        transport,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return SessionOutput::failed(request_path, args, kind, e),
    };
    if let Some((d, method)) = grpc.clone() {
        session.decode = Some(Box::new(move |e| d.decode_event(&method, e)));
    }
    // SSE sessions only receive; an empty message is not sent.
    let frame = match message.filter(|m| kind != "sse" && !m.data.is_empty()) {
        Some(m) => match frame_for(&session, grpc.as_ref(), &m) {
            Ok(frame) => Some(frame),
            Err(e) => {
                session.close().await;
                return SessionOutput::failed(request_path, args, kind, SendError::Prepare(e));
            }
        },
        None => None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let until = args.session.until;
    let limit = match until {
        Until::FirstMessage => Some(DEFAULT_WAIT),
        Until::Duration(d) => Some(d),
        Until::Close => None,
    };
    let mut frame = frame;
    let deadline = limit.map(|d| tokio::time::Instant::now() + d);
    let timer = {
        let tx = tx.clone();
        async move {
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
                let _ = tx.send(Command::Close);
            }
            std::future::pending::<()>().await;
        }
    };
    let drive = session.drive(
        |cx| rx.poll_recv(cx),
        |event| match event {
            StreamEvent::Opened { .. } => {
                if let Some(frame) = frame.take() {
                    let _ = tx.send(Command::Send(frame));
                    if kind == "grpc" {
                        let _ = tx.send(Command::Finish);
                    }
                }
            }
            StreamEvent::Message {
                direction: rt_core::stream::Direction::Received,
                ..
            } if until == Until::FirstMessage && kind != "grpc" => {
                let _ = tx.send(Command::Close);
            }
            _ => {}
        },
    );
    {
        let mut timer = std::pin::pin!(timer);
        let mut drive = std::pin::pin!(drive);
        std::future::poll_fn(|cx| {
            use std::future::Future as _;
            let _ = timer.as_mut().poll(cx);
            drive.as_mut().poll(cx)
        })
        .await;
    }
    SessionOutput {
        log: session.log,
        error: None,
    }
}

/// The frame of a saved message: text or binary as written (variables resolved). gRPC
/// messages are JSON that `rt-grpc` encodes to protobuf (wave 8, strand B).
fn frame_for(
    session: &rt_core::stream::Session<'_>,
    grpc: Option<&(rt_grpc::Descriptors, String)>,
    message: &MessageVariant,
) -> Result<Frame, String> {
    let frame = session.frame(message)?;
    match grpc {
        Some((d, method)) => crate::grpc::encode(d, method, frame),
        None => Ok(frame),
    }
}

/// Parses the session option `name` (`sse`, `message`, `duration`, `until-close`) of
/// `rt send`, taking its value from `parser`.
pub(crate) fn parse_option(
    name: &str,
    parser: &mut lexopt::Parser,
    args: &mut SessionArgs,
) -> Result<(), String> {
    use lexopt::ValueExt as _;
    let err = |e: lexopt::Error| e.to_string();
    match name {
        "sse" => args.sse = true,
        "message" => {
            args.message = Some(parser.value().map_err(err)?.string().map_err(err)?);
        }
        "duration" => {
            let seconds: f64 = parser.value().map_err(err)?.parse().map_err(err)?;
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(format!("--duration expects seconds, got {seconds}"));
            }
            args.until = Until::Duration(Duration::from_secs_f64(seconds));
        }
        "until-close" => args.until = Until::Close,
        other => return Err(format!("unknown session option --{other}")),
    }
    Ok(())
}
