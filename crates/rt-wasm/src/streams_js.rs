// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sessions in the browser (wave 8, strand A): WebSocket, SSE and gRPC requests opened with
//! `rt_core::stream` over the open `Workspace`, prepared like `Workspace.send`.
//!
//! ```text
//! Workspace.openStream(path, input, companion, services, onEvent) -> Promise<StreamHandle>
//! StreamHandle.send(message)   MessageVariant; `{{variables}}` resolved from the session
//! StreamHandle.finish()        gRPC: no more messages from the client
//! StreamHandle.close()
//! StreamHandle.log()           the masked StreamLog so far (also after the end)
//! StreamHandle.ended           whether the session is over
//! StreamHandle.release()       closes a running session and forgets it
//! ```
//!
//! With a paired companion (`companion` as for `Workspace.send`) every kind goes through its
//! relay `GET /v1/stream` (a browser WebSocket, the token in the query): the companion opens
//! the session natively, so handshake headers, auth and gRPC work as in the desktop app.
//! Without the companion only SSE works, over `fetch` (CORS applies); WebSocket and gRPC fail
//! with a `blocked` transport error that says the companion is needed: a browser WebSocket
//! cannot set handshake headers (auth, cookies), and browsers do not speak gRPC.
//!
//! `onEvent(event)` gets every event, masked (`opened` first, `closed`/`error` last). The
//! promise rejects with an `AppError` when the session could not be opened (request not
//! found, auth, plugin, script, or the transport error's text with kind `shell`). `input`
//! carries the secret values, as for a send; they never leave this module unmasked.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::task::{Poll, Waker};

use js_sys::{Function, Promise, Reflect};
use rt_app::{AppError, SendInput};
use rt_core::model::MessageVariant;
use rt_core::send::{SendError, SendOptions};
use rt_core::services::LocalBoxFuture;
use rt_core::stream::sse::SseSession;
use rt_core::stream::{
    Command, Frame, StreamEvent, StreamLog, StreamOpen, StreamSession, StreamTransport,
};
use rt_core::transport::companion::{
    CompanionTransport, StreamClientMessage, StreamServerMessage, WireStreamOpen,
};
use rt_core::transport::fetch::FetchTransport;
use rt_core::transport::{Transport, TransportError};
use rt_core::vars::Scope;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

use crate::workspace_js::{WorkspaceJs, app_error, from_js, named_error, to_js};
use crate::{BODY_LIMIT, Companion};

/// What a WebSocket or gRPC session without the companion fails with.
pub const NEEDS_COMPANION: &str = "WebSocket and gRPC sessions in the browser need the companion (`rt agent`): turn it on and pair it";

// ---- the handle JavaScript keeps ----

/// State shared by the handle and the task that drives the session.
struct HandleState {
    commands: RefCell<VecDeque<Command>>,
    waker: RefCell<Option<Waker>>,
    released: Cell<bool>,
    ended: Cell<bool>,
    log: RefCell<StreamLog>,
    /// The session's variables, for `{{templates}}` in messages.
    scope: Scope,
}

impl HandleState {
    fn push(&self, command: Command) -> Result<(), JsValue> {
        if self.ended.get() || self.released.get() {
            return Err(named_error("shell", "the session has ended"));
        }
        self.commands.borrow_mut().push_back(command);
        self.wake();
        Ok(())
    }

    fn wake(&self) {
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }

