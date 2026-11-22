// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the desktop app (wave 5, strand C): tokens in the keychain (`TokenStore`),
//! the system browser plus a loopback listener for the authorization code flow and the
//! `auth-prompt` event for the UI (`AuthPrompt`). Commands: `auth_tokens`,
//! `auth_fetch_token`, `auth_clear_tokens`, `auth_cancel_prompt`.
//!
//! - Tokens: `rt_app::auth::SecretTokens` over the keychain (service "RequestTailor", the
//!   same entries `rt send` uses), environment `oauth2`. The webview never gets a value:
//!   the commands return `TokenInfo` only.
//! - The browser is opened here, on the Rust side (`open`, `xdg-open`, `rundll32`); the
//!   webview has no shell or opener permission. Only the `auth-prompt` event reaches it
//!   (the authorization URL, a device code, or `done`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use rt_app::auth::loopback::NativePrompt;
use rt_app::auth::{AuthPrompt, MemorySecrets, SecretTokens, TokenInfo};
use rt_app::{AppError, SecretStore, SendInput, SendSnapshot, Workspace};
use rt_core::services::Services;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

/// The event the UI listens to (`apps/desktop/src/auth.ts`).
pub const PROMPT_EVENT: &str = "auth-prompt";

#[tauri::command]
pub async fn auth_tokens(core: State<'_, AppCore>) -> Result<Vec<TokenInfo>, CommandError> {
    let id = open_collection(&core)?;
    Ok(core.services.auth.tokens(&id).list()?)
}

#[tauri::command]
pub async fn auth_fetch_token(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<TokenInfo, CommandError> {
    Ok(fetch_token(&core, &path, input).await?)
}

#[tauri::command]
pub async fn auth_clear_tokens(
    core: State<'_, AppCore>,
    key: Option<String>,
) -> Result<(), CommandError> {
    let id = open_collection(&core)?;
    Ok(core.services.auth.tokens(&id).clear(key.as_deref())?)
}

#[tauri::command]
pub async fn auth_cancel_prompt(core: State<'_, AppCore>) -> Result<(), CommandError> {
    core.services.auth.cancel();
    Ok(())
}

fn open_collection<S: SecretStore>(core: &Core<S>) -> Result<String, AppError> {
    core.collection_id()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))
}

/// The open collection as it is now (unsaved edits of the request and the environments
/// included), with the selected environment's secrets from the keychain. `core.rs` keeps its
/// workspace to itself, so this reads the folder again and applies what the core has.
fn snapshot<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    mut input: SendInput,
) -> Result<(SendSnapshot, String), AppError> {
    let shell = |e: std::io::Error| AppError::Shell(e.to_string());
    let id = open_collection(core)?;
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    let mut workspace = Workspace::open(crate::files::read_folder(&root).map_err(shell)?)?;
    workspace.update_request(path, core.get_request(path)?)?;
    let environments = core.environments()?;
    for old in workspace.environments() {
        if !environments.iter().any(|e| e.name == old.name) {
            workspace.delete_environment(&old.name)?;
        }
    }
    for env in &environments {
        workspace.update_environment(env.clone())?;
    }
    // Never trusted from the webview, as at send.
    input.secrets = crate::core::secret_values(
        core.secrets(),
        &id,
        &environments,
        input.environment.as_deref(),
    )?;
    Ok((workspace.snapshot(path, input)?, id))
}

/// "Get token": runs the request's OAuth 2.0 flow now, on a blocking thread like a send.
pub async fn fetch_token<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    input: SendInput,
) -> Result<TokenInfo, AppError> {
    let (snapshot, id) = snapshot(core, path, input)?;
    let parts = Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let auth = parts.auth.for_send(&id);
        let services = Services {
            tokens: auth.tokens(),
            prompt: auth.prompt(),
            ..Services::none()
        };
        runtime.block_on(rt_app::auth::fetch_token(&snapshot, &transport, &services))
    })
    .await
    .map_err(|e| AppError::Shell(format!("fetching the token failed: {e}")))?
}

type Sink = Box<dyn Fn(&AuthPrompt) + Send + Sync>;

/// Token storage (keychain) and the prompt channel to the UI, kept by `Core`.
pub struct AuthParts {
    store: Arc<dyn SecretStore + Send + Sync>,
    cancel: Arc<AtomicBool>,
    /// Where prompts go: the `auth-prompt` event once the app is up ([`AuthParts::attach`]).
    sink: Arc<OnceLock<Sink>>,
    /// Prompts emitted without a sink (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    unsent: Arc<Mutex<Vec<AuthPrompt>>>,
}

