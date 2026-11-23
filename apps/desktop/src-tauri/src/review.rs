// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Pull-request review (`ReviewApi`, `rt_app::review`): tokens in the keychain (`rt_app::review::token_slot`), providers over the app's transport, PR heads fetched with `rt_git::review`.
//!
//! Wave 12, strand E. Commands: `review_hosts`, `review_set_hosts`, `review_repo`,
//! `review_has_token`, `review_set_token`, `review_delete_token`, `review_start_login`,
//! `review_finish_login`, `review_list`, `review_open`, `review_comments`, `review_comment`,
//! `review_submit`, `review_ci`.
//!
//! - The host is detected from the open collection's `origin`; self-hosted hosts are in
//!   `review/hosts.json` in the data folder (per device, never in the collection).
//! - The token lives in the keychain under `token_slot(host)`; the webview can set and
//!   delete it and ask whether one exists, never read it back. It is sent as the
//!   `Authorization` header (API) or the HTTPS password (fetching the PR head), nowhere else.
//! - Paths the webview sees are relative to the collection; the host's are relative to the
//!   repository, so this file converts both ways.
//! - Every call runs on a blocking thread with the native transport, like the AI assistant
//!   (the core's futures are not `Send`).

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use rt_app::auth::DeviceCodePrompt;
use rt_app::review::device::{self, DeviceLogin, DevicePoll};
use rt_app::review::{
    self, CiStatus, NewPullRequest, NewReviewComment, PullRequest, ReviewComment, ReviewError,
    ReviewHost, ReviewProvider, ReviewRepo, ReviewSession, ReviewSubmission, token_slot,
};
use rt_app::{AppError, SecretStore};
use rt_core::transport::native::NativeTransport;
use rt_git::review::TokenCredentials;
use rt_git::{Options, Repo};
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

const HOSTS_FILE: &str = "hosts.json";

fn review_error(e: ReviewError) -> CommandError {
    let kind = serde_json::to_value(&e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    CommandError {
        kind,
        message: e.to_string(),
    }
}

/// What the commands fail with: a Git host problem or an app problem.
pub enum Failure {
    Review(ReviewError),
    App(AppError),
}

impl From<ReviewError> for Failure {
    fn from(e: ReviewError) -> Self {
        Failure::Review(e)
    }
}

impl From<AppError> for Failure {
    fn from(e: AppError) -> Self {
        Failure::App(e)
    }
}

impl From<Failure> for CommandError {
    fn from(f: Failure) -> Self {
        match f {
            Failure::Review(e) => review_error(e),
            Failure::App(e) => e.into(),
        }
    }
}

type Result<T> = std::result::Result<T, Failure>;

fn shell(e: impl std::fmt::Display) -> AppError {
    AppError::Shell(e.to_string())
}

// ---- self-hosted hosts, per device -----------------------------------------------------

/// Without a data folder (tests) the hosts live in memory.
fn memory() -> &'static Mutex<Vec<ReviewHost>> {
    static MEMORY: LazyLock<Mutex<Vec<ReviewHost>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    &MEMORY
}

fn hosts_path(data_dir: &Path) -> PathBuf {
    data_dir.join("review").join(HOSTS_FILE)
}

/// The saved hosts (an unreadable file counts as none).
pub fn load_hosts(data_dir: Option<&Path>) -> Vec<ReviewHost> {
    match data_dir {
        Some(dir) => std::fs::read(hosts_path(dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default(),
        None => memory()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    }
}

/// Saves `hosts` (checked and normalised first).
pub fn save_hosts(data_dir: Option<&Path>, hosts: &[ReviewHost]) -> Result<()> {
    let hosts = review::normalize_hosts(hosts)?;
    let Some(dir) = data_dir else {
        *memory().lock().unwrap_or_else(PoisonError::into_inner) = hosts;
        return Ok(());
    };
    let path = hosts_path(dir);
    let io = |e: std::io::Error| shell(format!("cannot save the Git hosts: {e}"));
    std::fs::create_dir_all(path.parent().unwrap_or(dir)).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&hosts).unwrap_or_default()).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)?;
    Ok(())
}