    fn poll_command(&self, cx: &mut std::task::Context<'_>) -> Poll<Option<Command>> {
        if let Some(command) = self.commands.borrow_mut().pop_front() {
            return Poll::Ready(Some(command));
        }
        if self.released.get() {
            return Poll::Ready(None);
        }
        *self.waker.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// An open (or ended) session; see the module documentation.
#[wasm_bindgen]
pub struct StreamHandle {
    state: Rc<HandleState>,
}

#[wasm_bindgen]
impl StreamHandle {
    /// Sends a saved message (`MessageVariant`): variables resolved, binary from base64.
    pub fn send(&self, message: JsValue) -> Result<(), JsValue> {
        let message: MessageVariant = from_js(&message, "message")?;
        let frame = rt_core::stream::message_frame(&message, &self.state.scope)
            .map_err(|m| named_error("shell", &m))?;
        self.state.push(Command::Send(frame))
    }

    pub fn finish(&self) -> Result<(), JsValue> {
        self.state.push(Command::Finish)
    }

    /// Closes the session; one that already ended is left as it is.
    pub fn close(&self) {
        let _ = self.state.push(Command::Close);
    }

    pub fn log(&self) -> Result<JsValue, JsValue> {
        to_js(&*self.state.log.borrow())
    }

    #[wasm_bindgen(getter)]
    pub fn ended(&self) -> bool {
        self.state.ended.get()
    }

    /// Closes a running session; the handle stays readable (`log`).
    pub fn release(&self) {
        self.state.released.set(true);
        self.state.wake();
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.release();
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Opens the request at `path` as a session; see the module documentation.
    #[wasm_bindgen(js_name = openStream)]
    pub fn open_stream(
        &self,
        path: &str,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        #[wasm_bindgen(js_name = onEvent)] on_event: Function,
    ) -> Result<Promise, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let snapshot = self.workspace().snapshot(path, input).map_err(app_error)?;
        let parts = crate::services_js::SendServices::from_js(&services)?;

        let mut settle = None;
        let promise = Promise::new(&mut |resolve, reject| settle = Some((resolve, reject)));
        let (resolve, reject) = settle.expect("the executor runs synchronously");
        wasm_bindgen_futures::spawn_local(async move {
            let services = parts.services();
            let options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let settle = Settle {
                resolve,
                reject,
                on_event,
            };
            match companion {
                Some(c) => {
                    let http = CompanionTransport::new(c.url.clone(), c.token.clone());
                    let streams = BrowserStreams::Companion(c);
                    run(&snapshot, &http, &streams, &options, &services, settle).await
                }
                None => {
                    let http = FetchTransport::new();
                    let streams = BrowserStreams::Fetch(FetchTransport::new());
                    run(&snapshot, &http, &streams, &options, &services, settle).await
                }
            }
        });
        Ok(promise)
    }
}

struct Settle {
    resolve: Function,
    reject: Function,
    on_event: Function,
}

/// Opens the session, resolves the promise with its handle and drives it to the end.
async fn run<T: Transport>(
    snapshot: &rt_app::SendSnapshot,
    http: &T,
    streams: &BrowserStreams,
    options: &SendOptions,
    services: &rt_core::services::Services<'_>,
    settle: Settle,
) {
    let mut session = match snapshot.open_stream(http, streams, options, services).await {
        Ok(session) => session,
        Err(e) => {
            let _ = settle
                .reject
                .call1(&JsValue::NULL, &app_error(open_error(e)));
            return;
        }
    };
    let state = Rc::new(HandleState {
        commands: RefCell::new(VecDeque::new()),
        waker: RefCell::new(None),
        released: Cell::new(false),
        ended: Cell::new(false),
        log: RefCell::new(session.log.clone()),
        scope: session.scope.clone(),
    });
    let handle = StreamHandle {
        state: Rc::clone(&state),
    };
    if settle
        .resolve
        .call1(&JsValue::NULL, &JsValue::from(handle))
        .is_err()
    {
        session.close().await;
        return;
    }
    let on_event = settle.on_event;
    session
        .drive(
            |cx| state.poll_command(cx),
            |event| {
                state.log.borrow_mut().events.push(event.clone());
                if let Ok(value) = to_js(event) {
                    // A throwing listener must not stop the session.
                    let _ = on_event.call1(&JsValue::NULL, &value);
                }
            },
        )
        .await;
    state.ended.set(true);
}

/// What a failed open rejects with.
fn open_error(e: SendError) -> AppError {
    match e {
        SendError::NotFound(path) => AppError::RequestNotFound(path),
        SendError::Auth(m) => AppError::Auth(m),
        SendError::Plugin(m) => AppError::Plugin(m),
        e => AppError::Shell(rt_core::stream::error_text(&e)),
    }
}

// ---- transports ----

/// The browser's session transports: the companion relay, or `fetch` (SSE only).
enum BrowserStreams {
    Companion(Companion),
    Fetch(FetchTransport),
}

impl StreamTransport for BrowserStreams {
    fn open<'a>(
        &'a self,
        open: StreamOpen,
    ) -> LocalBoxFuture<'a, Result<Box<dyn StreamSession + 'a>, TransportError>> {
        Box::pin(async move {
            match self {
                BrowserStreams::Companion(c) => {
                    let session = CompanionSession::open(c, open).await?;
                    Ok(Box::new(session) as Box<dyn StreamSession + 'a>)
                }
                BrowserStreams::Fetch(fetch) => match open {
                    StreamOpen::Sse { request } => {
                        let timeout = request.options.timeout;
                        let opening = SseSession::open_over(fetch, request);
                        let session = match timeout {
                            Some(t) => with_timeout(opening, t.as_millis() as u64).await?,
                            None => opening.await,
                        }?;
                        Ok(Box::new(session) as Box<dyn StreamSession + 'a>)
                    }
                    _ => Err(TransportError::Blocked(NEEDS_COMPANION.into())),
                },
            }
        })
    }
}

/// `future`, or `TransportError::Timeout` after `ms`.
async fn with_timeout<F: Future>(future: F, ms: u64) -> Result<F::Output, TransportError> {
    let mut future = std::pin::pin!(future);
    let mut timer = std::pin::pin!(sleep(ms));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(TransportError::Timeout));
        }
        Poll::Pending
    })
    .await
}

