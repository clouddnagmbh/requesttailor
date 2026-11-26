// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant in the desktop app (wave 8, strand E; wave 16, strand A).
//!
//! - Off until the user sets it up: no settings file, no call. The settings (profiles, roles,
//!   preview mode; no key) live in `ai.json` in the data folder. A wave-8 `ai.json` (one
//!   config) reads as the profile `default` and is rewritten as settings on the next save.
//! - Keys live in the OS keychain: one per profile (`rt_app::ai::profile_key_slot`), falling
//!   back to the provider's (`key_slot`). The webview can set, delete and ask whether one is
//!   set, never read one back.
//! - `ai_preview` builds the masked call and describes it; `ai_ask` (a task) and
//!   `ai_complete` (a call of an extension, streamed over a Tauri channel) send exactly that
//!   call on a blocking thread over the native transport. `ai_cancel` stops one by id.
//! - A collection can be off in its file (`extensions.requesttailor.ai`) or on this device
//!   (`ai-off.json` in the data folder, collection ids); either gives `opted-out` before
//!   anything is built.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rt_app::ai::{
    self, AiAnswer, AiCollectionPolicy, AiConfig, AiError, AiEvent, AiPreview, AiProfile,
    AiProviderKind, AiReply, AiRequest, AiSettings, AiTask, AiWhat, Calls, Prepared,
};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::transport::native::NativeTransport;
use tauri::State;
use tauri::ipc::Channel;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

const CONFIG_FILE: &str = "ai.json";
const OFF_FILE: &str = "ai-off.json";

fn ai_error(e: AiError) -> CommandError {
    let kind = serde_json::to_value(&e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    CommandError {
        kind,
        message: ai::error_message(&e),
    }
}

fn shell(e: impl std::fmt::Display) -> AppError {
    AppError::Shell(e.to_string())
}

/// Without a data folder (tests) the settings and the opt-outs live in memory.
fn memory() -> &'static Mutex<(Option<AiSettings>, Vec<String>)> {
    static MEMORY: OnceLock<Mutex<(Option<AiSettings>, Vec<String>)>> = OnceLock::new();
    MEMORY.get_or_init(|| Mutex::new((None, Vec::new())))
}

fn calls() -> &'static Calls {
    static CALLS: OnceLock<Calls> = OnceLock::new();
    CALLS.get_or_init(Calls::default)
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_FILE)
}

/// The saved settings; `None` until the user sets them up (an unreadable file counts as
/// none).
pub fn load_settings(data_dir: Option<&Path>) -> Option<AiSettings> {
    match data_dir {
        Some(dir) => std::fs::read(config_path(dir))
            .ok()
            .and_then(|b| AiSettings::parse(&b)),
        None => memory().lock().ok()?.0.clone(),
    }
}

/// Saves `settings` (checked first; `previewed` is carried over only for profiles that did
/// not move); `None` turns the assistant off.
pub fn save_settings(
    data_dir: Option<&Path>,
    settings: Option<&AiSettings>,
) -> Result<(), AiError> {
    let settings = match settings {
        Some(s) => {
            s.validate()?;
            let mut s = s.clone();
            s.carry_previewed(load_settings(data_dir).as_ref());
            Some(s)
        }
        None => None,
    };
    write_settings(data_dir, settings.as_ref())
}

