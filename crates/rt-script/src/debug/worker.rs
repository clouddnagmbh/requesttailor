// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A debug session on a thread of its own (native only). The engine's futures are not `Send`,
//! so the desktop app (whose commands must be) keeps the session here and talks to it over
//! channels. The calls block until the script stops again (call them from a blocking task).

use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use super::{DebugCommand, DebugInput, DebugSession, DebugState, DebugStatus};
use crate::Limits;

type Job = (DebugCommand, Sender<DebugState>);

/// A running session; `Send`, cheap to clone.
#[derive(Clone)]
pub struct DebugHandle {
    id: u32,
    jobs: Sender<Job>,
    state: Arc<Mutex<DebugState>>,
}

impl std::fmt::Debug for DebugHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugHandle").field("id", &self.id).finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl DebugHandle {
    /// Starts the session and returns when the script first stops (or ends).
    pub fn start(id: u32, limits: Limits, input: DebugInput) -> Result<(Self, DebugState), String> {
        let (jobs, rx) = channel::<Job>();
        let (first_tx, first_rx) = channel::<Result<DebugState, String>>();
        let state = Arc::new(Mutex::new(DebugState {
            session: id,
            status: DebugStatus::Running,
            pause: None,
            output: Vec::new(),
            error: None,
        }));
        let shared = Arc::clone(&state);
        std::thread::Builder::new()
            .name(format!("rt-debug-{id}"))
            .spawn(move || {
                let mut session = match DebugSession::new(id, limits, input) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = first_tx.send(Err(e));
                        return;
                    }
                };
                block_on(async {
                    let first = session.start().await;
                    *lock(&shared) = first.clone();
                    let _ = first_tx.send(Ok(first));
                    while let Ok((command, reply)) = rx.recv() {
                        let next = session.command(command).await;
                        *lock(&shared) = next.clone();
                        let _ = reply.send(next.clone());
                        if matches!(next.status, DebugStatus::Finished | DebugStatus::Aborted) {
                            break;
                        }
                    }
                });
            })
            .map_err(|e| format!("cannot start the debugger: {e}"))?;
        let first = first_rx
            .recv()
            .map_err(|_| "the debugger stopped unexpectedly".to_owned())??;
        Ok((Self { id, jobs, state }, first))
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// Sends a command and waits for the next stop (or the end).
    pub fn command(&self, command: DebugCommand) -> DebugState {
        {
            let mut s = lock(&self.state);
            if s.status == DebugStatus::Paused {
                s.status = DebugStatus::Running;
            }
        }
        let (tx, rx) = channel();
        if self.jobs.send((command, tx)).is_err() {
            return self.state();
        }
        rx.recv().unwrap_or_else(|_| self.state())
    }

    /// The state as of the last stop; `running` while a command is being carried out.
    pub fn state(&self) -> DebugState {
        lock(&self.state).clone()
    }

    /// Ends the session.
    pub fn stop(&self) {
        let _ = self.command(DebugCommand::Abort);
    }
}

/// A minimal executor: the engine waits on timers that wake the thread.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct Thread(std::thread::Thread);
    impl Wake for Thread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}