/// Resolves after `ms` (`setTimeout`, in a window and in a worker).
async fn sleep(ms: u64) {
    let promise = Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        if let Ok(set_timeout) =
            Reflect::get(&global, &"setTimeout".into()).and_then(|f| f.dyn_into::<Function>())
        {
            let ms = ms.min(i32::MAX as u64) as i32;
            let _ = set_timeout.call2(&global, &resolve, &ms.into());
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// What arrives on the companion's WebSocket.
enum Incoming {
    Open,
    Text(String),
    /// Closed (or failed: browsers report no detail).
    Closed,
}

#[derive(Default)]
struct SocketState {
    queue: VecDeque<Incoming>,
    waker: Option<Waker>,
    closed: bool,
}

/// A browser WebSocket to the companion with its handlers.
struct Socket {
    ws: WebSocket,
    state: Rc<RefCell<SocketState>>,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
}

impl Socket {
    fn connect(url: &str) -> Result<Self, String> {
        let ws = WebSocket::new(url).map_err(|e| {
            e.dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
                .unwrap_or_else(|| "invalid URL".into())
        })?;
        let state = Rc::new(RefCell::new(SocketState::default()));
        let push = {
            let state = Rc::clone(&state);
            move |incoming: Incoming| {
                let mut s = state.borrow_mut();
                if s.closed {
                    return;
                }
                if matches!(incoming, Incoming::Closed) {
                    s.closed = true;
                }
                s.queue.push_back(incoming);
                if let Some(waker) = s.waker.take() {
                    waker.wake();
                }
            }
        };
        let on_open = {
            let push = push.clone();
            Closure::<dyn FnMut(Event)>::new(move |_| push(Incoming::Open))
        };
        let on_message = {
            let push = push.clone();
            Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                if let Some(text) = e.data().as_string() {
                    push(Incoming::Text(text));
                }
            })
        };
        let on_close = {
            let push = push.clone();
            Closure::<dyn FnMut(CloseEvent)>::new(move |_| push(Incoming::Closed))
        };
        let on_error = Closure::<dyn FnMut(Event)>::new(move |_| push(Incoming::Closed));
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        Ok(Self {
            ws,
            state,
            _on_open: on_open,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }

    /// The next thing that arrived; `Closed` once the socket is gone (and then forever).
    async fn recv(&self) -> Incoming {
        std::future::poll_fn(|cx| {
            let mut s = self.state.borrow_mut();
            if let Some(incoming) = s.queue.pop_front() {
                return Poll::Ready(incoming);
            }
            if s.closed {
                return Poll::Ready(Incoming::Closed);
            }
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }

    fn send(&self, message: &StreamClientMessage) -> Result<(), TransportError> {
        let json = serde_json::to_string(message)
            .map_err(|e| TransportError::Other(format!("encoding the message failed: {e}")))?;
        self.ws
            .send_with_str(&json)
            .map_err(|_| TransportError::Other("the companion connection is closed".into()))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        // Handlers first: the closures are dropped with `self`.
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}

/// `ws://…/v1/stream?token=…` for the companion at `base` (`http://127.0.0.1:47823`).
fn stream_url(base: &str, token: &str) -> String {
    let base = base.trim_end_matches('/');
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_owned()
    };
    format!(
        "{base}/v1/stream?token={}",
        String::from(js_sys::encode_uri_component(token))
    )
}

/// A session relayed by the companion.
struct CompanionSession {
    socket: Socket,
    /// The first event (`opened`), read while opening.
    first: Option<StreamEvent>,
    done: bool,
}

impl CompanionSession {
    async fn open(companion: &Companion, open: StreamOpen) -> Result<Self, TransportError> {
        let unreachable = |detail: &str| {
            TransportError::Blocked(format!(
                "the companion at {} is not reachable or refused the pairing ({detail})",
                companion.url
            ))
        };
        let socket = Socket::connect(&stream_url(&companion.url, &companion.token))
            .map_err(|m| unreachable(&m))?;
        loop {
            match socket.recv().await {
                Incoming::Open => break,
                Incoming::Text(_) => continue,
                Incoming::Closed => return Err(unreachable("the connection closed")),
            }
        }
        socket.send(&StreamClientMessage::Open {
            open: WireStreamOpen::from(open),
        })?;
        loop {
            match socket.recv().await {
                Incoming::Open => continue,
                Incoming::Text(text) => match serde_json::from_str(&text) {
                    Ok(StreamServerMessage::Event { event }) => {
                        let done = matches!(
                            event,
                            StreamEvent::Closed { .. } | StreamEvent::Error { .. }
                        );
                        return Ok(Self {
                            socket,
                            first: Some(event),
                            done,
                        });
                    }
                    Ok(StreamServerMessage::Failed { error }) => return Err(error),
                    Err(e) => {
                        return Err(TransportError::Other(format!(
                            "the companion sent an unknown message: {e}"
                        )));
                    }
                },
                Incoming::Closed => {
                    return Err(TransportError::Other(
                        "the companion closed the session before it opened".into(),
                    ));
                }
            }
        }
    }

    async fn next_event(&mut self) -> Option<StreamEvent> {
        if let Some(event) = self.first.take() {
            return Some(event);
        }
        if self.done {
            return None;
        }
        loop {
            let event = match self.socket.recv().await {
                Incoming::Open => continue,
                Incoming::Text(text) => match serde_json::from_str(&text) {
                    Ok(StreamServerMessage::Event { event }) => event,
                    Ok(StreamServerMessage::Failed { error }) => {
                        StreamEvent::Error { at_ms: 0, error }
                    }
                    Err(_) => continue,
                },
                Incoming::Closed => StreamEvent::Error {
                    at_ms: 0,
                    error: TransportError::Other("the connection to the companion was lost".into()),
                },
            };
            if matches!(
                event,
                StreamEvent::Closed { .. } | StreamEvent::Error { .. }
            ) {
                self.done = true;
            }
            return Some(event);
        }
    }
}

impl StreamSession for CompanionSession {
    fn send(&mut self, frame: Frame) -> LocalBoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move { self.socket.send(&StreamClientMessage::Send { frame }) })
    }

    fn finish(&mut self) -> LocalBoxFuture<'_, Result<(), TransportError>> {
        Box::pin(async move { self.socket.send(&StreamClientMessage::Finish) })
    }

    fn next(&mut self) -> LocalBoxFuture<'_, Option<StreamEvent>> {
        Box::pin(self.next_event())
    }

    fn close(&mut self) -> LocalBoxFuture<'_, ()> {
        Box::pin(async move {
            if !self.done {
                // The companion closes the native session and relays its `closed` event.
                let _ = self.socket.send(&StreamClientMessage::Close);
            }
        })
    }
}