fn write_settings(data_dir: Option<&Path>, settings: Option<&AiSettings>) -> Result<(), AiError> {
    let Some(dir) = data_dir else {
        memory().lock().unwrap_or_else(|p| p.into_inner()).0 = settings.cloned();
        return Ok(());
    };
    let path = config_path(dir);
    let io = |e: std::io::Error| AiError::Transport(format!("cannot save the AI settings: {e}"));
    match settings {
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

/// The user confirmed the first-use preview of `profile`.
pub fn confirm_preview(data_dir: Option<&Path>, profile: &str) -> Result<(), AiError> {
    let Some(mut s) = load_settings(data_dir) else {
        return Err(AiError::NotConfigured);
    };
    s.confirm(profile);
    write_settings(data_dir, Some(&s))
}

/// The wave-8 view: the default profile's configuration.
pub fn load_config(data_dir: Option<&Path>) -> Option<AiConfig> {
    load_settings(data_dir)?.default_config().cloned()
}

/// The wave-8 setter: replaces the default profile's configuration (or creates the profile
/// `default`); `None` turns the assistant off.
pub fn save_config(data_dir: Option<&Path>, config: Option<&AiConfig>) -> Result<(), AiError> {
    let Some(config) = config else {
        return save_settings(data_dir, None);
    };
    config.validate()?;
    let mut s = match load_settings(data_dir) {
        Some(mut s) => {
            let id = s.resolve(None).map(|p| p.id.clone());
            match id.and_then(|id| s.profiles.iter_mut().find(|p| p.id == id)) {
                Some(p) => p.config = config.clone(),
                None => s = AiSettings::from_legacy(config.clone()),
            }
            s
        }
        None => AiSettings::from_legacy(config.clone()),
    };
    s.validate()?;
    s.carry_previewed(load_settings(data_dir).as_ref());
    write_settings(data_dir, Some(&s))
}

fn off_ids(data_dir: Option<&Path>) -> Vec<String> {
    match data_dir {
        Some(dir) => std::fs::read(dir.join(OFF_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default(),
        None => memory().lock().map(|m| m.1.clone()).unwrap_or_default(),
    }
}

/// Turns the assistant off (or on again) for collection `id` on this device.
pub fn set_device_off(data_dir: Option<&Path>, id: &str, off: bool) -> Result<(), AiError> {
    let mut ids = off_ids(data_dir);
    ids.retain(|i| i != id);
    if off {
        ids.push(id.to_owned());
    }
    match data_dir {
        Some(dir) => {
            let io =
                |e: std::io::Error| AiError::Transport(format!("cannot save the AI settings: {e}"));
            std::fs::create_dir_all(dir).map_err(io)?;
            std::fs::write(
                dir.join(OFF_FILE),
                serde_json::to_vec(&ids).unwrap_or_default(),
            )
            .map_err(io)
        }
        None => {
            memory().lock().unwrap_or_else(|p| p.into_inner()).1 = ids;
            Ok(())
        }
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
    set_slot(store, &ai::key_slot(provider), key)
}

fn set_slot<S: SecretStore>(
    store: &S,
    slot: &rt_app::SecretKey,
    key: &str,
) -> Result<(), AppError> {
    let key = key.trim();
    if key.is_empty() {
        return store.delete(slot);
    }
    store.set(slot, key)
}

pub fn has_profile_key<S: SecretStore>(store: &S, profile: &str) -> Result<bool, AppError> {
    Ok(store
        .get(&ai::profile_key_slot(profile))?
        .is_some_and(|k| !k.is_empty()))
}

/// The key of `profile`: its own, else the provider's.
fn key_of<S: SecretStore>(store: &S, profile: &AiProfile) -> Result<Option<String>, AppError> {
    let own = store
        .get(&ai::profile_key_slot(&profile.id))?
        .filter(|k| !k.is_empty());
    match own {
        Some(k) => Ok(Some(k)),
        None => Ok(store
            .get(&ai::key_slot(profile.config.provider))?
            .filter(|k| !k.is_empty())),
    }
}

fn checked_key<S: SecretStore>(
    store: &S,
    profile: &AiProfile,
) -> Result<Option<String>, CommandError> {
    let key = key_of(store, profile)?;
    if profile.config.provider.needs_key() && key.is_none() {
        return Err(ai_error(AiError::NoKey));
    }
    Ok(key)
}

/// Builds what `what` would send (profile, unmasked call, secrets) for the active collection.
fn prepare<S: SecretStore>(
    core: &Core<S>,
    what: &AiWhat,
    input: SendInput,
) -> Result<(Prepared, AiSettings), CommandError> {
    let settings = load_settings(core.data_dir.as_deref());
    // Never trusted from the webview, as at send.
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let off = off_ids(core.data_dir.as_deref());
    let prepared = core.with_ai_scope(input, |ws, responses, input, id| {
        let tokens = ai::token_values(&core.services.auth.tokens(id))
            .map_err(|e| AiError::Transport(e.to_string()))?;
        ai::prepare_what(
            ws,
            responses,
            settings.as_ref(),
            what,
            input,
            tokens,
            off.iter().any(|i| i == id),
        )
    })?;
    let prepared = prepared.map_err(ai_error)?;
    Ok((
        prepared,
        settings.ok_or_else(|| ai_error(AiError::NotConfigured))?,
    ))
}

/// The policy of the active collection.
pub fn policy<S: SecretStore>(core: &Core<S>) -> Result<AiCollectionPolicy, AppError> {
    let off = off_ids(core.data_dir.as_deref());
    core.with_ai_scope(SendInput::default(), |ws, _, _, id| {
        ai::policy(ws.collection(), off.iter().any(|i| i == id))
    })
}

/// Exactly what `what` would send, masked; nothing leaves.
pub fn preview<S: SecretStore>(
    core: &Core<S>,
    what: &AiWhat,
    input: SendInput,
) -> Result<AiPreview, CommandError> {
    let (p, settings) = prepare(core, what, input)?;
    ai::preview(&settings, &p.profile, p.call, &p.secrets).map_err(ai_error)
}

/// One task: the call of the profile of its role, the answer parsed.
pub async fn ask<S: SecretStore>(
    core: &Core<S>,
    task: AiTask,
    input: SendInput,
) -> Result<AiAnswer, CommandError> {
    // A cURL snippet is read by the importer: no model, so no settings needed.
    if let Some(local) = ai::local_answer(&task) {
        return local.map_err(ai_error);
    }
    let (prepared, settings) = prepare(core, &AiWhat::Task { task: task.clone() }, input)?;
    let key = checked_key(core.secrets(), &prepared.profile)?;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AiError::Transport(e.to_string()))?;
        runtime.block_on(ai::ask_with(
            &transport,
            &settings,
            prepared,
            &task,
            key,
            false,
            &ai::CancelToken::default(),
        ))
    })
    .await
    .map_err(|e| CommandError::from(shell(format!("the AI call failed: {e}"))))?
    .map_err(ai_error)
}

/// A call of an extension, streamed: `on` gets the pieces and the end. `ai_cancel` stops it.
pub async fn complete<S: SecretStore>(
    core: &Core<S>,
    request: AiRequest,
    input: SendInput,
    on: impl Fn(AiEvent) + Send + Sync + 'static,
) -> Result<AiReply, CommandError> {
    let id = request.id.clone();
    let (prepared, settings) = prepare(core, &AiWhat::Request { request }, input)?;
    let key = checked_key(core.secrets(), &prepared.profile)?;
    let shown = ai::preview(
        &settings,
        &prepared.profile,
        prepared.call.clone(),
        &prepared.secrets,
    )
    .map_err(ai_error)?;
    let (token, _guard) = calls().start(&id);
    let runtime = tokio::runtime::Handle::current();
    let on = std::sync::Arc::new(on);
    let on2 = on.clone();
    let result = tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AiError::Transport(e.to_string()))?;
        runtime.block_on(ai::execute(
            &transport,
            &prepared.profile,
            key,
            false,
            shown.call,
            &prepared.secrets,
            &token,
            &|d| on2(AiEvent::Delta { delta: d }),
        ))
    })
    .await
    .map_err(|e| CommandError::from(shell(format!("the AI call failed: {e}"))))?;
    match result {
        Ok(reply) => {
            on(AiEvent::Done {
                reply: reply.clone(),
            });
            Ok(reply)
        }
        Err(e) => {
            on(AiEvent::Error { error: e.clone() });
            Err(ai_error(e))
        }
    }
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