// ---- the repository and its token ------------------------------------------------------

fn open_repo<S: SecretStore>(core: &Core<S>) -> std::result::Result<Option<Repo>, AppError> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Repo::open(&root, &Options::default())
}

/// The Git host and project of the open collection's `origin`.
pub fn repo<S: SecretStore>(core: &Core<S>) -> Result<Option<ReviewRepo>> {
    let Some(git) = open_repo(core)? else {
        return Ok(None);
    };
    let hosts = load_hosts(core.data_dir.as_deref());
    Ok(git
        .origin_url()
        .and_then(|url| review::detect(&url, &hosts)))
}

/// The collection's folder and its review repository.
fn require_repo<S: SecretStore>(core: &Core<S>) -> Result<(PathBuf, ReviewRepo)> {
    let git = open_repo(core)?.ok_or_else(|| {
        ReviewError::NotFound("this collection is not in a Git repository".into())
    })?;
    let hosts = load_hosts(core.data_dir.as_deref());
    let repo = git
        .origin_url()
        .and_then(|url| review::detect(&url, &hosts))
        .ok_or_else(|| {
            ReviewError::NotFound(
                "the origin of this collection is not on a Git host the app knows; add the host under Git hosts"
                    .into(),
            )
        })?;
    Ok((git.root().to_path_buf(), repo))
}

fn token<S: SecretStore>(core: &Core<S>, host: &str) -> Result<String> {
    match core.secrets().get(&token_slot(host))? {
        Some(t) if !t.trim().is_empty() => Ok(t),
        _ => Err(ReviewError::NoToken(host.to_owned()).into()),
    }
}

pub fn has_token<S: SecretStore>(core: &Core<S>, host: &str) -> Result<bool> {
    Ok(core
        .secrets()
        .get(&token_slot(host))?
        .is_some_and(|t| !t.trim().is_empty()))
}

pub fn set_token<S: SecretStore>(core: &Core<S>, host: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        core.secrets().delete(&token_slot(host))?;
    } else {
        core.secrets().set(&token_slot(host), value)?;
    }
    Ok(())
}

// ---- paths: collection-relative for the webview, repository-relative for the host -------

/// The collection's folder within the repository (`""` at the root, else `sub/dir/`).
fn prefix(git: &Repo) -> String {
    match git.root().strip_prefix(git.workdir()) {
        Ok(rel) if !rel.as_os_str().is_empty() => {
            format!("{}/", rel.to_string_lossy().replace('\\', "/"))
        }
        _ => String::new(),
    }
}

// ---- calls ---------------------------------------------------------------------------------

/// Runs `job` with the provider of the open collection's host on a blocking thread.
async fn with_provider<S, T, F>(core: &Core<S>, job: F) -> Result<T>
where
    S: SecretStore,
    T: Send + 'static,
    F: for<'a> FnOnce(
            &'a dyn ReviewProvider,
            &'a Repo,
            &'a str,
            &'a tokio::runtime::Handle,
        ) -> std::result::Result<T, Failure>
        + Send
        + 'static,
{
    let (root, repo) = require_repo(core)?;
    let token = token(core, &repo.host)?;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        // Git handles are not `Send`: opened on this thread.
        let git = Repo::require(&root, &Options::default())?;
        let transport =
            NativeTransport::new().map_err(|e| ReviewError::Transport(e.to_string()))?;
        let provider = review::provider(&transport, &repo, &token);
        job(&*provider, &git, &token, &runtime)
    })
    .await
    .map_err(|e| shell(format!("the review call failed: {e}")))?
}

pub async fn list<S: SecretStore>(core: &Core<S>) -> Result<Vec<PullRequest>> {
    with_provider(core, |p, _, _, rt| Ok(rt.block_on(p.list())?)).await
}

