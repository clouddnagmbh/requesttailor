// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sessions in the desktop app (wave 8, strand A): WebSocket, SSE and gRPC requests opened
//! with `rt_core::stream` over the native transport, events over a Tauri `Channel`.
//! Commands: `stream_open`, `stream_send`, `stream_finish`, `stream_close`, `stream_log`,
//! `stream_release`.
//!
//! Like a send: secret values come from the keychain (and external secrets), never from the
//! webview; the services are the send services of the open collection. Each session runs on
//! a blocking thread driven by the app's runtime (the core's futures are not `Send`) until it
//! ends or is released; commands reach it over a channel. Every event the webview gets is
//! masked; `stream_log` returns the masked history, also after the session ended, until
//! `stream_release`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rt_app::{AppError, SecretStore, SendInput};
use rt_core::model::MessageVariant;
use rt_core::send::{SendError, SendOptions};
use rt_core::stream::{Command, Frame, StreamEvent, StreamLog};
use rt_core::transport::native::NativeTransport;
use rt_core::vars::Scope;
use tauri::State;
use tauri::ipc::Channel;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// One open (or ended, not yet released) session.
struct Handle {
    commands: UnboundedSender<Command>,
    log: Arc<Mutex<StreamLog>>,
    /// The session's variables, for `{{templates}}` in messages.
    scope: Scope,
}

/// The app's sessions by id.
#[derive(Default)]
pub struct Streams {
    next: AtomicU32,
    sessions: Mutex<HashMap<u32, Handle>>,
}

impl Streams {
    fn with<T>(&self, id: u32, f: impl FnOnce(&Handle) -> T) -> Result<T, AppError> {
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        sessions
            .get(&id)
            .map(f)
            .ok_or_else(|| AppError::Shell(format!("no session {id}")))
    }

    fn command(&self, id: u32, command: Command) -> Result<(), AppError> {
        self.with(id, |h| h.commands.send(command))?
            .map_err(|_| AppError::Shell("the session has ended".into()))
    }

    /// Sends a saved message: variables resolved from the session's scope.
    pub fn send(&self, id: u32, message: &MessageVariant) -> Result<(), AppError> {
        let frame = self.with(id, |h| frame(&h.scope, message))??;
        self.command(id, Command::Send(frame))
    }

    pub fn finish(&self, id: u32) -> Result<(), AppError> {
        self.command(id, Command::Finish)
    }

    /// Closes the session; a session that already ended is left as it is.
    pub fn close(&self, id: u32) -> Result<(), AppError> {
        let _ = self.command(id, Command::Close);
        self.with(id, |_| ())
    }

    pub fn log(&self, id: u32) -> Result<StreamLog, AppError> {
        self.with(id, |h| {
            h.log.lock().unwrap_or_else(PoisonError::into_inner).clone()
        })
    }

    /// Forgets the session; a running one is closed (its command channel ends).
    pub fn release(&self, id: u32) {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }
}

/// The app's sessions (one app per process).
static STREAMS: std::sync::LazyLock<Streams> = std::sync::LazyLock::new(Streams::default);

/// The frame of a saved message: variables resolved, binary from base64. gRPC messages are
/// JSON that `rt-grpc` encodes to protobuf (strand B); until that lands they go out as text.
fn frame(scope: &Scope, message: &MessageVariant) -> Result<Frame, AppError> {
    rt_core::stream::message_frame(message, scope).map_err(AppError::Shell)
}

/// What a failed open rejects with.
pub(crate) fn open_error(e: SendError) -> AppError {
    match e {
        SendError::NotFound(path) => AppError::RequestNotFound(path),
        SendError::Auth(m) => AppError::Auth(m),
        SendError::Plugin(m) => AppError::Plugin(m),
        e => AppError::Shell(rt_core::stream::error_text(&e)),
    }
}

/// Opens the request at `path` as a session; `emit` gets every (masked) event.
pub async fn open<S: SecretStore>(
    core: &Core<S>,
    streams: &Streams,
    path: &str,
    input: SendInput,
    emit: impl Fn(StreamEvent) + Send + 'static,
) -> Result<u32, AppError> {
    // Never trusted from the webview, as at send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let (snapshot, collection) = core.stream_snapshot(path, input).await?;
    let parts = Arc::clone(&core.services);
    let (commands, mut rx) = unbounded_channel();
    let (opened_tx, opened_rx) = tokio::sync::oneshot::channel();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = match NativeTransport::new() {
            Ok(t) => t,
            Err(e) => {
                let _ = opened_tx.send(Err(SendError::Transport(e)));
                return;
            }
        };
        let services = parts.for_send(&collection);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        runtime.block_on(async {
            let opened = snapshot
                .open_stream(&transport, &transport, &options, &services.services())
                .await;
            let mut session = match opened {
                Ok(session) => session,
                Err(e) => {
                    let _ = opened_tx.send(Err(e));
                    return;
                }
            };
            let log = Arc::new(Mutex::new(session.log.clone()));
            if opened_tx
                .send(Ok((Arc::clone(&log), session.scope.clone())))
                .is_err()
            {
                session.close().await;
                return;
            }
            session
                .drive(
                    |cx| rx.poll_recv(cx),
                    |event| {
                        log.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .events
                            .push(event.clone());
                        emit(event.clone());
                    },
                )
                .await;
        });
    });
    let (log, scope) = opened_rx
        .await
        .map_err(|_| AppError::Shell("the session failed to start".into()))?
        .map_err(open_error)?;
    let id = streams.next.fetch_add(1, Ordering::SeqCst) + 1;
    streams
        .sessions
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            id,
            Handle {
                commands,
                log,
                scope,
            },
        );
    Ok(id)
}

