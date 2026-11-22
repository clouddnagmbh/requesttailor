// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands for gRPC services and templates and GraphQL schemas (wave 8, strand B):
//! `grpc_services`, `grpc_template`, `graphql_schema`.
//!
//! Like a send: secret values come from the keychain (and external secrets), never from the
//! webview. gRPC descriptors come from the request's or the collection's proto files, else by
//! server reflection over the native HTTP/2 stack. The GraphQL introspection query goes out
//! like the request itself, with the send services of the open collection (OAuth 2.0 tokens,
//! cookies, plugins); the SDL is cached per collection, environment and URL until `refresh`.

use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use rt_app::graphql::{self, SchemaCache};
use rt_app::grpc::{self, GrpcService};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// GraphQL schemas of this app (one app per process), keyed by collection id and
/// `graphql::cache_key`.
static SCHEMAS: LazyLock<Mutex<SchemaCache>> = LazyLock::new(Mutex::default);

/// The webview never supplies secret values.
fn from_webview(input: SendInput) -> SendInput {
    SendInput {
        secrets: Vec::new(),
        ..input
    }
}

fn transport() -> Result<NativeTransport, AppError> {
    NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))
}

/// Runs a core future (not `Send`) on a blocking thread driven by the app's runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce(&tokio::runtime::Handle) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || work(&runtime))
        .await
        .map_err(|e| AppError::Shell(format!("the call failed: {e}")))?
}

pub async fn services<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    input: SendInput,
) -> Result<Vec<GrpcService>, AppError> {
    let snapshot = core
        .with_send_input(from_webview(input), |ws, input, _| {
            grpc::snapshot(ws, path, input)
        })
        .await?;
    blocking(move |rt| rt.block_on(grpc::services(&snapshot, &transport()?))).await
}

pub async fn template<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    method: String,
    input: SendInput,
) -> Result<String, AppError> {
    let snapshot = core
        .with_send_input(from_webview(input), |ws, input, _| {
            grpc::snapshot(ws, path, input)
        })
        .await?;
    blocking(move |rt| rt.block_on(grpc::template(&snapshot, &method, &transport()?))).await
}

pub async fn schema<S: SecretStore>(
    core: &Core<S>,
    cache: &Mutex<SchemaCache>,
    path: &str,
    input: SendInput,
    refresh: bool,
) -> Result<String, AppError> {
    let (key, snapshot, id) = core
        .with_send_input(from_webview(input), |ws, input, id| {
            let key = format!("{id}\n{}", graphql::cache_key(ws, path, &input)?);
            let snapshot = graphql::introspection_snapshot(ws, path, input)?;
            Ok((key, snapshot, id.to_owned()))
        })
        .await?;
    if !refresh
        && let Some(sdl) = cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
    {
        return Ok(sdl.to_owned());
    }
    let parts = Arc::clone(&core.services);
    let report = blocking(move |rt| {
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        Ok(rt.block_on(snapshot.send_with(&transport()?, &options, &services.services())))
    })
    .await?;
    let sdl = graphql::schema_from_report(&report)?;
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .put(key, sdl.clone());
    Ok(sdl)
}

#[tauri::command]
pub async fn grpc_services(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<Vec<GrpcService>, CommandError> {
    Ok(services(&core, &path, input).await?)
}

#[tauri::command]
pub async fn grpc_template(
    core: State<'_, AppCore>,
    path: String,
    method: String,
    input: SendInput,
) -> Result<String, CommandError> {
    Ok(template(&core, &path, method, input).await?)
}

#[tauri::command]
pub async fn graphql_schema(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
    refresh: bool,
) -> Result<String, CommandError> {
    Ok(schema(&core, &SCHEMAS, &path, input, refresh).await?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};

    use rt_app::SecretKey;

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
                "rt-desktop-grpc-{tag}-{}-{:?}",
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

    fn collection(dir: &Path, http: SocketAddr, grpc: SocketAddr) -> PathBuf {
        let root = dir.join("collection");
        std::fs::create_dir_all(root.join("environments")).unwrap();
        let write = |path: &str, text: String| std::fs::write(root.join(path), text).unwrap();
        write(
            "opencollection.yml",
            "opencollection: \"1.0.0\"\ninfo:\n  name: B\n".into(),
        );
        write(
            "q.yml",
            "info:\n  name: q\n  type: graphql\n\ngraphql:\n  method: POST\n  url: \"{{base}}/graphql\"\n  body:\n    query: \"{ hello }\"\n    variables: \"\"\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n".into(),
        );
        write(
            "echo.yml",
            "info:\n  name: echo\n  type: grpc\n\ngrpc:\n  url: \"{{grpc}}\"\n  method: /rt.echo.v1.EchoService/Echo\n  methodType: unary\n".into(),
        );
        write(
            "environments/Local.yml",
            format!(
                "name: Local\nvariables:\n  - name: base\n    value: http://{http}\n  - name: grpc\n    value: grpc://{grpc}\n  - secret: true\n    name: token\n"
            ),
        );
        root
    }

    fn local() -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: Vec::new(),
            secrets: vec![("token".into(), "from-webview".into())],
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn services_templates_and_cached_schemas() {
        let http = rt_testserver::spawn_local().await;
        let grpc_addr = rt_testserver::spawn_grpc(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let tmp = TempDir::new("all");
        let root = collection(&tmp.0, http, grpc_addr);
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&root).unwrap();
        core.secret_set("Local", "token", "kc-secret").unwrap();

        let list = services(&core, "echo.yml", local()).await.unwrap();
        assert_eq!(list[0].name, "rt.echo.v1.EchoService");
        let t = template(
            &core,
            "echo.yml",
            "/rt.echo.v1.EchoService/Collect".into(),
            local(),
        )
        .await
        .unwrap();
        assert!(t.contains("\"message\": \"\""), "{t}");

        let cache = Mutex::new(SchemaCache::default());
        let sdl = schema(&core, &cache, "q.yml", local(), false)
            .await
            .unwrap();
        assert!(sdl.contains("type Book {"), "{sdl}");
        // Cached: a broken server does not matter until refresh.
        let again = schema(&core, &cache, "q.yml", local(), false)
            .await
            .unwrap();
        assert_eq!(again, sdl);
        assert!(services(&core, "q.yml", local()).await.is_err());
        assert!(matches!(
            template(&core, "echo.yml", "/nope.X/Y".into(), local()).await,
            Err(AppError::Shell(m)) if m.contains("/nope.X/Y")
        ));
    }
}
