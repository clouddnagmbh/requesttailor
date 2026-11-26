// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs at a revision, run comparison and bisect (`CompareApi`, `rt_app::compare`, `rt_app::bisect`): the collection read with `Repo::tree_at`, sent like the runner (`runner.rs`) with the app's services.
//!
//! Wave 12, strand C. Commands: `compare_run`, `compare_revisions`, `compare_bisect` (each
//! tested commit reported over a Tauri channel) and `compare_cancel`.
//!
//! The requests come from Git's object store (no checkout); the environment, the secret
//! values (keychain and external secrets, never the webview's), cookies, plugins and scripts
//! are today's, as for a send. One compare or bisect at a time (`COMPARES`, cancelled by
//! `compare_cancel` before the next request or commit). Variable changes stay in the run.
//! Each run's test results are recorded under the commit it ran at (`timeline.rs`,
//! `RecordSource::Compare`/`Bisect`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rt_app::bisect::{BisectOutcome, BisectRequest, BisectStep, search, test_revision};
use rt_app::compare::{
    CompareRequest, RevisionRun, RevisionRunRequest, RunComparison,
    compare_revisions as compare_runs, run_revision,
};
use rt_app::git::{GitCommit, GitTree};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use rt_git::{Options, Repo, RepoExt as _};
use tauri::State;
use tauri::ipc::Channel;

use rt_app::timeline::RecordSource;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};
use crate::timeline::{ResultsDir, RevisionRecorder};

/// The compare or bisect in progress (its cancel flag), if any.
#[derive(Default)]
pub struct Compares {
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl Compares {
    fn start(&self) -> Result<Active<'_>, AppError> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.is_some() {
            return Err(AppError::Shell(
                "a compare or bisect is already in progress".into(),
            ));
        }
        let flag = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&flag));
        Ok(Active { owner: self, flag })
    }

    /// Stops the one in progress (no-op without one).
    pub fn cancel(&self) {
        if let Some(flag) = &*self.active.lock().unwrap_or_else(PoisonError::into_inner) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Marks the compare as finished when dropped (also on errors and panics).
struct Active<'a> {
    owner: &'a Compares,
    flag: Arc<AtomicBool>,
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        *self
            .owner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// The app's compares (one app per process).
static COMPARES: std::sync::LazyLock<Compares> = std::sync::LazyLock::new(Compares::default);

fn open_repo(root: Option<std::path::PathBuf>) -> Result<Repo, AppError> {
    let root = root.ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Ok(Repo::require(&root, &Options::default())?)
}

fn recorder(repo: &Repo, results: ResultsDir, source: RecordSource) -> RevisionRecorder {
    RevisionRecorder::new(results, repo.collection_prefix(), source)
}

/// The commit `rev` names, with its tree.
fn at(repo: &Repo, rev: &str) -> Result<(GitCommit, GitTree), AppError> {
    let tree = repo.tree_at(rev)?;
    let commit = repo
        .commits(None, &tree.oid, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Git(format!("{rev:?} does not name a commit")))?;
    Ok((commit, tree))
}

/// Runs `job` on a blocking thread with the open collection's run snapshot, the send
/// services and the native transport (the core's futures are not `Send`).
async fn with_run<S, T, F>(
    core: &Core<S>,
    compares: &Compares,
    input: SendInput,
    job: F,
) -> Result<T, AppError>
where
    S: SecretStore,
    T: Send + 'static,
    F: FnOnce(
            &rt_app::runner::RunSnapshot,
            &NativeTransport,
            &SendOptions,
            &rt_core::services::Services<'_>,
            &dyn Fn() -> bool,
            &tokio::runtime::Handle,
        ) -> Result<T, AppError>
        + Send
        + 'static,
{
    let active = compares.start()?;
    // Never trusted from the webview, as at send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let (snapshot, id) = core.run_snapshot(input).await?;
    let parts = Arc::clone(&core.services);
    let cancelled = Arc::clone(&active.flag);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        job(
            &snapshot,
            &transport,
            &options,
            &services.services(),
            &|| cancelled.load(Ordering::SeqCst),
            &runtime,
        )
    })
    .await
    .map_err(|e| AppError::Shell(format!("the run failed: {e}")))?
}

