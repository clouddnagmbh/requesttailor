// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the app (wave 5, strand C): the cached tokens as the UI lists them (never
//! their values) and the prompts the UI shows while a flow runs.
//!
//! Tokens are secrets: natively in the OS keychain, in the web in the encrypted browser
//! store, under `SecretKey { collection, environment: "oauth2", variable: <cache key> }`.
//! Changes here are interface changes.
//!
//! - [`SecretTokens`]: `rt_core::services::TokenStore` over any [`SecretStore`] (the desktop
//!   app and `rt send` use the keychain, so both share the tokens of a collection). The keys
//!   of a collection's tokens are kept in one more entry (`variable: "index"`), because a
//!   keychain cannot be listed.
//! - [`fetch_token`]: "Get token" in the UI, for every shell.
//! - [`loopback`] (native): the system browser and the redirect listener for the
//!   authorization code flow, shared by the desktop app and the CLI.

use std::sync::{Arc, Mutex};

use rt_core::model::Auth;
use rt_core::prepare::{FileSource, PrepareInput, prepare};
use rt_core::services::{LocalBoxFuture, Services, StoredToken, TokenStore};
use rt_core::transport::Transport;
use serde::{Deserialize, Serialize};

pub use rt_core::services::DeviceCodePrompt;

use crate::{AppError, SecretKey, SecretStore, SendSnapshot};

/// A cached token, without its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenInfo {
    /// `rt_core::auth::oauth2::cache_key`.
    pub key: String,
    /// Token endpoint host, client id and scope, for display.
    pub label: String,
    /// Unix milliseconds.
    pub obtained_at: i64,
    pub expires_at: Option<i64>,
    pub refreshable: bool,
}

/// What the UI shows while a flow waits for the user. Pushed by the shell
/// (`Backend.auth.onPrompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AuthPrompt {
    /// The system browser (desktop, CLI) or a popup (web) shows the provider's login.
    Browser {
        url: String,
    },
    DeviceCode(DeviceCodePrompt),
    /// The flow ended (token received, failed or cancelled): close the prompt.
    Done,
}

impl TokenInfo {
    pub fn new(key: &str, token: &StoredToken) -> Self {
        Self {
            key: key.to_owned(),
            label: rt_core::auth::oauth2::key_label(key),
            obtained_at: token.obtained_at,
            expires_at: token.expires_at,
            refreshable: token.refresh_token.is_some(),
        }
    }
}

/// `SecretKey::environment` of token entries.
pub const TOKEN_ENVIRONMENT: &str = "oauth2";
/// `SecretKey::variable` of the entry listing a collection's token keys. Cache keys start
/// with `oauth2|`, so they never collide with it.
const INDEX: &str = "index";

/// Index updates are read-modify-write; one lock for the process keeps them whole.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// A [`SecretStore`] in memory: tokens for this process only (tests, and shells told not to
/// use the keychain).
#[derive(Default)]
pub struct MemorySecrets(Mutex<std::collections::HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
        Ok(())
    }
}

/// The tokens of one collection in a [`SecretStore`].
#[derive(Clone)]
pub struct SecretTokens {
    store: Arc<dyn SecretStore + Send + Sync>,
    collection: String,
}

impl SecretTokens {
    pub fn new(store: Arc<dyn SecretStore + Send + Sync>, collection: impl Into<String>) -> Self {
        Self {
            store,
            collection: collection.into(),
        }
    }

    fn key(&self, variable: &str) -> SecretKey {
        SecretKey {
            collection: self.collection.clone(),
            environment: TOKEN_ENVIRONMENT.to_owned(),
            variable: variable.to_owned(),
        }
    }

    fn index(&self) -> Result<Vec<String>, AppError> {
        Ok(match self.store.get(&self.key(INDEX))? {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => Vec::new(),
        })
    }

    fn write_index(&self, keys: &[String]) -> Result<(), AppError> {
        if keys.is_empty() {
            return self.store.delete(&self.key(INDEX));
        }
        let json = serde_json::to_string(keys).map_err(|e| AppError::Shell(e.to_string()))?;
        self.store.set(&self.key(INDEX), &json)
    }

    pub fn get_token(&self, key: &str) -> Result<Option<StoredToken>, AppError> {
        Ok(self
            .store
            .get(&self.key(key))?
            .and_then(|json| serde_json::from_str(&json).ok()))
    }