#[tauri::command]
pub async fn ai_settings(core: State<'_, AppCore>) -> Result<Option<AiSettings>, CommandError> {
    Ok(load_settings(core.data_dir.as_deref()))
}

#[tauri::command]
pub async fn ai_set_settings(
    core: State<'_, AppCore>,
    settings: Option<AiSettings>,
) -> Result<(), CommandError> {
    save_settings(core.data_dir.as_deref(), settings.as_ref()).map_err(ai_error)
}

#[tauri::command]
pub async fn ai_has_profile_key(
    core: State<'_, AppCore>,
    profile: String,
) -> Result<bool, CommandError> {
    Ok(has_profile_key(core.secrets(), &profile)?)
}

#[tauri::command]
pub async fn ai_set_profile_key(
    core: State<'_, AppCore>,
    profile: String,
    key: String,
) -> Result<(), CommandError> {
    Ok(set_slot(
        core.secrets(),
        &ai::profile_key_slot(&profile),
        &key,
    )?)
}

#[tauri::command]
pub async fn ai_delete_profile_key(
    core: State<'_, AppCore>,
    profile: String,
) -> Result<(), CommandError> {
    Ok(core.secrets().delete(&ai::profile_key_slot(&profile))?)
}

#[tauri::command]
pub async fn ai_policy(core: State<'_, AppCore>) -> Result<AiCollectionPolicy, CommandError> {
    Ok(policy(core.inner())?)
}