/// Runs the requests of `request` at one revision.
pub async fn run<S: SecretStore>(
    core: &Core<S>,
    compares: &Compares,
    request: RevisionRunRequest,
    input: SendInput,
) -> Result<RevisionRun, AppError> {
    let root = core.root();
    let results = ResultsDir::new(core.data_dir.as_deref());
    with_run(
        core,
        compares,
        input,
        move |today, transport, options, services, cancelled, runtime| {
            let repo = open_repo(root)?;
            let (commit, tree) = at(&repo, &request.rev)?;
            let run = runtime.block_on(run_revision(
                today,
                commit,
                &tree,
                &request,
                transport,
                options,
                services,
                &mut |_| {},
                cancelled,
            ))?;
            recorder(&repo, results, RecordSource::Compare).record(&run.commit.oid, &run.steps);
            Ok(run)
        },
    )
    .await
}

/// Runs the same requests at two revisions and compares the answers.
pub async fn compare<S: SecretStore>(
    core: &Core<S>,
    compares: &Compares,
    request: CompareRequest,
    input: SendInput,
) -> Result<RunComparison, AppError> {
    let root = core.root();
    let results = ResultsDir::new(core.data_dir.as_deref());
    with_run(
        core,
        compares,
        input,
        move |today, transport, options, services, cancelled, runtime| {
            let repo = open_repo(root)?;
            let record = recorder(&repo, results, RecordSource::Compare);
            let (from_commit, from_tree) = at(&repo, &request.from)?;
            let (to_commit, to_tree) = at(&repo, &request.to)?;
            runtime.block_on(compare_runs(
                today,
                (from_commit, &from_tree),
                (to_commit, &to_tree),
                &request,
                transport,
                options,
                services,
                &mut |commit, run| record.record(&commit.oid, &run.steps),
                cancelled,
            ))
        },
    )
    .await
}

/// Finds the first commit between `good` and `bad` at which something fails; `on_step`
/// gets every tested commit.
pub async fn bisect<S: SecretStore>(
    core: &Core<S>,
    compares: &Compares,
    request: BisectRequest,
    input: SendInput,
    on_step: impl Fn(BisectStep) + Send + 'static,
) -> Result<BisectOutcome, AppError> {
    let root = core.root();
    let results = ResultsDir::new(core.data_dir.as_deref());
    with_run(
        core,
        compares,
        input,
        move |today, transport, options, services, cancelled, runtime| {
            let repo = open_repo(root)?;
            let record = recorder(&repo, results, RecordSource::Bisect);
            let candidates = repo.commits(Some(&request.good), &request.bad, u32::MAX)?;
            let folder = request.folder.as_deref().filter(|f| !f.is_empty());
            Ok(runtime.block_on(search(
                candidates,
                async |commit: &GitCommit| {
                    let tree = repo.tree_at(&commit.oid).map_err(|e| e.to_string());
                    test_revision(
                        today,
                        tree,
                        folder,
                        transport,
                        options,
                        services,
                        &mut |run| record.record(&commit.oid, &run.steps),
                        cancelled,
                    )
                    .await
                },
                &mut |step| on_step(step.clone()),
                cancelled,
            )))
        },
    )
    .await
}

#[tauri::command]
pub async fn compare_run(
    core: State<'_, AppCore>,
    request: RevisionRunRequest,
    input: SendInput,
) -> Result<RevisionRun, CommandError> {
    Ok(run(&core, &COMPARES, request, input).await?)
}

#[tauri::command]
pub async fn compare_revisions(
    core: State<'_, AppCore>,
    request: CompareRequest,
    input: SendInput,
) -> Result<RunComparison, CommandError> {
    Ok(compare(&core, &COMPARES, request, input).await?)
}

#[tauri::command]
pub async fn compare_bisect(
    core: State<'_, AppCore>,
    request: BisectRequest,
    input: SendInput,
    on_step: Channel<BisectStep>,
) -> Result<BisectOutcome, CommandError> {
    Ok(bisect(&core, &COMPARES, request, input, move |step| {
        let _ = on_step.send(step);
    })
    .await?)
}

