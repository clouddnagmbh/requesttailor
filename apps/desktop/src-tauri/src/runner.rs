// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner in the desktop app (wave 7, strand D owns this file): runs
//! `rt_core::runner::run` on a blocking thread with the send services, progress over a Tauri
//! channel. Commands: `runner_plan`, `runner_parse_data`, `runner_run`, `runner_cancel`.
//!
//! Like a send: secret values come from the keychain (and external secrets), never from the
//! webview; the services are the send services of the open collection (`services.rs`).
//! One run at a time; `runner_cancel` stops it before its next request or during the delay.
//! Variable changes stay in the run. Step events carry a body preview only
//! (`rt_app::runner::for_view`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rt_app::{AppError, SecretStore, SendInput};
use rt_core::runner::{RunEvent, RunPlan, RunSummary};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;
use tauri::ipc::Channel;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// A data row as the UI gets it.
pub type DataRow = Vec<(String, serde_json::Value)>;

/// The run in progress (its cancel flag), if any.
#[derive(Default)]
pub struct Runs {
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl Runs {
    fn start(&self) -> Result<Active<'_>, AppError> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.is_some() {
            return Err(AppError::Shell("a run is already in progress".into()));
        }
        let flag = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&flag));
        Ok(Active { runs: self, flag })
    }

    /// Stops the run in progress (no-op without one).
    pub fn cancel(&self) {
        if let Some(flag) = &*self.active.lock().unwrap_or_else(PoisonError::into_inner) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Marks the run as finished when dropped (also on errors and panics).
struct Active<'a> {
    runs: &'a Runs,
    flag: Arc<AtomicBool>,
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        *self
            .runs
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The app's runs (one app per process).
static RUNS: std::sync::LazyLock<Runs> = std::sync::LazyLock::new(Runs::default);

/// Runs `plan` on the open collection; `emit` gets every event (body previews only).
pub async fn run<S: SecretStore>(
    core: &Core<S>,
    runs: &Runs,
    plan: RunPlan,
    input: SendInput,
    emit: impl Fn(RunEvent) + Send + 'static,
) -> Result<RunSummary, AppError> {
    let active = runs.start()?;
    // Wave 12, strand D: results of a run of `HEAD`'s files are recorded per commit.
    let recorder = crate::timeline::RunRecorder::for_run(core);
    let emit = move |event: RunEvent| {
        recorder.observe(&event);
        emit(event);
    };
    // Never trusted from the webview, as at send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let (snapshot, id) = core.run_snapshot(input).await?;
    let parts = Arc::clone(&core.services);
    let cancelled = Arc::clone(&active.flag);
    // The core's futures are not `Send`: run on a blocking thread driven by the app's
    // runtime, as sends do.
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        Ok(runtime.block_on(snapshot.run(
            &plan,
            &transport,
            &options,
            &services.services(),
            &mut |event| emit(rt_app::runner::for_view(event)),
            &|| cancelled.load(Ordering::SeqCst),
        )))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the run failed: {e}")))?
}

#[tauri::command]
pub async fn runner_plan(
    core: State<'_, AppCore>,
    folder: Option<String>,
) -> Result<Vec<String>, CommandError> {
    Ok(core.run_plan(folder.as_deref())?)
}

#[tauri::command]
pub async fn runner_parse_data(name: String, data: Vec<u8>) -> Result<Vec<DataRow>, CommandError> {
    Ok(rt_core::runner::parse_data(&name, &data).map_err(AppError::Shell)?)
}

#[tauri::command]
pub async fn runner_run(
    core: State<'_, AppCore>,
    plan: RunPlan,
    input: rt_app::SendInput,
    on_event: Channel<RunEvent>,
) -> Result<RunSummary, CommandError> {
    Ok(run(&core, &RUNS, plan, input, move |event| {
        let _ = on_event.send(event);
    })
    .await?)
}

#[tauri::command]
pub async fn runner_cancel(core: State<'_, AppCore>) -> Result<(), CommandError> {
    let _ = core;
    RUNS.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use rt_app::SecretKey;
    use rt_core::send::ReportBody;

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

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rt-desktop-runner-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `api/` with three requests against `base`; `Local` has a secret `token`.
    fn collection(dir: &Path, base: &str) -> PathBuf {
        let root = dir.join("collection");
        std::fs::create_dir_all(root.join("environments")).unwrap();
        std::fs::create_dir_all(root.join("api")).unwrap();
        let write = |path: &str, text: String| std::fs::write(root.join(path), text).unwrap();
        write(
            "opencollection.yml",
            "opencollection: \"1.0.0\"\ninfo:\n  name: Run\n".into(),
        );
        write(
            "api/folder.yml",
            "info:\n  name: API\n  type: folder\n  seq: 1\n".into(),
        );
        for (seq, name, url) in [
            (1, "first", "{{base}}/echo/get?user={{user}}"),
            (2, "second", "{{base}}/status/{{code}}"),
            (3, "third", "{{base}}/echo/headers?t={{token}}"),
        ] {
            write(
                &format!("api/{name}.yml"),
                format!(
                    "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{url}\"\n"
                ),
            );
        }
        write(
            "other.yml",
            format!(
                "info:\n  name: other\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: {base}/get\n"
            ),
        );
        write(
            "environments/Local.yml",
            format!(
                "name: Local\nvariables:\n  - name: base\n    value: {base}\n  - name: code\n    value: \"200\"\n  - secret: true\n    name: token\n"
            ),
        );
        root
    }

    fn local() -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            // Ignored: the keychain is the only source of secret values.
            secrets: vec![("token".into(), "from-webview".into())],
        }
    }

    #[test]
    fn plan_and_parse_data() {
        let tmp = TempDir::new("plan");
        let root = collection(&tmp.0, "http://127.0.0.1:1");
        let core = Core::new(MemorySecrets::default()).unwrap();
        assert!(core.run_plan(None).is_err(), "no collection open");
        core.open(&root).unwrap();
        assert_eq!(
            core.run_plan(None).unwrap(),
            [
                "api/first.yml",
                "api/second.yml",
                "api/third.yml",
                "other.yml"
            ]
        );
        assert_eq!(
            core.run_plan(Some("api")).unwrap(),
            ["api/first.yml", "api/second.yml", "api/third.yml"]
        );
        let rows = rt_core::runner::parse_data("u.csv", b"user\nada\nbob\n").unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runs_a_folder_with_data_rows_and_keychain_secrets() {
        let addr = rt_testserver::spawn_local().await;
        let tmp = TempDir::new("run");
        let root = collection(&tmp.0, &format!("http://{addr}"));
        let secrets = MemorySecrets::default();
        let core = Core::new(secrets).unwrap();
        let info = core.open(&root).unwrap();
        core.secret_set("Local", "token", "kc-secret").unwrap();
        assert_eq!(info.name, "Run");

        let plan = RunPlan {
            requests: core.run_plan(Some("api")).unwrap(),
            data: rt_core::runner::parse_data("users.csv", b"user\r\nada\r\nbob\r\n").unwrap(),
            iterations: 1,
            delay_ms: 0,
            bail: false,
            throttle: Default::default(),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let runs = Runs::default();
        let summary = run(&core, &runs, plan, local(), move |e| {
            sink.lock().unwrap().push(e)
        })
        .await
        .unwrap();
        assert_eq!(summary.requests, 6, "{summary:?}");
        assert_eq!(summary.passed, 6);
        assert!(!summary.stopped);

        let events = events.lock().unwrap();
        assert!(matches!(events[0], RunEvent::Started { total: 6 }));
        assert!(matches!(events.last(), Some(RunEvent::Finished { .. })));
        let steps: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Step { step } => Some(step),
                _ => None,
            })
            .collect();
        let urls: Vec<_> = steps
            .iter()
            .map(|s| s.report.request.as_ref().unwrap().url.clone())
            .collect();
        let base = format!("http://{addr}");
        assert_eq!(
            urls,
            [
                format!("{base}/echo/get?user=ada"),
                format!("{base}/status/200"),
                // The keychain's value went out, masked in the report.
                format!("{base}/echo/headers?t=***"),
                format!("{base}/echo/get?user=bob"),
                format!("{base}/status/200"),
                format!("{base}/echo/headers?t=***"),
            ]
        );
        assert_eq!(steps[0].report.response.as_ref().unwrap().status, 200);
        // The echo server reports the query it saw: the keychain value, not the webview's.
        let ReportBody::Utf8(body) = &steps[2].report.response.as_ref().unwrap().body else {
            panic!("text body")
        };
        assert!(
            body.contains("kc-secret") && !body.contains("from-webview"),
            "{body}"
        );
        // The run is over: another one may start.
        assert!(runs.start().is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_run_at_a_time_and_cancel_stops_it() {
        let addr = rt_testserver::spawn_local().await;
        let tmp = TempDir::new("cancel");
        let root = collection(&tmp.0, &format!("http://{addr}"));
        let core = Arc::new(Core::new(MemorySecrets::default()).unwrap());
        core.open(&root).unwrap();
        let runs = Arc::new(Runs::default());
        let plan = RunPlan {
            requests: core.run_plan(None).unwrap(),
            iterations: 100,
            delay_ms: 20,
            ..Default::default()
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let task = {
            let core = Arc::clone(&core);
            let runs = Arc::clone(&runs);
            tokio::spawn(async move {
                run(&core, &runs, plan, local(), move |e| {
                    let _ = tx.send(e);
                })
                .await
            })
        };
        // Wait for the first step, then a second run is refused and cancel stops the first.
        while let Some(e) = rx.recv().await {
            if matches!(e, RunEvent::Step { .. }) {
                break;
            }
        }
        let second = run(&core, &runs, RunPlan::default(), local(), |_| {}).await;
        assert!(matches!(second, Err(AppError::Shell(m)) if m.contains("in progress")));
        runs.cancel();
        let summary = task.await.unwrap().unwrap();
        assert!(summary.stopped);
        assert!(summary.requests < 400, "{summary:?}");
        assert!(runs.start().is_ok());
    }
}