/// Fetches the head of the pull request and the target branch (no checkout).
pub async fn open<S: SecretStore>(core: &Core<S>, number: u64) -> Result<ReviewSession> {
    with_provider(core, move |p, git, token, rt| {
        let pull = rt.block_on(p.get(number))?;
        let repo = p.repo();
        let credentials = TokenCredentials {
            host: &repo.host,
            kind: repo.kind,
            token,
        };
        let fetched = git.fetch_review(
            repo.kind,
            number,
            &p.head_ref(number),
            &pull.target,
            &credentials,
        )?;
        Ok(ReviewSession {
            pull,
            head_ref: fetched.head_ref,
            merge_base: fetched.merge_base,
        })
    })
    .await
}

pub async fn comments<S: SecretStore>(core: &Core<S>, number: u64) -> Result<Vec<ReviewComment>> {
    with_provider(core, move |p, git, _, rt| {
        let prefix = prefix(git);
        let mut comments = rt.block_on(p.comments(number))?;
        for c in &mut comments {
            if let Some(path) = c
                .path
                .as_deref()
                .and_then(|p| p.strip_prefix(prefix.as_str()))
            {
                c.path = Some(path.to_owned());
            }
        }
        Ok(comments)
    })
    .await
}

pub async fn comment<S: SecretStore>(
    core: &Core<S>,
    number: u64,
    comment: NewReviewComment,
) -> Result<ReviewComment> {
    with_provider(core, move |p, git, _, rt| {
        let prefix = prefix(git);
        let pull = rt.block_on(p.get(number))?;
        let host_side = NewReviewComment {
            path: format!("{prefix}{}", comment.path),
            ..comment
        };
        let mut made = rt.block_on(p.comment(&pull, &host_side))?;
        if let Some(path) = made
            .path
            .as_deref()
            .and_then(|p| p.strip_prefix(prefix.as_str()))
        {
            made.path = Some(path.to_owned());
        }
        Ok(made)
    })
    .await
}

pub async fn submit<S: SecretStore>(
    core: &Core<S>,
    number: u64,
    review: ReviewSubmission,
) -> Result<()> {
    with_provider(core, move |p, _, _, rt| {
        let pull = rt.block_on(p.get(number))?;
        Ok(rt.block_on(p.submit(&pull, &review))?)
    })
    .await
}

pub async fn ci<S: SecretStore>(core: &Core<S>, number: u64) -> Result<CiStatus> {
    with_provider(core, move |p, _, _, rt| {
        let pull = rt.block_on(p.get(number))?;
        Ok(rt.block_on(p.ci(&pull))?)
    })
    .await
}

/// Wave 15: opens a pull request from a branch on the remote (the provider's `create`; strand E
/// implements it for GitHub and GitLab).
pub async fn create<S: SecretStore>(core: &Core<S>, pull: NewPullRequest) -> Result<PullRequest> {
    with_provider(core, move |p, _, _, rt| Ok(rt.block_on(p.create(&pull))?)).await
}

// ---- device-code login -------------------------------------------------------------------

/// Logins started and not finished, by host.
static LOGINS: LazyLock<Mutex<Vec<(String, DeviceLogin)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

pub async fn start_login<S: SecretStore>(core: &Core<S>, host: &str) -> Result<DeviceCodePrompt> {
    let (_, repo) = require_repo(core)?;
    if !repo.host.eq_ignore_ascii_case(host) {
        return Err(
            ReviewError::NotFound(format!("{host} is not this collection's Git host")).into(),
        );
    }
    let client_id = device::client_id(&repo).ok_or_else(|| {
        AppError::Unsupported(
            "Signing in with a code is not available for this host; use a personal access token."
                .into(),
        )
    })?;
    let login = tokio::task::spawn_blocking(move || {
        let transport =
            NativeTransport::new().map_err(|e| ReviewError::Transport(e.to_string()))?;
        tokio::runtime::Handle::current().block_on(device::start(&transport, &repo, client_id))
    })
    .await
    .map_err(|e| shell(format!("the login failed: {e}")))??;
    let prompt = login.prompt.clone();
    let mut logins = LOGINS.lock().unwrap_or_else(PoisonError::into_inner);
    logins.retain(|(h, _)| h != host);
    logins.push((host.to_owned(), login));
    Ok(prompt)
}