    pub fn put_token(&self, key: &str, token: &StoredToken) -> Result<(), AppError> {
        let json = serde_json::to_string(token).map_err(|e| AppError::Shell(e.to_string()))?;
        self.store.set(&self.key(key), &json)?;
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut keys = self.index()?;
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
            self.write_index(&keys)?;
        }
        Ok(())
    }

    pub fn remove_token(&self, key: &str) -> Result<(), AppError> {
        self.store.delete(&self.key(key))?;
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut keys = self.index()?;
        let before = keys.len();
        keys.retain(|k| k != key);
        if keys.len() != before {
            self.write_index(&keys)?;
        }
        Ok(())
    }

    /// The collection's tokens, oldest first. Entries whose token is gone are skipped.
    pub fn list(&self) -> Result<Vec<TokenInfo>, AppError> {
        let mut out = Vec::new();
        for key in self.index()? {
            if let Some(token) = self.get_token(&key)? {
                out.push(TokenInfo::new(&key, &token));
            }
        }
        out.sort_by_key(|t| t.obtained_at);
        Ok(out)
    }

    /// Forgets one token, or all of the collection's (`None`).
    pub fn clear(&self, key: Option<&str>) -> Result<(), AppError> {
        match key {
            Some(key) => self.remove_token(key),
            None => {
                for key in self.index()? {
                    self.store.delete(&self.key(&key))?;
                }
                let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                self.write_index(&[])
            }
        }
    }
}

/// Storage failures cannot stop a send: a token that is not cached is fetched again next
/// time. They are reported on stderr (the CLI) or the app's log.
impl TokenStore for SecretTokens {
    fn get<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, Option<StoredToken>> {
        let token = self.get_token(key).unwrap_or_else(|e| {
            log_storage_error(&e);
            None
        });
        Box::pin(std::future::ready(token))
    }

    fn put<'a>(&'a self, key: &'a str, token: &'a StoredToken) -> LocalBoxFuture<'a, ()> {
        if let Err(e) = self.put_token(key, token) {
            log_storage_error(&e);
        }
        Box::pin(std::future::ready(()))
    }

    fn remove<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, ()> {
        if let Err(e) = self.remove_token(key) {
            log_storage_error(&e);
        }
        Box::pin(std::future::ready(()))
    }
}

fn log_storage_error(e: &AppError) {
    // The message never holds a token: keychain errors name the entry at most.
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("RequestTailor: OAuth 2.0 token storage: {e}");
    #[cfg(target_arch = "wasm32")]
    let _ = e;
}

struct SnapshotFiles<'a>(&'a SendSnapshot);

impl FileSource for SnapshotFiles<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let key = path.trim_start_matches("./");
        self.0
            .files
            .files
            .get(key)
            .cloned()
            .ok_or_else(|| format!("no file {key}"))
    }
}

/// Runs the OAuth 2.0 flow of the snapshot's request now (whatever is cached) and stores the
/// token through `services.tokens`. Prompts through `services.prompt` as the flow needs.
pub async fn fetch_token<T: Transport>(
    snapshot: &SendSnapshot,
    transport: &T,
    services: &Services<'_>,
) -> Result<TokenInfo, AppError> {
    let context = snapshot
        .collection
        .request(&snapshot.request_path)
        .ok_or_else(|| AppError::RequestNotFound(snapshot.request_path.clone()))?;
    let input = PrepareInput {
        environment: snapshot.environment.as_deref(),
        globals: Vec::new(),
        runtime: snapshot.vars.clone(),
    };
    // Secret values may appear resolved in a message (e.g. an invalid URL): mask them.
    let secret_vars: Vec<&rt_core::model::Variable> = snapshot
        .environment
        .as_deref()
        .and_then(|name| snapshot.collection.environment(name))
        .into_iter()
        .flat_map(|e| &e.variables)
        .filter(|v| v.secret)
        .collect();
    let mut secrets: Vec<String> = secret_vars
        .iter()
        .filter_map(|v| v.value.clone())
        .chain(
            snapshot
                .vars
                .iter()
                .filter(|(n, _)| secret_vars.iter().any(|v| &v.name == n))
                .map(|(_, v)| v.clone()),
        )
        .filter(|v| !v.is_empty())
        .collect();
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mask = |text: String| {
        secrets
            .iter()
            .fold(text, |t, s| t.replace(s.as_str(), rt_core::send::MASK))
    };
    let prepared = prepare(&context, &input, &SnapshotFiles(snapshot))
        .map_err(|e| AppError::Auth(mask(e.to_string())))?;
    let Auth::OAuth2(oauth) = &prepared.auth else {
        return Err(AppError::Auth(
            "this request does not use OAuth 2.0 (also not inherited)".to_owned(),
        ));
    };
    let (key, token) =
        rt_core::auth::oauth2::fetch_token(oauth, &prepared.scope, transport, services)
            .await
            .map_err(|e| AppError::Auth(mask(e)))?;
    Ok(TokenInfo::new(&key, &token))
}