#[tauri::command]
pub async fn compare_cancel() -> Result<(), CommandError> {
    COMPARES.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Mutex;

    use rt_app::SecretKey;
    use rt_app::bisect::BisectVerdict;
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
                "rt-desktop-compare-{tag}-{}-{:?}",
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

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git is installed");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// `check.yml`: GET `{{base}}/status/<status>` asserting 200, with the secret `token` as a
    /// query value (`/echo/get?t=`) in `echo.yml`.
    fn write_requests(root: &Path, status: u16) {
        let write = |path: &str, text: String| std::fs::write(root.join(path), text).unwrap();
        write(
            "check.yml",
            format!(
                "info:\n  name: Check\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/status/{status}\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n"
            ),
        );
        write(
            "echo.yml",
            "info:\n  name: Echo\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/echo/get?t={{token}}\"\n".into(),
        );
    }

    /// Four commits, the check goes to `/status/500` from the third on. Returns the root and
    /// the commit ids.
    fn repo(tmp: &Path, base: &str) -> (PathBuf, Vec<String>) {
        let root = tmp.join("collection");
        std::fs::create_dir_all(root.join("environments")).unwrap();
        git(&root, &["init", "-q"]);
        std::fs::write(
            root.join("opencollection.yml"),
            "opencollection: \"1.0.0\"\ninfo:\n  name: Cmp\n",
        )
        .unwrap();
        std::fs::write(
            root.join("environments/Local.yml"),
            format!(
                "name: Local\nvariables:\n  - name: base\n    value: {base}\n  - secret: true\n    name: token\n"
            ),
        )
        .unwrap();
        let mut ids = Vec::new();
        for i in 1..=4 {
            write_requests(&root, if i >= 3 { 500 } else { 200 });
            std::fs::write(root.join("notes.txt"), format!("{i}\n")).unwrap();
            git(&root, &["add", "-A"]);
            git(&root, &["commit", "-q", "-m", &format!("commit {i}")]);
            ids.push(git(&root, &["rev-parse", "HEAD"]));
        }
        (root, ids)
    }

    fn local() -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            // Ignored: the keychain is the only source of secret values.
            secrets: vec![("token".into(), "from-webview".into())],
        }
    }

    async fn opened(tag: &str) -> (TempDir, Core<MemorySecrets>, Vec<String>) {
        let addr = rt_testserver::spawn_local().await;
        let tmp = TempDir::new(tag);
        let (root, ids) = repo(&tmp.0, &format!("http://{addr}"));
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&root).unwrap();
        core.secret_set("Local", "token", "kc-secret").unwrap();
        (tmp, core, ids)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runs_a_revision_without_a_checkout_with_keychain_secrets() {
        let (_tmp, core, ids) = opened("run").await;
        let compares = Compares::default();
        let request = |rev: &str| RevisionRunRequest {
            rev: rev.into(),
            folder: None,
            paths: None,
        };
        let good = run(&core, &compares, request(&ids[0]), local())
            .await
            .unwrap();
        assert_eq!(good.commit.oid, ids[0]);
        assert_eq!(good.commit.summary, "commit 1");
        assert_eq!((good.summary.requests, good.summary.passed), (2, 2));
        // The keychain's value went out, not the webview's; the report masks it.
        assert!(
            good.steps[1]
                .report
                .request
                .as_ref()
                .unwrap()
                .url
                .ends_with("/echo/get?t=***")
        );
        let ReportBody::Utf8(body) = &good.steps[1].report.response.as_ref().unwrap().body else {
            panic!("text body")
        };
        assert!(body.contains("kc-secret") && !body.contains("from-webview"));
        // `HEAD` is commit 4: the assertion fails there. The working folder was not touched.
        let bad = run(&core, &compares, request("HEAD"), local())
            .await
            .unwrap();
        assert_eq!(bad.summary.failed, 1, "{:?}", bad.summary);
        assert_eq!(bad.summary.assertions_failed, 1);
        let on_disk = std::fs::read_to_string(core.root().unwrap().join("check.yml")).unwrap();
        assert!(on_disk.contains("/status/500"));
        // An unknown revision is an error, and the next run may start.
        assert!(
            run(&core, &compares, request("nope"), local())
                .await
                .is_err()
        );
        assert!(compares.start().is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn compares_two_revisions() {
        let (_tmp, core, ids) = opened("compare").await;
        let cmp = compare(
            &core,
            &Compares::default(),
            CompareRequest {
                from: ids[0].clone(),
                to: "main".into(),
                folder: None,
                paths: None,
            },
            local(),
        )
        .await
        .unwrap();
        assert_eq!(cmp.from.oid, ids[0]);
        assert_eq!(cmp.to.oid, ids[3]);
        let check = cmp.requests.iter().find(|r| r.path == "check.yml").unwrap();
        let status = check.status.as_ref().unwrap();
        assert_eq!((status.old, status.new), (Some(200), Some(500)));
        assert_eq!(check.tests.len(), 1);
        assert!(
            cmp.requests
                .iter()
                .find(|r| r.path == "echo.yml")
                .unwrap()
                .same
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bisects_and_reports_each_step() {
        let (_tmp, core, ids) = opened("bisect").await;
        let steps = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&steps);
        let outcome = bisect(
            &core,
            &Compares::default(),
            BisectRequest {
                good: ids[0].clone(),
                bad: "HEAD".into(),
                folder: None,
            },
            local(),
            move |s| sink.lock().unwrap().push(s),
        )
        .await
        .unwrap();
        assert_eq!(outcome.first_bad.unwrap().oid, ids[2]);
        assert_eq!(outcome.candidates, 3);
        let steps = steps.lock().unwrap();
        assert_eq!(*steps, outcome.steps);
        assert_eq!(steps[0].commit.oid, ids[3]);
        assert_eq!(steps[0].verdict, BisectVerdict::Bad);
        assert!(
            steps[0].failed[0].starts_with("Check"),
            "{:?}",
            steps[0].failed
        );
    }

    /// Runs at a revision go to the time travel under the commit they ran at: compare and
    /// bisect with their own source, nothing in the repository.
    #[tokio::test(flavor = "multi_thread")]
    async fn records_the_runs_under_the_commits_they_ran_at() {
        let addr = rt_testserver::spawn_local().await;
        let tmp = TempDir::new("record");
        let (root, ids) = repo(&tmp.0, &format!("http://{addr}"));
        let data = tmp.0.join("data");
        let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
        core.open(&root).unwrap();
        core.secret_set("Local", "token", "kc-secret").unwrap();
        let results = ResultsDir::new(Some(&data));
        let compares = Compares::default();

        run(
            &core,
            &compares,
            RevisionRunRequest {
                rev: ids[0].clone(),
                folder: None,
                paths: None,
            },
            local(),
        )
        .await
        .unwrap();
        let first = results.read(&ids[0]);
        assert_eq!(first.len(), 2, "{first:?}");
        assert!(
            first
                .iter()
                .all(|r| r.source == RecordSource::Compare && r.passed)
        );

        compare(
            &core,
            &compares,
            CompareRequest {
                from: ids[1].clone(),
                to: ids[3].clone(),
                folder: None,
                paths: None,
            },
            local(),
        )
        .await
        .unwrap();
        assert_eq!(results.read(&ids[1]).len(), 2);
        let last = results.read(&ids[3]);
        let check = last.iter().find(|r| r.request == "check.yml").unwrap();
        assert_eq!((check.passed, check.status), (false, Some(500)));

        let outcome = bisect(
            &core,
            &compares,
            BisectRequest {
                good: ids[0].clone(),
                bad: "HEAD".into(),
                folder: None,
            },
            local(),
            |_| {},
        )
        .await
        .unwrap();
        for step in &outcome.steps {
            let records = results.read(&step.commit.oid);
            assert!(
                records.iter().any(|r| r.source == RecordSource::Bisect),
                "{records:?}"
            );
        }
        // No secret value in a record, nothing in the repository.
        let all =
            std::fs::read_to_string(data.join(rt_app::timeline::results_file(&ids[3]))).unwrap();
        assert!(!all.contains("kc-secret"));
        assert_eq!(git(&root, &["status", "--porcelain"]), "");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_at_a_time_and_cancel_stops_a_bisect() {
        let (_tmp, core, ids) = opened("cancel").await;
        let core = std::sync::Arc::new(core);
        let compares = std::sync::Arc::new(Compares::default());
        // The first step is reported, then the search waits until the test has looked around.
        let (step_tx, mut step_rx) = tokio::sync::mpsc::unbounded_channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let task = {
            let (core, compares, ids) = (
                std::sync::Arc::clone(&core),
                std::sync::Arc::clone(&compares),
                ids.clone(),
            );
            tokio::spawn(async move {
                bisect(
                    &core,
                    &compares,
                    BisectRequest {
                        good: ids[0].clone(),
                        bad: "HEAD".into(),
                        folder: None,
                    },
                    local(),
                    move |s| {
                        let _ = step_tx.send(s);
                        let _ = go_rx.recv();
                    },
                )
                .await
            })
        };
        step_rx.recv().await.unwrap();
        let second = run(
            &core,
            &compares,
            RevisionRunRequest {
                rev: "HEAD".into(),
                folder: None,
                paths: None,
            },
            local(),
        )
        .await;
        assert!(matches!(second, Err(AppError::Shell(m)) if m.contains("in progress")));
        compares.cancel();
        go_tx.send(()).unwrap();
        let outcome = task.await.unwrap().unwrap();
        assert!(outcome.first_bad.is_none(), "{outcome:?}");
        assert_eq!(outcome.steps.len(), 1);
        assert!(compares.start().is_ok());
    }
}
