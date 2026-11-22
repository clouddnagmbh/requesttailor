// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant in the desktop app (wave 8, strand E). Commands: `ai_config`,
//! `ai_set_config`, `ai_has_key`, `ai_set_key`, `ai_delete_key`, `ai_ask`.
//!
//! - Off until the user sets it up: no config file, no call. The config (provider, model,
//!   base URL; no key) lives in `ai.json` in the data folder.
//! - Keys live in the OS keychain (`rt_app::ai::key_slot`); the webview can set and delete
//!   one, and learn whether one is set, never read it back.
//! - `ai_ask` builds the masked context in the core (`Core::ai_context`), adds the stored
//!   OAuth tokens to the values masked, and calls the provider over the native transport on
//!   a blocking thread, like a send. The answer is a proposal; nothing is saved here.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rt_app::ai::{self, AiAnswer, AiConfig, AiError, AiProviderKind, AiTask};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

const CONFIG_FILE: &str = "ai.json";

fn ai_error(e: AiError) -> CommandError {
    let kind = serde_json::to_value(&e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    CommandError {
        kind,
        message: e.to_string(),
    }
}

fn shell(e: impl std::fmt::Display) -> AppError {
    AppError::Shell(e.to_string())
}

/// Without a data folder (tests) the config lives in memory.
fn memory() -> &'static Mutex<Option<AiConfig>> {
    static MEMORY: OnceLock<Mutex<Option<AiConfig>>> = OnceLock::new();
    MEMORY.get_or_init(|| Mutex::new(None))
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_FILE)
}

/// The saved config; `None` until the user sets one up (an unreadable file counts as none).
pub fn load_config(data_dir: Option<&Path>) -> Option<AiConfig> {
    match data_dir {
        Some(dir) => std::fs::read(config_path(dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok()),
        None => memory().lock().ok()?.clone(),
    }
}

/// Saves `config` (checked first); `None` turns the assistant off.
pub fn save_config(data_dir: Option<&Path>, config: Option<&AiConfig>) -> Result<(), AiError> {
    if let Some(c) = config {
        c.validate()?;
    }
    let Some(dir) = data_dir else {
        *memory().lock().unwrap_or_else(|p| p.into_inner()) = config.cloned();
        return Ok(());
    };
    let path = config_path(dir);
    let io = |e: std::io::Error| AiError::Transport(format!("cannot save the AI settings: {e}"));
    match config {
        Some(c) => {
            std::fs::create_dir_all(dir).map_err(io)?;
            let json = serde_json::to_vec_pretty(c).unwrap_or_default();
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, json).map_err(io)?;
            std::fs::rename(&tmp, &path).map_err(io)
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        },
    }
}

pub fn has_key<S: SecretStore>(store: &S, provider: AiProviderKind) -> Result<bool, AppError> {
    Ok(store
        .get(&ai::key_slot(provider))?
        .is_some_and(|k| !k.is_empty()))
}

pub fn set_key<S: SecretStore>(
    store: &S,
    provider: AiProviderKind,
    key: &str,
) -> Result<(), AppError> {
    let key = key.trim();
    if key.is_empty() {
        return store.delete(&ai::key_slot(provider));
    }
    store.set(&ai::key_slot(provider), key)
}

/// One task: the masked context from the core, the provider from the config and keychain.
pub async fn ask<S: SecretStore>(
    core: &Core<S>,
    task: AiTask,
    input: SendInput,
) -> Result<AiAnswer, CommandError> {
    let config =
        load_config(core.data_dir.as_deref()).ok_or_else(|| ai_error(AiError::NotConfigured))?;
    config.validate().map_err(ai_error)?;
    let key = core.secrets().get(&ai::key_slot(config.provider))?;
    if config.provider.needs_key() && key.as_deref().is_none_or(str::is_empty) {
        return Err(ai_error(AiError::NoKey));
    }
    // Never trusted from the webview, as at send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let (context, mut secrets, id) = core.ai_context(&task, input)?;
    secrets.extend(ai::token_values(&core.services.auth.tokens(&id))?);
    let runtime = tokio::runtime::Handle::current();
    let answer = tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AiError::Transport(e.to_string()))?;
        let provider = ai::provider(&transport, &config, key, false)?;
        runtime.block_on(ai::run(&task, &context, &*provider, &secrets))
    })
    .await
    .map_err(|e| CommandError::from(shell(format!("the AI call failed: {e}"))))?;
    answer.map_err(ai_error)
}

#[tauri::command]
pub async fn ai_config(core: State<'_, AppCore>) -> Result<Option<AiConfig>, CommandError> {
    Ok(load_config(core.data_dir.as_deref()))
}