pub async fn finish_login<S: SecretStore>(core: &Core<S>, host: &str) -> Result<()> {
    let (_, repo) = require_repo(core)?;
    let login = {
        let mut logins = LOGINS.lock().unwrap_or_else(PoisonError::into_inner);
        let at = logins
            .iter()
            .position(|(h, _)| h == host)
            .ok_or_else(|| AppError::Shell("start the login first".into()))?;
        logins.remove(at).1
    };
    let client_id = device::client_id(&repo)
        .ok_or_else(|| AppError::Unsupported("device-code login is not available".into()))?;
    let runtime = tokio::runtime::Handle::current();
    let token = tokio::task::spawn_blocking(move || -> std::result::Result<String, ReviewError> {
        let transport =
            NativeTransport::new().map_err(|e| ReviewError::Transport(e.to_string()))?;
        let mut waited = 0u64;
        let mut interval = login.interval;
        loop {
            if waited > login.prompt.expires_in {
                return Err(ReviewError::Auth(
                    "the code expired; start the login again".into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_secs(interval));
            waited += interval;
            match runtime.block_on(device::poll(&transport, &repo, client_id, &login))? {
                DevicePoll::Token(t) => return Ok(t),
                DevicePoll::Pending => {}
                DevicePoll::SlowDown(next) => interval = next,
            }
        }
    })
    .await
    .map_err(|e| shell(format!("the login failed: {e}")))??;
    set_token(core, host, &token)
}

// ---- Tauri commands ----------------------------------------------------------------------

#[tauri::command]
pub async fn review_hosts(
    core: State<'_, AppCore>,
) -> std::result::Result<Vec<ReviewHost>, CommandError> {
    Ok(load_hosts(core.data_dir.as_deref()))
}

#[tauri::command]
pub async fn review_set_hosts(
    core: State<'_, AppCore>,
    hosts: Vec<ReviewHost>,
) -> std::result::Result<(), CommandError> {
    Ok(save_hosts(core.data_dir.as_deref(), &hosts)?)
}

#[tauri::command]
pub async fn review_repo(
    core: State<'_, AppCore>,
) -> std::result::Result<Option<ReviewRepo>, CommandError> {
    Ok(repo(&core)?)
}

#[tauri::command]
pub async fn review_has_token(
    core: State<'_, AppCore>,
    host: String,
) -> std::result::Result<bool, CommandError> {
    Ok(has_token(&core, &host)?)
}

#[tauri::command]
pub async fn review_set_token(
    core: State<'_, AppCore>,
    host: String,
    token: String,
) -> std::result::Result<(), CommandError> {
    Ok(set_token(&core, &host, &token)?)
}

#[tauri::command]
pub async fn review_delete_token(
    core: State<'_, AppCore>,
    host: String,
) -> std::result::Result<(), CommandError> {
    Ok(set_token(&core, &host, "")?)
}

#[tauri::command]
pub async fn review_start_login(
    core: State<'_, AppCore>,
    host: String,
) -> std::result::Result<DeviceCodePrompt, CommandError> {
    Ok(start_login(&core, &host).await?)
}

#[tauri::command]
pub async fn review_finish_login(
    core: State<'_, AppCore>,
    host: String,
) -> std::result::Result<(), CommandError> {
    Ok(finish_login(&core, &host).await?)
}

#[tauri::command]
pub async fn review_list(
    core: State<'_, AppCore>,
) -> std::result::Result<Vec<PullRequest>, CommandError> {
    Ok(list(&core).await?)
}

#[tauri::command]
pub async fn review_open(
    core: State<'_, AppCore>,
    number: u64,
) -> std::result::Result<ReviewSession, CommandError> {
    Ok(open(&core, number).await?)
}

#[tauri::command]
pub async fn review_comments(
    core: State<'_, AppCore>,
    number: u64,
) -> std::result::Result<Vec<ReviewComment>, CommandError> {
    Ok(comments(&core, number).await?)
}

#[tauri::command]
pub async fn review_comment(
    core: State<'_, AppCore>,
    number: u64,
    comment: NewReviewComment,
) -> std::result::Result<ReviewComment, CommandError> {
    Ok(self::comment(&core, number, comment).await?)
}

#[tauri::command]
pub async fn review_submit(
    core: State<'_, AppCore>,
    number: u64,
    review: ReviewSubmission,
) -> std::result::Result<(), CommandError> {
    Ok(submit(&core, number, review).await?)
}

#[tauri::command]
pub async fn review_ci(
    core: State<'_, AppCore>,
    number: u64,
) -> std::result::Result<CiStatus, CommandError> {
    Ok(ci(&core, number).await?)
}

#[tauri::command]
pub async fn review_create(
    core: State<'_, AppCore>,
    pull: NewPullRequest,
) -> std::result::Result<PullRequest, CommandError> {
    Ok(create(&core, pull).await?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, ChildStdin, Command, Stdio};

    use rt_app::compare::RevisionRunRequest;
    use rt_app::review::{
        CiState, PullState, ReviewKind, ReviewVerdict, review_base_ref, review_ref,
    };
    use rt_app::{SecretKey, SendInput};
    use rt_format::diff::{ChangeKind, ChangeTarget};
    use serde_json::Value;

    use super::*;
    use crate::compare::{Compares, run as run_revision};

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &SecretKey) -> std::result::Result<Option<String>, AppError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> std::result::Result<(), AppError> {
            self.0.lock().unwrap().insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> std::result::Result<(), AppError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// The fake Git host (`testdata/review/fake-host.mjs`), a child process.
    struct FakeHost {
        child: Child,
        stdin: Option<ChildStdin>,
        info: Value,
    }

    impl FakeHost {
        fn start() -> Self {
            let script = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../testdata/review/fake-host.mjs"
            );
            let mut child = Command::new("node")
                .arg(script)
                .args(["--github-port", "0", "--gitlab-port", "0"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("node is needed for the fake Git host");
            let stdin = child.stdin.take();
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            let json = line
                .trim()
                .strip_prefix("READY ")
                .expect("fake host did not start");
            Self {
                child,
                stdin,
                info: serde_json::from_str(json).unwrap(),
            }
        }

        fn port(&self, kind: ReviewKind) -> u64 {
            match kind {
                ReviewKind::Github => self.info["github"].as_u64().unwrap(),
                ReviewKind::Gitlab => self.info["gitlab"].as_u64().unwrap(),
            }
        }

        fn review_host(&self, kind: ReviewKind) -> ReviewHost {
            let port = self.port(kind);
            ReviewHost {
                host: format!("127.0.0.1:{port}"),
                kind,
                api: format!(
                    "http://127.0.0.1:{port}/{}",
                    if kind == ReviewKind::Github {
                        "api/v3"
                    } else {
                        "api/v4"
                    }
                ),
            }
        }

        fn token(&self) -> String {
            self.info["token"].as_str().unwrap().to_owned()
        }
    }

    impl Drop for FakeHost {
        fn drop(&mut self) {
            drop(self.stdin.take());
            let _ = self.child.wait();
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rt-desktop-review-{tag}-{}-{:?}",
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

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git is installed");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The collection a reviewer has: a clone of the fake host's repository, with their own
    /// (uncommitted) environment pointing at `base`.
    fn clone(tmp: &Path, host: &FakeHost, kind: ReviewKind, base: &str) -> PathBuf {
        let dir = tmp.join(kind.id());
        let url = format!("http://127.0.0.1:{}/acme/orders.git", host.port(kind));
        git(tmp, &["clone", "-q", &url, kind.id()]);
        std::fs::write(
            dir.join("environments/Local.yml"),
            format!("name: Local\nvariables:\n  - name: baseUrl\n    value: {base}\n  - name: tenant\n    value: acme\n"),
        )
        .unwrap();
        dir
    }

    fn local() -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            secrets: Vec::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reviews_a_pull_request_end_to_end_on_both_hosts() {
        let host = FakeHost::start();
        let echo = rt_testserver::spawn_local().await;
        let tmp = TempDir::new("e2e");
        let hosts = [
            host.review_host(ReviewKind::Github),
            host.review_host(ReviewKind::Gitlab),
        ];
        save_hosts(None, &hosts).map_err(|_| ()).unwrap();

        for kind in [ReviewKind::Github, ReviewKind::Gitlab] {
            let root = clone(&tmp.0, &host, kind, &format!("http://{echo}/echo"));
            let core = Core::new(MemorySecrets::default()).unwrap();
            core.open(&root).unwrap();
            let slot = hosts.iter().find(|h| h.kind == kind).unwrap().host.clone();

            // The host is found from `origin`; without a token every call says so.
            let found = repo(&core).map_err(|_| ()).unwrap().expect("a known host");
            assert_eq!((found.kind, found.project.as_str()), (kind, "acme/orders"));
            assert_eq!(found.host, slot);
            assert!(!has_token(&core, &slot).map_err(|_| ()).unwrap());
            let Err(Failure::Review(ReviewError::NoToken(h))) = list(&core).await else {
                panic!("expected no-token")
            };
            assert_eq!(h, slot);

            // A wrong token is an auth error that does not show the token.
            set_token(&core, &slot, "wrong-token-VALUE-123")
                .map_err(|_| ())
                .unwrap();
            let Err(Failure::Review(e)) = list(&core).await else {
                panic!("expected an error")
            };
            assert!(matches!(e, ReviewError::Auth(_)), "{e:?}");
            assert!(!e.to_string().contains("wrong-token-VALUE-123"));

            // The right one: list, open.
            set_token(&core, &slot, &host.token())
                .map_err(|_| ())
                .unwrap();
            assert!(has_token(&core, &slot).map_err(|_| ()).unwrap());
            let pulls = list(&core).await.map_err(|_| ()).unwrap();
            assert_eq!(pulls.iter().map(|p| p.number).collect::<Vec<_>>(), [1, 2]);
            assert_eq!(pulls[0].state, PullState::Open);

            let session = open(&core, 1).await.map_err(|_| ()).unwrap();
            assert_eq!(session.head_ref, review_ref(kind, 1));
            assert_eq!(session.pull.head, host.info["head"].as_str().unwrap());
            assert_eq!(session.merge_base, host.info["base"].as_str().unwrap());
            // Fetched without a checkout: the working folder shows the user's `main`.
            assert!(!root.join("orders/cancel-order.yml").exists());
            let repo_ = Repo::require(&root, &Options::default()).unwrap();
            assert_eq!(
                repo_.rev_parse(&review_ref(kind, 1)).unwrap(),
                session.pull.head
            );
            assert_eq!(
                repo_.rev_parse(&review_base_ref(kind, 1)).unwrap(),
                session.merge_base
            );

            // The semantic diff of the pull request: two changed requests, one new one.
            let diff = crate::diffs::between(&core, &session.merge_base, &session.head_ref)
                .map_err(|_| ())
                .unwrap();
            let request = |path: &str| {
                diff.changes
                    .iter()
                    .find(|c| matches!(&c.target, ChangeTarget::Request { path: p } if p == path))
                    .unwrap_or_else(|| panic!("no change for {path}"))
            };
            assert_eq!(
                request("orders/list-orders.yml").change,
                ChangeKind::Modified
            );
            assert_eq!(
                request("orders/create-order.yml").change,
                ChangeKind::Modified
            );
            assert_eq!(request("orders/cancel-order.yml").change, ChangeKind::Added);

            // The changed requests run at the head against the reviewer's own environment.
            let paths: Vec<String> = [
                "orders/list-orders.yml",
                "orders/create-order.yml",
                "orders/cancel-order.yml",
            ]
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
            let run = run_revision(
                &core,
                &Compares::default(),
                RevisionRunRequest {
                    rev: session.head_ref.clone(),
                    folder: None,
                    paths: Some(paths),
                },
                local(),
            )
            .await
            .map_err(|_| ())
            .unwrap();
            assert_eq!(run.commit.oid, session.pull.head);
            assert_eq!(run.steps.len(), 3);
            for step in &run.steps {
                let report = &step.report;
                let url = &report.request.as_ref().expect("sent").url;
                assert!(
                    url.starts_with(&format!("http://{echo}/echo/orders")),
                    "{url}"
                );
                assert!(report.response.is_some(), "{url}: {:?}", report.error);
            }
            // The list at the head asks for 25, as the pull request changed it.
            assert!(
                run.steps[0]
                    .report
                    .request
                    .as_ref()
                    .unwrap()
                    .url
                    .ends_with("limit=25")
            );

            // Comments: paths are relative to the collection, comments on lines work.
            let comments = comments(&core, 1).await.map_err(|_| ()).unwrap();
            let line = comments
                .iter()
                .find(|c| c.path.is_some() && !c.outdated)
                .unwrap();
            assert_eq!(line.path.as_deref(), Some("orders/create-order.yml"));
            let made = comment(
                &core,
                1,
                NewReviewComment {
                    path: "orders/list-orders.yml".into(),
                    line: 12,
                    body: "Why 25?".into(),
                },
            )
            .await
            .map_err(|_| ())
            .unwrap();
            assert_eq!(made.path.as_deref(), Some("orders/list-orders.yml"));
            assert_eq!(made.line, Some(12));
            let Err(Failure::Review(bad)) = comment(
                &core,
                1,
                NewReviewComment {
                    path: "orders/nope.yml".into(),
                    line: 1,
                    body: "x".into(),
                },
            )
            .await
            else {
                panic!("expected a refusal")
            };
            assert!(matches!(bad, ReviewError::Host(_)), "{bad:?}");

            submit(
                &core,
                1,
                ReviewSubmission {
                    verdict: ReviewVerdict::Approve,
                    body: "LGTM".into(),
                },
            )
            .await
            .map_err(|_| ())
            .unwrap();
            let status = ci(&core, 1).await.map_err(|_| ()).unwrap();
            assert_eq!(status.state, CiState::Failure);
            assert!(
                status
                    .checks
                    .iter()
                    .any(|c| c.name == "build" && c.state == CiState::Success)
            );

            // An unknown PR is not-found, not a crash.
            let Err(Failure::Review(e)) = open(&core, 99).await else {
                panic!("expected an error")
            };
            assert!(matches!(e, ReviewError::NotFound(_)), "{e:?}");

            // The token is in the keychain stand-in only: never in the repository's config.
            let config = std::fs::read_to_string(root.join(".git/config")).unwrap();
            assert!(!config.contains(&host.token()));
        }
    }

    #[test]
    fn hosts_are_per_device_and_checked() {
        let dir = TempDir::new("hosts");
        let ok = ReviewHost {
            host: "Git.Example.com".into(),
            kind: ReviewKind::Github,
            api: "https://git.example.com/api/v3/".into(),
        };
        save_hosts(Some(&dir.0), &[ok]).map_err(|_| ()).unwrap();
        let loaded = load_hosts(Some(&dir.0));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].host, "git.example.com");
        assert_eq!(loaded[0].api, "https://git.example.com/api/v3");
        assert!(dir.0.join("review/hosts.json").exists());
        let bad = ReviewHost {
            host: "x".into(),
            kind: ReviewKind::Gitlab,
            api: "nope".into(),
        };
        assert!(save_hosts(Some(&dir.0), &[bad]).is_err());
        assert_eq!(
            load_hosts(Some(&dir.0)).len(),
            1,
            "a refused list changes nothing"
        );
    }
}
