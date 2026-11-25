// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The script debugger for the desktop app (`Backend.debugger`, `rt_script::debug`). A session
//! runs the script of one request (`before-request`, `after-response` or `tests`, of the
//! request, a folder or the collection) in the sandbox on a thread of its own and stops it at
//! the breakpoints; the webview sends commands and gets the state back.
//!
//! The scripts and variables are the ones a send would give the sandbox: the open
//! collection as edited (unsaved changes included), the selected environment with its secret
//! values from the keychain (scripts may use them; nothing the debugger shows has them, they
//! are masked), the session variables. `after-response` and `tests` read a kept response
//! (`DebugTarget.response_id`). Only collections the user trusts run scripts here too.
//! `rt.sendRequest` is not available while debugging.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

use rt_app::{AppError, SecretStore, SendInput};
use rt_core::script::ScriptResponse;
use rt_script::Limits;
use rt_script::debug::{
    DebugCommand, DebugHandle, DebugInput, DebugState, DebugStatus, DebugTarget, PrepareArgs,
    prepare,
};
use tauri::State;

use crate::Core;
use crate::commands::{AppCore, CommandError};

/// The sessions of this app, by id.
#[derive(Default)]
pub struct Sessions {
    next: AtomicU32,
    open: Mutex<HashMap<u32, DebugHandle>>,
}

impl Sessions {
    fn insert(&self, id: u32, handle: DebugHandle) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        // Sessions that ended are dropped here, so an abandoned page cannot pile them up.
        open.retain(|_, h| {
            !matches!(
                h.state().status,
                DebugStatus::Finished | DebugStatus::Aborted
            )
        });
        open.insert(id, handle);
    }

    fn get(&self, id: u32) -> Result<DebugHandle, AppError> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
            .ok_or_else(|| shell(format!("there is no debug session {id}")))
    }

    fn remove(&self, id: u32) -> Option<DebugHandle> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id)
    }

    fn next_id(&self) -> u32 {
        self.next.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// The app's sessions (one app per process).
static SESSIONS: LazyLock<Sessions> = LazyLock::new(Sessions::default);

fn shell(message: String) -> AppError {
    AppError::Shell(message)
}

/// The input of a debug run for `target`: the open collection, its environment, the kept
/// response.
pub fn input_of<S: SecretStore>(
    core: &Core<S>,
    target: &DebugTarget,
) -> Result<DebugInput, AppError> {
    let wanted = SendInput {
        environment: target.environment.clone(),
        ..SendInput::default()
    };
    core.with_ai_scope(wanted, |workspace, responses, input, collection_id| {
        if !core.services.scripts.is_trusted(collection_id) {
            return Err(shell(
                "Trust this collection to run its scripts before debugging them.".into(),
            ));
        }
        let snapshot = workspace.snapshot(&target.request_path, input.clone())?;
        let response = match target.response_id {
            None => None,
            Some(id) => {
                let report = responses.report(id)?;
                let received = report
                    .response
                    .as_ref()
                    .ok_or(AppError::ResponseNotFound(id))?;
                Some(ScriptResponse {
                    status: received.status,
                    headers: received.headers.clone(),
                    body: responses.body(id)?.to_vec(),
                    time_ms: u64::try_from(received.total.as_millis()).unwrap_or(u64::MAX),
                    url: received.url.clone(),
                })
            }
        };
        prepare(
            PrepareArgs {
                collection: &snapshot.collection,
                runtime: snapshot.vars.clone(),
                response,
            },
            target,
        )
        .map_err(shell)
    })?
}

/// Starts the session of `input`; returns at its first stop (or its end).
pub fn start(input: DebugInput) -> Result<DebugState, AppError> {
    let id = SESSIONS.next_id();
    let (handle, first) = DebugHandle::start(id, Limits::default(), input).map_err(shell)?;
    SESSIONS.insert(id, handle);
    Ok(first)
}

pub fn command(session: u32, command: DebugCommand) -> Result<DebugState, AppError> {
    Ok(SESSIONS.get(session)?.command(command))
}

pub fn state(session: u32) -> Result<DebugState, AppError> {
    Ok(SESSIONS.get(session)?.state())
}

pub fn stop(session: u32) {
    if let Some(handle) = SESSIONS.remove(session) {
        handle.stop();
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, CommandError> {
    Ok(tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| shell(format!("the debugger failed: {e}")))??)
}

#[tauri::command]
pub async fn debugger_start(
    core: State<'_, AppCore>,
    target: DebugTarget,
) -> Result<DebugState, CommandError> {
    // The input is read here (the workspace lives in the core); the session starts on a
    // blocking thread, which waits for the first stop.
    let input = input_of(&core, &target)?;
    blocking(move || start(input)).await
}

#[tauri::command]
pub async fn debugger_command(
    session: u32,
    command: DebugCommand,
) -> Result<DebugState, CommandError> {
    blocking(move || self::command(session, command)).await
}

#[tauri::command]
pub async fn debugger_state(session: u32) -> Result<DebugState, CommandError> {
    Ok(state(session)?)
}

#[tauri::command]
pub async fn debugger_stop(session: u32) -> Result<(), CommandError> {
    blocking(move || {
        stop(session);
        Ok(())
    })
    .await
}