#[tauri::command]
pub async fn ai_set_config(
    core: State<'_, AppCore>,
    config: Option<AiConfig>,
) -> Result<(), CommandError> {
    save_config(core.data_dir.as_deref(), config.as_ref()).map_err(ai_error)
}

#[tauri::command]
pub async fn ai_has_key(
    core: State<'_, AppCore>,
    provider: AiProviderKind,
) -> Result<bool, CommandError> {
    Ok(has_key(core.secrets(), provider)?)
}

#[tauri::command]
pub async fn ai_set_key(
    core: State<'_, AppCore>,
    provider: AiProviderKind,
    key: String,
) -> Result<(), CommandError> {
    Ok(set_key(core.secrets(), provider, &key)?)
}

#[tauri::command]
pub async fn ai_delete_key(
    core: State<'_, AppCore>,
    provider: AiProviderKind,
) -> Result<(), CommandError> {
    Ok(core.secrets().delete(&ai::key_slot(provider))?)
}

#[tauri::command]
pub async fn ai_ask(
    core: State<'_, AppCore>,
    task: AiTask,
    input: SendInput,
) -> Result<AiAnswer, CommandError> {
    ask(core.inner(), task, input).await
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    use rt_app::auth::MemorySecrets;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-ai-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn local(base: &str) -> AiConfig {
        AiConfig {
            provider: AiProviderKind::OpenaiCompatible,
            model: "llama3.1".into(),
            base_url: Some(base.into()),
        }
    }

    #[test]
    fn config_file_and_keys() {
        let dir = temp_dir("config");
        assert_eq!(load_config(Some(&dir)), None);
        save_config(Some(&dir), Some(&local("http://localhost:11434/v1"))).unwrap();
        assert_eq!(
            load_config(Some(&dir)).unwrap().base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        let bad = AiConfig {
            base_url: None,
            ..local("")
        };
        assert_eq!(
            save_config(Some(&dir), Some(&bad)),
            Err(AiError::NotConfigured)
        );
        save_config(Some(&dir), None).unwrap();
        assert_eq!(load_config(Some(&dir)), None);
        save_config(Some(&dir), None).unwrap();

        let store = MemorySecrets::default();
        assert!(!has_key(&store, AiProviderKind::Anthropic).unwrap());
        set_key(&store, AiProviderKind::Anthropic, " sk-1 ").unwrap();
        assert!(has_key(&store, AiProviderKind::Anthropic).unwrap());
        assert!(!has_key(&store, AiProviderKind::OpenaiCompatible).unwrap());
        set_key(&store, AiProviderKind::Anthropic, "").unwrap();
        assert!(!has_key(&store, AiProviderKind::Anthropic).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ask_is_off_until_configured_and_masks_keychain_secrets() {
        let data = temp_dir("data");
        let collection = temp_dir("collection");
        std::fs::write(
            collection.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: c\n",
        )
        .unwrap();
        std::fs::create_dir_all(collection.join("environments")).unwrap();
        std::fs::write(
            collection.join("environments/Local.yml"),
            "name: Local\nvariables:\n  - secret: true\n    name: token\n",
        )
        .unwrap();
        let core = Core::with_data_dir(MemorySecrets::default(), Some(data.clone())).unwrap();
        core.open(&collection).unwrap();
        core.secret_set("Local", "token", "keychain-value-42")
            .unwrap();
        let task = AiTask::CreateRequest {
            description: "use keychain-value-42".into(),
            folder: String::new(),
        };
        let input = SendInput {
            environment: Some("Local".into()),
            ..Default::default()
        };
        let err = ask(&core, task.clone(), input.clone()).await.unwrap_err();
        assert_eq!(err.kind, "not-configured");

        // A fake local model that records what it got.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut raw = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                raw.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            raw.push_str(&String::from_utf8_lossy(&body));
            let text = "```json\n{\"text\":\"t\",\"request\":{\"name\":\"N\",\"method\":\"GET\",\"url\":\"{{base}}/x\"}}\n```";
            let answer =
                serde_json::json!({ "choices": [{ "message": { "content": text } }] }).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
            raw
        });
        save_config(
            Some(&data),
            Some(&local(&format!("http://127.0.0.1:{port}/v1"))),
        )
        .unwrap();
        let answer = ask(&core, task, input).await.unwrap();
        assert_eq!(answer.request.unwrap().url, "{{base}}/x");
        let raw = server.join().unwrap();
        assert!(raw.contains("POST /v1/chat/completions"), "{raw}");
        assert!(!raw.contains("keychain-value-42"), "{raw}");
        assert!(raw.contains("token (secret)"), "{raw}");
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&collection);
    }
}