#[tauri::command]
pub async fn ai_set_device_opt_out(
    core: State<'_, AppCore>,
    off: bool,
) -> Result<(), CommandError> {
    let id = core
        .collection_id()
        .ok_or_else(|| CommandError::from(shell("no collection is open")))?;
    set_device_off(core.data_dir.as_deref(), &id, off).map_err(ai_error)
}

#[tauri::command]
pub async fn ai_preview(
    core: State<'_, AppCore>,
    what: AiWhat,
    input: SendInput,
) -> Result<AiPreview, CommandError> {
    preview(core.inner(), &what, input)
}

#[tauri::command]
pub async fn ai_confirm_preview(
    core: State<'_, AppCore>,
    profile: String,
) -> Result<(), CommandError> {
    confirm_preview(core.data_dir.as_deref(), &profile).map_err(ai_error)
}

#[tauri::command]
pub async fn ai_complete(
    core: State<'_, AppCore>,
    request: AiRequest,
    input: SendInput,
    on_event: Channel<AiEvent>,
) -> Result<AiReply, CommandError> {
    complete(core.inner(), request, input, move |e| {
        let _ = on_event.send(e);
    })
    .await
}

#[tauri::command]
pub async fn ai_cancel(id: String) -> Result<(), CommandError> {
    calls().cancel(&id);
    Ok(())
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

    /// A fake local model: answers one request with `text` and returns the raw request.
    fn fake_model(text: &str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let text = text.to_owned();
        let h = std::thread::spawn(move || {
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
        (port, h)
    }

    fn profile(id: &str, port: u16) -> rt_app::ai::AiProfile {
        rt_app::ai::AiProfile {
            id: id.into(),
            label: id.into(),
            config: local(&format!("http://127.0.0.1:{port}/v1")),
        }
    }

    fn open_core(data: &Path) -> (Core<MemorySecrets>, PathBuf) {
        let collection = temp_dir("coll");
        std::fs::write(
            collection.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: c\n",
        )
        .unwrap();
        let core = Core::with_data_dir(MemorySecrets::default(), Some(data.to_path_buf())).unwrap();
        core.open(&collection).unwrap();
        (core, collection)
    }

    fn request(id: &str, profile: Option<&str>) -> AiRequest {
        AiRequest {
            id: id.into(),
            purpose: "test".into(),
            profile: profile.map(str::to_owned),
            system: "be brief".into(),
            messages: vec![rt_app::ai::AiMessage::text(
                rt_app::ai::AiRole::User,
                "say hi with sk-live-secret-1",
            )],
            scope: Default::default(),
            tools: vec![],
            json: None,
            max_tokens: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn roles_route_to_different_profiles_keys_fall_back_and_the_preview_is_what_is_sent() {
        let data = temp_dir("roles");
        let (core, collection) = open_core(&data);
        let (fast_port, fast) = fake_model("fast answer");
        let (smart_port, smart) = fake_model("smart answer");
        let mut s = AiSettings {
            profiles: vec![profile("quick", fast_port), profile("big", smart_port)],
            default_profile: Some("big".into()),
            roles: rt_app::ai::AiRoles {
                fast: Some("quick".into()),
                smart: Some("big".into()),
            },
            ..Default::default()
        };
        // An endpoint that needs no key: give "quick" its own key anyway, "big" none.
        save_settings(Some(&data), Some(&s)).unwrap();
        set_slot(
            core.secrets(),
            &ai::profile_key_slot("quick"),
            "profile-key-1",
        )
        .unwrap();
        set_key(
            core.secrets(),
            AiProviderKind::OpenaiCompatible,
            "provider-key-2",
        )
        .unwrap();

        // The preview shows the first use, masks the secret, and is the call that goes out.
        let what = AiWhat::Request {
            request: request("c1", Some("fast")),
        };
        let shown = preview(&core, &what, SendInput::default()).unwrap();
        assert_eq!(shown.profile, "quick");
        assert!(
            shown.required,
            "the first call of a profile asks for the preview"
        );
        assert!(
            shown.endpoint.starts_with("http://127.0.0.1:")
                && shown.endpoint.ends_with("/v1/chat/completions")
        );

        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let reply = complete(
            &core,
            request("c1", Some("fast")),
            SendInput::default(),
            move |e| sink.lock().unwrap().push(e),
        )
        .await
        .unwrap();
        assert_eq!(reply.text, "fast answer");
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(AiEvent::Done { .. })
        ));
        let reply = complete(
            &core,
            request("c2", Some("smart")),
            SendInput::default(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(reply.text, "smart answer");

        let fast_raw = fast.join().unwrap();
        let smart_raw = smart.join().unwrap();
        assert!(
            fast_raw
                .to_ascii_lowercase()
                .contains("authorization: bearer profile-key-1"),
            "{fast_raw}"
        );
        assert!(
            smart_raw
                .to_ascii_lowercase()
                .contains("authorization: bearer provider-key-2"),
            "{smart_raw}"
        );
        let body = fast_raw.split("\r\n\r\n").nth(1).unwrap();
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            sent["messages"][0]["content"],
            serde_json::json!(shown.call.system)
        );
        assert_eq!(
            sent["messages"][1]["content"],
            serde_json::json!(shown.call.messages[0].content)
        );

        // Confirming remembers the first use; moving the profile forgets it.
        confirm_preview(Some(&data), "quick").unwrap();
        assert!(
            !preview(&core, &what, SendInput::default())
                .unwrap()
                .required
        );
        s = load_settings(Some(&data)).unwrap();
        s.profiles[0].config.model = "other".into();
        save_settings(Some(&data), Some(&s)).unwrap();
        assert!(
            preview(&core, &what, SendInput::default())
                .unwrap()
                .required
        );
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&collection);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_collection_off_in_its_file_or_on_the_device_is_never_called() {
        let data = temp_dir("optout");
        let (core, collection) = open_core(&data);
        // Nothing listens here: any call would fail with a transport error, not opted-out.
        save_settings(
            Some(&data),
            Some(&AiSettings::from_legacy(local("http://127.0.0.1:9/v1"))),
        )
        .unwrap();
        assert!(policy(&core).unwrap().allowed());

        let id = core.collection_id().unwrap();
        set_device_off(Some(&data), &id, true).unwrap();
        assert!(policy(&core).unwrap().device_off);
        let err = complete(&core, request("o1", None), SendInput::default(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.kind, "opted-out");
        assert_eq!(
            preview(
                &core,
                &AiWhat::Request {
                    request: request("o1", None)
                },
                SendInput::default()
            )
            .unwrap_err()
            .kind,
            "opted-out"
        );
        let task = AiTask::CreateRequest {
            description: "x".into(),
            folder: String::new(),
        };
        assert_eq!(
            ask(&core, task, SendInput::default())
                .await
                .unwrap_err()
                .kind,
            "opted-out"
        );
        set_device_off(Some(&data), &id, false).unwrap();
        assert!(policy(&core).unwrap().allowed());

        // The team's switch in the file.
        std::fs::write(
            collection.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: c\nextensions:\n  requesttailor:\n    ai:\n      enabled: false\n",
        )
        .unwrap();
        core.reload().unwrap();
        assert!(policy(&core).unwrap().file_off);
        let err = complete(&core, request("o2", None), SendInput::default(), |_| {})
            .await
            .unwrap_err();
        assert_eq!(err.kind, "opted-out");
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&collection);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_a_running_call_ends_it_with_cancelled() {
        let data = temp_dir("cancel");
        let (core, collection) = open_core(&data);
        // A server that accepts and never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hold = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(stream);
        });
        save_settings(
            Some(&data),
            Some(&AiSettings::from_legacy(local(&format!(
                "http://127.0.0.1:{port}/v1"
            )))),
        )
        .unwrap();
        let run = complete(&core, request("slow", None), SendInput::default(), |_| {});
        let stop = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            calls().cancel("slow");
        };
        let (result, ()) = tokio::join!(run, stop);
        assert_eq!(result.unwrap_err().kind, "cancelled");
        let _ = hold.join();
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&collection);
    }
}