#[tauri::command]
pub async fn stream_open(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
    on_event: Channel<StreamEvent>,
) -> Result<u32, CommandError> {
    Ok(open(&core, &STREAMS, &path, input, move |event| {
        let _ = on_event.send(event);
    })
    .await?)
}

#[tauri::command]
pub async fn stream_send(session: u32, message: MessageVariant) -> Result<(), CommandError> {
    Ok(STREAMS.send(session, &message)?)
}

#[tauri::command]
pub async fn stream_finish(session: u32) -> Result<(), CommandError> {
    Ok(STREAMS.finish(session)?)
}

#[tauri::command]
pub async fn stream_close(session: u32) -> Result<(), CommandError> {
    Ok(STREAMS.close(session)?)
}

#[tauri::command]
pub async fn stream_log(session: u32) -> Result<StreamLog, CommandError> {
    Ok(STREAMS.log(session)?)
}

#[tauri::command]
pub async fn stream_release(session: u32) -> Result<(), CommandError> {
    STREAMS.release(session);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use rt_app::SecretKey;
    use rt_core::model::MessageKind;
    use rt_core::stream::Direction;

    use super::*;

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SECRET: &str = "desktop-ws-secret";

    /// A collection with a WebSocket request and an SSE request against `addr`; `Local` has
    /// a secret `token`.
    fn collection(tag: &str, addr: std::net::SocketAddr) -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("rt-desktop-streams-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("environments")).unwrap();
        let write = |path: &str, text: String| std::fs::write(dir.join(path), text).unwrap();
        write(
            "opencollection.yml",
            "opencollection: \"1.0.0\"\ninfo:\n  name: Streams\n".into(),
        );
        write(
            "socket.yml",
            format!(
                "info:\n  name: socket\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: ws://{addr}/ws/echo?t={{{{token}}}}\n  message:\n    type: text\n    data: hi {{{{token}}}}\n  auth:\n    type: none\n"
            ),
        );
        write(
            "events.yml",
            format!(
                "info:\n  name: events\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: http://{addr}/sse/count/2\n"
            ),
        );
        write(
            "environments/Local.yml",
            "name: Local\nvariables:\n  - secret: true\n    name: token\n".into(),
        );
        TempDir(dir)
    }

    fn core_with(root: &Path) -> Core<MemorySecrets> {
        let secrets = MemorySecrets::default();
        let core = Core::new(secrets).unwrap();
        core.open(root).unwrap();
        core.secret_set("Local", "token", SECRET).unwrap();
        core
    }

    fn local() -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            // Ignored: the keychain is the only source of secret values.
            secrets: vec![("token".into(), "from-webview".into())],
        }
    }

    async fn wait_for(log: impl Fn() -> StreamLog, done: impl Fn(&StreamLog) -> bool) -> StreamLog {
        for _ in 0..250 {
            let current = log();
            if done(&current) {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out; log: {:?}", log());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn websocket_session_with_keychain_secrets_masked() {
        let addr = rt_testserver::spawn_local().await;
        let tmp = collection("ws", addr);
        let core = core_with(&tmp.0);
        let streams = Streams::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let id = open(&core, &streams, "socket.yml", local(), move |e| {
            sink.lock().unwrap().push(e)
        })
        .await
        .unwrap();
        let message = MessageVariant {
            title: String::new(),
            selected: true,
            kind: MessageKind::Text,
            data: "hi {{token}}".into(),
        };
        streams.send(id, &message).unwrap();
        wait_for(
            || streams.log(id).unwrap(),
            |l| {
                l.events.iter().any(|e| {
                    matches!(
                        e,
                        StreamEvent::Message {
                            direction: Direction::Received,
                            ..
                        }
                    )
                })
            },
        )
        .await;
        streams.close(id).unwrap();
        let log = wait_for(
            || streams.log(id).unwrap(),
            |l| matches!(l.events.last(), Some(StreamEvent::Closed { .. })),
        )
        .await;
        let json = serde_json::to_string(&log).unwrap();
        assert!(!json.contains(SECRET), "{json}");
        assert!(!json.contains("from-webview"), "{json}");
        assert!(json.contains("hi ***"), "{json}");
        assert_eq!(log.kind, "websocket");
        assert_eq!(seen.lock().unwrap().len(), log.events.len());
        streams.release(id);
        assert!(streams.log(id).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sse_session_and_open_errors() {
        let addr = rt_testserver::spawn_local().await;
        let tmp = collection("sse", addr);
        let core = core_with(&tmp.0);
        let streams = Streams::default();
        let id = open(&core, &streams, "events.yml", local(), |_| {})
            .await
            .unwrap();
        let log = wait_for(
            || streams.log(id).unwrap(),
            |l| matches!(l.events.last(), Some(StreamEvent::Closed { .. })),
        )
        .await;
        assert_eq!(log.kind, "sse");
        assert_eq!(log.events.len(), 4, "{log:?}");

        let error = open(&core, &streams, "missing.yml", local(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::RequestNotFound(_)), "{error:?}");
    }
}