impl AuthParts {
    /// `data_dir`: `Some` in the app (tokens in the keychain), `None` in tests (in memory).
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let store: Arc<dyn SecretStore + Send + Sync> = match data_dir {
            Some(_) => Arc::new(rt_app::secrets::keychain::KeychainStore::default()),
            None => Arc::new(MemorySecrets::default()),
        };
        Self {
            store,
            cancel: Arc::new(AtomicBool::new(false)),
            sink: Arc::new(OnceLock::new()),
            unsent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Sends prompts to the main window as the `auth-prompt` event (called once at setup).
    pub fn attach(&self, app: tauri::AppHandle) {
        use tauri::Emitter as _;
        let _ = self.sink.set(Box::new(move |prompt: &AuthPrompt| {
            let _ = app.emit_to("main", PROMPT_EVENT, prompt);
        }));
    }

    pub fn tokens(&self, collection: &str) -> SecretTokens {
        SecretTokens::new(Arc::clone(&self.store), collection)
    }

    /// Stops the flow waiting for the user (browser step or device code).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn for_send(&self, collection: &str) -> AuthSend<'_> {
        // A new flow starts uncancelled.
        self.cancel.store(false, Ordering::SeqCst);
        let sink = Arc::clone(&self.sink);
        let unsent = Arc::clone(&self.unsent);
        AuthSend {
            _parts: self,
            tokens: self.tokens(collection),
            prompt: NativePrompt::new(
                move |prompt| match sink.get() {
                    Some(send) => send(prompt),
                    None => unsent
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(prompt.clone()),
                },
                Arc::clone(&self.cancel),
            ),
        }
    }
}

/// Token cache and prompts of one send.
pub struct AuthSend<'a> {
    _parts: &'a AuthParts,
    tokens: SecretTokens,
    prompt: NativePrompt,
}

impl AuthSend<'_> {
    pub fn tokens(&self) -> Option<&dyn rt_core::services::TokenStore> {
        Some(&self.tokens)
    }

    pub fn prompt(&self) -> Option<&dyn rt_core::services::AuthPrompt> {
        Some(&self.prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_collection(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-auth-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: c\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("r.yml"),
            "info:\n  name: r\n  type: http\nhttp:\n  method: GET\n  url: https://api\n",
        )
        .unwrap();
        dir
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tokens_are_per_collection_and_never_hold_values() {
        let core = Core::new(MemorySecrets::default()).unwrap();
        let dir = temp_collection("tokens");
        let info = core.open(&dir).unwrap();
        let tokens = core.services.auth.tokens(&info.id);
        let token = rt_core::services::StoredToken {
            access_token: "secret-access".into(),
            refresh_token: None,
            id_token: None,
            token_type: None,
            scope: None,
            expires_at: None,
            obtained_at: 1,
        };
        tokens
            .put_token("oauth2|1|client-credentials|https://idp/t|c|||", &token)
            .unwrap();
        let list = core.services.auth.tokens(&info.id).list().unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            !serde_json::to_string(&list)
                .unwrap()
                .contains("secret-access")
        );
        assert!(
            core.services
                .auth
                .tokens("/other")
                .list()
                .unwrap()
                .is_empty()
        );
        core.services.auth.tokens(&info.id).clear(None).unwrap();
        assert!(
            core.services
                .auth
                .tokens(&info.id)
                .list()
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fetching_uses_the_unsaved_request() {
        let core = Core::new(MemorySecrets::default()).unwrap();
        let dir = temp_collection("fetch");
        core.open(&dir).unwrap();
        let input = SendInput {
            environment: None,
            vars: vec![],
            secrets: vec![],
        };
        // Saved without auth: refused before anything goes out.
        let err = fetch_token(&core, "r.yml", input.clone())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not use OAuth 2.0"), "{err}");

        // Unsaved: OAuth 2.0 implicit (never run), so the flow fails with its own error.
        let mut spec = core.get_request("r.yml").unwrap();
        let oauth = rt_core::model::OAuth2 {
            flow: rt_core::model::OAuth2Flow::Implicit {
                authorization_url: "https://idp/authorize".into(),
                callback_url: "http://127.0.0.1:8765/cb".into(),
                state: String::new(),
            },
            ..rt_core::model::OAuth2::default()
        };
        spec.auth = rt_core::model::Auth::OAuth2(Box::new(oauth));
        core.update_request("r.yml", spec).unwrap();
        let err = fetch_token(&core, "r.yml", input).await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("implicit"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prompts_without_a_window_are_kept_and_cancel_resets_per_flow() {
        let parts = AuthParts::new(None);
        parts.cancel();
        let send = parts.for_send("/c");
        let prompt = send.prompt().unwrap();
        assert!(!prompt.cancelled(), "a new flow starts uncancelled");
        parts.cancel();
        assert!(prompt.cancelled());
        let device = rt_core::services::DeviceCodePrompt {
            user_code: "WDJB-MJHT".into(),
            verification_uri: "https://idp/device".into(),
            verification_uri_complete: None,
            expires_in: 600,
        };
        drop(prompt.device_code(&device));
        assert_eq!(
            *parts.unsent.lock().unwrap(),
            vec![AuthPrompt::DeviceCode(device)]
        );
    }
}