/// The system browser and a one-shot HTTP listener on the loopback redirect URI (RFC 8252
/// §7.3), for the authorization code flow on the desktop and in the CLI. Blocking; callers
/// run it on a thread of their own (both shells send on one).
#[cfg(not(target_arch = "wasm32"))]
pub mod loopback {
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    /// How long the browser step may take before the flow gives up.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    /// Opens `url` in the default browser. The URL goes to the OS as one argument, never
    /// through a shell.
    pub fn open_browser(url: &str) -> Result<(), String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("only http and https URLs are opened".to_owned());
        }
        #[cfg(target_os = "macos")]
        let mut command = std::process::Command::new("open");
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut c = std::process::Command::new("rundll32");
            c.arg("url.dll,FileProtocolHandler");
            c
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let mut command = std::process::Command::new("xdg-open");
        command
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("cannot open the browser: {e}"))
    }

    /// Listens on a loopback redirect URI (`http://127.0.0.1:<port>/<path>`, `localhost` or
    /// `[::1]`, explicit port).
    pub struct Listener {
        listener: TcpListener,
        /// `scheme://host:port`, to rebuild the full redirect URL.
        origin: String,
        path: String,
    }

    pub fn listen(redirect_uri: &str) -> Result<Listener, String> {
        let not_loopback = || {
            format!(
                "the callback URL {redirect_uri:?} is not a loopback URL with a port \
                 (e.g. http://127.0.0.1:8765/callback); the app can only receive the sign-in \
                 there. Register such a URL at the provider and use it as callback URL."
            )
        };
        let url = url::Url::parse(redirect_uri).map_err(|_| not_loopback())?;
        if url.scheme() != "http" {
            return Err(not_loopback());
        }
        let port = url.port().ok_or_else(not_loopback)?;
        let ip: std::net::IpAddr = match url.host() {
            Some(url::Host::Domain("localhost")) => std::net::Ipv4Addr::LOCALHOST.into(),
            Some(url::Host::Ipv4(ip)) if ip.is_loopback() => ip.into(),
            Some(url::Host::Ipv6(ip)) if ip.is_loopback() => ip.into(),
            _ => return Err(not_loopback()),
        };
        let listener = TcpListener::bind(SocketAddr::new(ip, port))
            .map_err(|e| format!("cannot listen on {redirect_uri}: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("cannot listen on {redirect_uri}: {e}"))?;
        let origin = url.origin().ascii_serialization();
        Ok(Listener {
            listener,
            origin,
            path: url.path().to_owned(),
        })
    }

    const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>RequestTailor</title>\
        <body style=\"font-family:system-ui,sans-serif;margin:3rem\"><h1>Signed in</h1>\
        <p>You can close this window and return to RequestTailor.</p>";

    impl Listener {
        /// Waits for the browser's request to the redirect path and returns its full URL.
        /// Other requests (e.g. `/favicon.ico`) get a 404. Stops when `cancelled()` or after
        /// `timeout`.
        pub fn wait(
            &self,
            cancelled: &dyn Fn() -> bool,
            timeout: Duration,
        ) -> Result<String, String> {
            let deadline = Instant::now() + timeout;
            loop {
                if cancelled() {
                    return Err("cancelled".to_owned());
                }
                if Instant::now() > deadline {
                    return Err("the sign-in in the browser took too long".to_owned());
                }
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        if let Some(url) = self.answer(stream) {
                            return Ok(url);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => return Err(format!("the redirect listener failed: {e}")),
                }
            }
        }

        /// Reads one request; answers it; returns the redirect URL if it was the one.
        fn answer(&self, mut stream: TcpStream) -> Option<String> {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&buf[..n]),
                }
            }
            let head = String::from_utf8_lossy(&head);
            let target = head
                .lines()
                .next()
                .and_then(|line| {
                    let mut parts = line.split(' ');
                    (parts.next() == Some("GET")).then(|| parts.next())?
                })
                .unwrap_or("");
            let path = target.split(['?', '#']).next().unwrap_or("");
            let hit = !target.is_empty() && path == self.path;
            let (status, body) = if hit {
                ("200 OK", DONE_PAGE)
            } else {
                ("404 Not Found", "not found")
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.flush();
            hit.then(|| format!("{}{target}", self.origin))
        }
    }

    /// Opens a URL in a browser.
    type Opener = dyn Fn(&str) -> Result<(), String> + Send + Sync;

    /// `rt_core::services::AuthPrompt` for native shells: binds the loopback redirect URI,
    /// opens the system browser, waits for the redirect; device codes go to `notify` (the
    /// desktop app emits them to the UI, the CLI prints them). `notify` never gets a token.
    pub struct NativePrompt {
        notify: Box<dyn Fn(&super::AuthPrompt) + Send + Sync>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        open: Box<Opener>,
        timeout: Duration,
    }

    impl NativePrompt {
        /// `cancel`: set it to stop a waiting flow (the shell's cancel button, Ctrl-C).
        pub fn new(
            notify: impl Fn(&super::AuthPrompt) + Send + Sync + 'static,
            cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ) -> Self {
            Self {
                notify: Box::new(notify),
                cancel,
                open: Box::new(open_browser),
                timeout: TIMEOUT,
            }
        }

        /// Replaces opening the system browser (tests, or a CLI that only prints the URL).
        pub fn with_opener(
            mut self,
            open: impl Fn(&str) -> Result<(), String> + Send + Sync + 'static,
        ) -> Self {
            self.open = Box::new(open);
            self
        }

        fn is_cancelled(&self) -> bool {
            self.cancel.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// Blocking; the shells run sends on a thread of their own.
        fn authorize_blocking(&self, url: &str, redirect_uri: &str) -> Result<String, String> {
            let listener = listen(redirect_uri)?;
            (self.notify)(&super::AuthPrompt::Browser {
                url: url.to_owned(),
            });
            let result = (self.open)(url)
                .and_then(|()| listener.wait(&|| self.is_cancelled(), self.timeout));
            (self.notify)(&super::AuthPrompt::Done);
            result
        }
    }

    impl rt_core::services::AuthPrompt for NativePrompt {
        fn authorize<'a>(
            &'a self,
            url: &'a str,
            redirect_uri: &'a str,
        ) -> rt_core::services::LocalBoxFuture<'a, Result<String, String>> {
            Box::pin(std::future::ready(
                self.authorize_blocking(url, redirect_uri),
            ))
        }

        fn device_code<'a>(
            &'a self,
            prompt: &'a rt_core::services::DeviceCodePrompt,
        ) -> rt_core::services::LocalBoxFuture<'a, ()> {
            (self.notify)(&super::AuthPrompt::DeviceCode(prompt.clone()));
            Box::pin(std::future::ready(()))
        }

        fn device_code_done<'a>(&'a self) -> rt_core::services::LocalBoxFuture<'a, ()> {
            (self.notify)(&super::AuthPrompt::Done);
            Box::pin(std::future::ready(()))
        }

        fn cancelled(&self) -> bool {
            self.is_cancelled()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Memory = MemorySecrets;

    fn token(access: &str, at: i64) -> StoredToken {
        StoredToken {
            access_token: access.into(),
            refresh_token: Some("r".into()),
            id_token: None,
            token_type: None,
            scope: None,
            expires_at: Some(at + 1000),
            obtained_at: at,
        }
    }

    #[test]
    fn tokens_live_in_the_secret_store_under_the_oauth2_environment() {
        let memory = Arc::new(Memory::default());
        let tokens = SecretTokens::new(memory.clone(), "/c");
        let key = "oauth2|1|client-credentials|https://idp/token|cli|||";
        tokens.put_token(key, &token("tok", 5)).unwrap();
        let stored = memory.0.lock().unwrap().clone();
        let entry = stored
            .get(&SecretKey {
                collection: "/c".into(),
                environment: "oauth2".into(),
                variable: key.into(),
            })
            .unwrap();
        assert!(entry.contains("\"tok\""));
        assert_eq!(tokens.get_token(key).unwrap().unwrap().access_token, "tok");

        let list = tokens.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].key, key);
        assert_eq!(list[0].label, "idp · cli");
        assert!(list[0].refreshable);
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains("tok\""), "no value in the list: {json}");

        // Another collection does not see it.
        assert!(
            SecretTokens::new(memory.clone(), "/other")
                .list()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn clearing_one_or_all() {
        let memory = Arc::new(Memory::default());
        let tokens = SecretTokens::new(memory.clone(), "/c");
        tokens.put_token("oauth2|a", &token("a", 2)).unwrap();
        tokens.put_token("oauth2|b", &token("b", 1)).unwrap();
        tokens.put_token("oauth2|a", &token("a2", 3)).unwrap();
        let keys: Vec<String> = tokens.list().unwrap().into_iter().map(|t| t.key).collect();
        assert_eq!(keys, ["oauth2|b", "oauth2|a"]);
        tokens.clear(Some("oauth2|b")).unwrap();
        assert_eq!(tokens.list().unwrap().len(), 1);
        tokens.clear(None).unwrap();
        assert!(tokens.list().unwrap().is_empty());
        assert!(memory.0.lock().unwrap().is_empty(), "index removed too");
    }

    #[test]
    fn the_token_store_trait() {
        let tokens = SecretTokens::new(Arc::new(Memory::default()), "/c");
        let store: &dyn TokenStore = &tokens;
        let poll = |f: LocalBoxFuture<'_, ()>| {
            let mut f = f;
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(f.as_mut().poll(&mut cx).is_ready());
        };
        poll(store.put("oauth2|k", &token("v", 1)));
        assert_eq!(
            tokens.get_token("oauth2|k").unwrap().unwrap().access_token,
            "v"
        );
        poll(store.remove("oauth2|k"));
        assert!(tokens.get_token("oauth2|k").unwrap().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_loopback_listener_returns_the_redirect() {
        use std::io::{Read as _, Write as _};

        assert!(loopback::listen("https://app.example/callback").is_err());
        assert!(loopback::listen("http://192.168.1.2:8765/cb").is_err());
        assert!(
            loopback::listen("http://127.0.0.1/cb").is_err(),
            "port required"
        );

        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let uri = format!("http://127.0.0.1:{port}/callback");
        let listener = loopback::listen(&uri).unwrap();
        let client = std::thread::spawn(move || {
            let get = |target: &str| {
                let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut answer = String::new();
                s.read_to_string(&mut answer).unwrap();
                answer
            };
            let other = get("/favicon.ico");
            let hit = get("/callback?code=abc&state=xyz");
            (other, hit)
        });
        let url = listener
            .wait(&|| false, std::time::Duration::from_secs(10))
            .unwrap();
        let (other, hit) = client.join().unwrap();
        assert_eq!(
            url,
            format!("http://127.0.0.1:{port}/callback?code=abc&state=xyz")
        );
        assert!(other.starts_with("HTTP/1.1 404"));
        assert!(hit.starts_with("HTTP/1.1 200") && hit.contains("Signed in"));
        assert!(!hit.contains("abc"), "the page does not echo the code");
        drop(listener);

        let cancelled = loopback::listen(&uri).unwrap();
        assert_eq!(
            cancelled.wait(&|| true, std::time::Duration::from_secs(10)),
            Err("cancelled".to_owned())
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_native_prompt_opens_the_browser_and_reports_the_steps() {
        use std::io::{Read as _, Write as _};
        use std::sync::atomic::AtomicBool;

        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        // The "browser": follows the authorization URL straight to the redirect.
        let prompt = loopback::NativePrompt::new(
            move |p| log.lock().unwrap().push(p.clone()),
            Arc::new(AtomicBool::new(false)),
        )
        .with_opener(move |url| {
            assert_eq!(url, "https://idp/authorize?x=1");
            std::thread::spawn(move || {
                let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET /cb?code=c1&state=s1 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let _ = s.read_to_end(&mut Vec::new());
            });
            Ok(())
        });
        let prompt: &dyn rt_core::services::AuthPrompt = &prompt;
        let redirect = format!("http://127.0.0.1:{port}/cb");
        let url = block_on(prompt.authorize("https://idp/authorize?x=1", &redirect)).unwrap();
        assert_eq!(url, format!("{redirect}?code=c1&state=s1"));
        let device = DeviceCodePrompt {
            user_code: "ABCD-EFGH".into(),
            verification_uri: "https://idp/device".into(),
            verification_uri_complete: None,
            expires_in: 600,
        };
        block_on(prompt.device_code(&device));
        block_on(prompt.device_code_done());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                AuthPrompt::Browser {
                    url: "https://idp/authorize?x=1".into()
                },
                AuthPrompt::Done,
                AuthPrompt::DeviceCode(device),
                AuthPrompt::Done,
            ]
        );

        // A redirect URI the app cannot listen on fails before the browser opens.
        let err = block_on(prompt.authorize("https://idp/a", "https://app.example/cb"));
        assert!(err.unwrap_err().contains("not a loopback URL"));
        assert_eq!(seen.lock().unwrap().len(), 4);
    }

    /// A token endpoint that answers every request with the same token.
    struct FakeIdp(std::cell::Cell<usize>);

    struct Once(Option<bytes::Bytes>);

    impl futures_core::Stream for Once {
        type Item = Result<bytes::Bytes, rt_core::transport::TransportError>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.take().map(Ok))
        }
    }

    impl Transport for FakeIdp {
        async fn send(
            &self,
            request: rt_core::transport::HttpRequest,
        ) -> Result<rt_core::transport::Response, rt_core::transport::TransportError> {
            self.0.set(self.0.get() + 1);
            assert_eq!(request.url, "https://idp.example/token");
            let body = r#"{"access_token":"the-token","refresh_token":"r","expires_in":60}"#;
            Ok(rt_core::transport::Response {
                head: rt_core::transport::ResponseHead {
                    status: 200,
                    version: rt_core::transport::HttpVersion::Http11,
                    headers: vec![],
                    url: request.url,
                    redirects: vec![],
                    timing: Default::default(),
                },
                body: Box::pin(Once(Some(bytes::Bytes::from_static(body.as_bytes())))),
            })
        }
    }

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    const SECURED: &str = r#"info:
  name: secured
  type: http
