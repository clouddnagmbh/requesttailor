// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The stdio transport (native only): the server is a child process speaking
//! newline-delimited JSON-RPC on stdin/stdout. A reader thread turns stdout into messages;
//! another keeps the end of stderr for error messages.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use super::{McpError, keep_notification};

/// Bytes of stderr kept for error messages.
const STDERR_TAIL: usize = 4096;

pub(super) struct StdioWire {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Value>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

/// The working directory: `cwd` below `root` (no `..`, not absolute), else `root`.
fn working_dir(cwd: Option<&str>, root: Option<&Path>) -> Result<Option<PathBuf>, McpError> {
    let Some(cwd) = cwd.filter(|c| !c.trim().is_empty()) else {
        return Ok(root.map(Path::to_path_buf));
    };
    let relative = Path::new(cwd);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(McpError::Connect(format!(
            "cwd {cwd:?} must be a folder inside the collection"
        )));
    }
    Ok(Some(match root {
        Some(r) => r.join(relative),
        None => relative.to_path_buf(),
    }))
}

impl StdioWire {
    pub(super) fn spawn(
        command: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: Option<&str>,
        root: Option<&Path>,
    ) -> Result<Self, McpError> {
        if command.trim().is_empty() {
            return Err(McpError::Connect("no command given".into()));
        }
        let dir = working_dir(cwd, root)?;
        // A relative path with a separator (`./server`, `bin/server`) is relative to the
        // working directory, as in a shell started there.
        let program = match &dir {
            Some(d) if command.contains(['/', '\\']) && Path::new(command).is_relative() => {
                d.join(command)
            }
            _ => PathBuf::from(command),
        };
        let mut cmd = Command::new(&program);
        cmd.args(args)
            .envs(env.iter().filter(|(n, _)| !n.is_empty()).cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(d) = &dir {
            cmd.current_dir(d);
        }
        let mut child = cmd.spawn().map_err(|e| {
            McpError::Connect(match e.kind() {
                std::io::ErrorKind::NotFound => format!("{command}: command not found"),
                _ => format!("cannot start {command}: {e}"),
            })
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped");
        let stderr_pipe = child.stderr.take().expect("piped");
        let (tx, messages) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                // Lines that are not JSON (a server printing to stdout) are skipped.
                if let Ok(v) = serde_json::from_str::<Value>(line.trim())
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let tail = Arc::clone(&stderr);
        std::thread::spawn(move || {
            let mut pipe = stderr_pipe;
            let mut buf = [0u8; 1024];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap_or_else(PoisonError::into_inner);
                t.extend_from_slice(&buf[..n]);
                if t.len() > STDERR_TAIL {
                    let cut = t.len() - STDERR_TAIL;
                    t.drain(..cut);
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            messages,
            stderr,
        })
    }

    /// How the process ended, with the end of its stderr.
    fn ended(&mut self) -> McpError {
        // The reader saw the end of stdout; give the process a moment to exit.
        let mut status = None;
        for _ in 0..20 {
            if let Ok(Some(s)) = self.child.try_wait() {
                status = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut detail = match status {
            Some(s) => format!(" ({s})"),
            None => String::new(),
        };
        let stderr = self.stderr.lock().unwrap_or_else(PoisonError::into_inner);
        let text = String::from_utf8_lossy(&stderr);
        let text = text.trim();
        if !text.is_empty() {
            detail.push_str(": ");
            detail.push_str(text);
        }
        McpError::Closed(detail)
    }

    fn send(&mut self, message: &Value) -> Result<(), McpError> {
        let Some(stdin) = &mut self.stdin else {
            return Err(McpError::Closed(String::new()));
        };
        let mut line = serde_json::to_vec(message).expect("JSON serializes");
        line.push(b'\n');
        if stdin.write_all(&line).and_then(|_| stdin.flush()).is_err() {
            return Err(self.ended());
        }
        Ok(())
    }

    /// Sends one message; with `id`, waits for its response (blocking, at most `timeout`).
    /// Notifications that arrive meanwhile go to `notifications`.
    pub(super) fn exchange(
        &mut self,
        message: &Value,
        id: Option<u64>,
        timeout: Duration,
        notifications: &mut Vec<Value>,
    ) -> Result<Option<Value>, McpError> {
        self.send(message)?;
        let Some(id) = id else {
            return Ok(None);
        };
        let deadline = web_time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(web_time::Instant::now());
            let m = match self.messages.recv_timeout(left) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => return Err(McpError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(self.ended()),
            };
            let is_response = m.get("result").is_some() || m.get("error").is_some();
            match (m.get("method").and_then(Value::as_str), m.get("id")) {
                // A request from the server: answer it, we offer no capabilities.
                (Some(method), Some(their_id)) => {
                    let reply = match method {
                        "ping" => json!({ "jsonrpc": "2.0", "id": their_id, "result": {} }),
                        _ => json!({ "jsonrpc": "2.0", "id": their_id,
                            "error": { "code": -32601, "message": format!("RequestTailor does not support {method}") } }),
                    };
                    self.send(&reply)?;
                }
                // A notification (logging, progress): kept for the call's result.
                (Some(_), None) => keep_notification(notifications, m),
                (None, Some(v)) if is_response && v.as_u64() == Some(id) => return Ok(Some(m)),
                _ => {}
            }
        }
    }

    /// Closes stdin, then kills the process if it is still running after two seconds.
    pub(super) fn close(mut self) {
        self.stdin = None;
        for _ in 0..200 {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StdioWire {
    fn drop(&mut self) {
        // Never leave a server running when a connection is dropped without `close`.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_dirs_stay_inside_the_collection() {
        let root = Path::new("/c");
        assert_eq!(working_dir(None, Some(root)).unwrap(), Some(root.into()));
        assert_eq!(
            working_dir(Some("tools/bin"), Some(root)).unwrap(),
            Some(root.join("tools/bin"))
        );
        for bad in ["../x", "/abs", "a/../../b"] {
            assert!(working_dir(Some(bad), Some(root)).is_err(), "{bad}");
        }
    }
}