http:
  method: GET
  url: https://api
  auth:
    type: oauth2
    flow: client_credentials
    accessTokenUrl: "{{idp}}/token"
    credentials:
      clientId: cli
      clientSecret: "{{secret}}"
      placement: body
    scope: read
"#;

    #[test]
    fn fetching_a_token_for_a_request() {
        let mut files = rt_format::FileSet::default();
        let file = |s: &str| s.as_bytes().to_vec();
        files.files.insert(
            "opencollection.yml".into(),
            file("opencollection: 1.0.0\ninfo:\n  name: c\n"),
        );
        files.files.insert("secured.yml".into(), file(SECURED));
        files.files.insert(
            "plain.yml".into(),
            file("info:\n  name: plain\n  type: http\nhttp:\n  method: GET\n  url: https://api\n"),
        );
        let ws = crate::Workspace::open(files).unwrap();
        let input = crate::SendInput {
            environment: None,
            vars: vec![
                ("idp".into(), "https://idp.example".into()),
                ("secret".into(), "s3cret".into()),
            ],
            secrets: vec![],
        };
        let tokens = SecretTokens::new(Arc::new(Memory::default()), "/c");
        let services = Services {
            tokens: Some(&tokens),
            ..Services::none()
        };
        let idp = FakeIdp(Default::default());
        let snapshot = ws.snapshot("secured.yml", input.clone()).unwrap();
        let info = block_on(fetch_token(&snapshot, &idp, &services)).unwrap();
        assert_eq!(info.label, "idp.example · cli · read");
        assert!(info.refreshable);
        assert_eq!(tokens.list().unwrap(), vec![info.clone()]);
        assert_eq!(
            tokens.get_token(&info.key).unwrap().unwrap().access_token,
            "the-token"
        );
        // "Get token" always runs the flow, even with a valid token cached.
        block_on(fetch_token(&snapshot, &idp, &services)).unwrap();
        assert_eq!(idp.0.get(), 2);

        let snapshot = ws.snapshot("plain.yml", input).unwrap();
        let err = block_on(fetch_token(&snapshot, &idp, &services)).unwrap_err();
        assert!(matches!(err, AppError::Auth(m) if m.contains("does not use OAuth 2.0")));
    }
}
