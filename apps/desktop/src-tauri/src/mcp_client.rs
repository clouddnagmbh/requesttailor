// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands for the MCP client (stdio and Streamable HTTP). Wave 8, strand D.
//!
//! Commands: `mcp_connect`, `mcp_tools`, `mcp_call`, `mcp_disconnect`, `mcp_tests`,
//! `mcp_get_test`, `mcp_save_test`, `mcp_run_test` (`McpClientApi` in
//! `apps/ui/src/commands/backend.ts`).
//!
//! - Like a send: the variables are the open collection's with the selected environment and
//!   its secret values from the keychain (and external secrets), never from the webview
//!   (`Core::run_snapshot`). Everything returned is masked by `rt_app::mcp_client`.
//! - Connections live here, by id, until `mcp_disconnect` (or the app ends; stdio servers
//!   are killed when their connection is dropped). The client's futures are not `Send`, so
//!   every call runs on a blocking thread driven by the app's runtime, as sends do; one call
//!   per connection at a time.
//! - stdio servers start in the collection folder (`cwd` below it).
//! - Saved tests are staged in the workspace (`Core::with_workspace`) and written by `save`;
//!   a test that would contain a stored secret value is refused.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

use rt_app::mcp_client::{
    self, McpCallResult, McpClient, McpServerConfig, McpServerInfo, McpTest, McpTestResult,
    McpTool, McpVars,
};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::transport::native::NativeTransport;
use serde::Serialize;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

/// What `mcp_connect` returns (`{ id, info }` in TypeScript).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connected {
    pub id: u64,
    pub info: McpServerInfo,
}

/// Open connections by id. A connection is taken out while a call runs.
#[derive(Default)]
pub struct Connections {
    next: AtomicU64,
    open: Mutex<HashMap<u64, Option<McpClient>>>,
}

impl Connections {
    fn add(&self, client: McpClient) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.lock().insert(id, Some(client));
        id
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Option<McpClient>>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn take(&self, id: u64) -> Result<McpClient, AppError> {
        match self.lock().get_mut(&id) {
            Some(slot) => slot
                .take()
                .ok_or_else(|| AppError::Shell("the MCP connection is busy".into())),
            None => Err(AppError::Shell(format!("no MCP connection {id}"))),
        }
    }

    fn put(&self, id: u64, client: McpClient) {
        if let Some(slot) = self.lock().get_mut(&id) {
            *slot = Some(client);
        }
    }

    fn remove(&self, id: u64) -> Option<McpClient> {
        self.lock().remove(&id).flatten()
    }
}

/// The app's connections (one app per process).
static CONNECTIONS: LazyLock<Connections> = LazyLock::new(Connections::default);

/// Runs `f` with a native transport on a blocking thread driven by the current runtime.
async fn blocking<T, F, Fut>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce(NativeTransport) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        Ok(runtime.block_on(f(transport)))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the MCP call failed: {e}")))?
}

/// The variables of a connection: never the webview's secrets.
async fn vars<S: SecretStore>(core: &Core<S>, input: SendInput) -> Result<McpVars, AppError> {
    let input = SendInput {
        secrets: Vec::new(),
        ..input
    };
    let (snapshot, _) = core.run_snapshot(input).await?;
    Ok(McpVars::of_snapshot(&snapshot))
}

pub async fn connect<S: SecretStore>(
    core: &Core<S>,
    connections: &Connections,
    server: McpServerConfig,
    input: SendInput,
) -> Result<Connected, AppError> {
    let vars = vars(core, input).await?;
    let root = core.root();
    let client =
        blocking(
            move |t| async move { McpClient::connect(&t, &server, vars, root.as_deref()).await },
        )
        .await??;
    let info = client.info().clone();
    Ok(Connected {
        id: connections.add(client),
        info,
    })
}

pub async fn tools(connections: &Connections, id: u64) -> Result<Vec<McpTool>, AppError> {
    let mut client = connections.take(id)?;
    let (client, tools) = blocking(move |t| async move {
        let tools = client.tools(&t).await;
        (client, tools)
    })
    .await?;
    connections.put(id, client);
    Ok(tools?)
}

pub async fn call(
    connections: &Connections,
    id: u64,
    tool: String,
    args: serde_json::Value,
) -> Result<McpCallResult, AppError> {
    let mut client = connections.take(id)?;
    let (client, result) = blocking(move |t| async move {
        let result = client.call(&t, &tool, &args).await;
        (client, result)
    })
    .await?;
    connections.put(id, client);
    Ok(result?)
}

pub async fn disconnect(connections: &Connections, id: u64) -> Result<(), AppError> {
    if let Some(client) = connections.remove(id) {
        blocking(move |t| async move { client.disconnect(&t).await }).await?;
    }
    Ok(())
}

/// Stored values of every secret variable of the collection's environments.
fn secret_values<S: SecretStore>(
    core: &Core<S>,
    workspace: &rt_app::Workspace,
    id: &str,
) -> Result<Vec<String>, AppError> {
    let environments = workspace.environments();
    let mut values = Vec::new();
    for env in &environments {
        for (_, value) in
            crate::core::secret_values(core.secrets(), id, &environments, Some(&env.name))?
        {
            values.push(value);
        }
    }
    Ok(values)
}

pub fn save_test<S: SecretStore>(
    core: &Core<S>,
    path: Option<String>,
    test: McpTest,
) -> Result<String, AppError> {
    core.with_workspace(|ws, id| {
        let secrets = secret_values(core, ws, id)?;
        mcp_client::save_test(ws, path.as_deref(), &test, &secrets)
    })
}

pub async fn run_test<S: SecretStore>(
    core: &Core<S>,
    path: String,
    input: SendInput,
) -> Result<McpTestResult, AppError> {
    let test = core.with_workspace(|ws, _| mcp_client::get_test(ws, &path))?;
    let vars = vars(core, input).await?;
    let root = core.root();
    blocking(move |t| async move {
        mcp_client::run_test(&t, &path, &test, &vars, root.as_deref()).await
    })
    .await
}

#[tauri::command]
pub async fn mcp_connect(
    core: State<'_, AppCore>,
    server: McpServerConfig,
    input: SendInput,
) -> Result<Connected, CommandError> {
    Ok(connect(&core, &CONNECTIONS, server, input).await?)
}

#[tauri::command]
pub async fn mcp_tools(id: u64) -> Result<Vec<McpTool>, CommandError> {
    Ok(tools(&CONNECTIONS, id).await?)
}

#[tauri::command]
pub async fn mcp_call(
    id: u64,
    tool: String,
    args: serde_json::Value,
) -> Result<McpCallResult, CommandError> {
    Ok(call(&CONNECTIONS, id, tool, args).await?)
}

#[tauri::command]
pub async fn mcp_disconnect(id: u64) -> Result<(), CommandError> {
    Ok(disconnect(&CONNECTIONS, id).await?)
}

#[tauri::command]
pub async fn mcp_tests(core: State<'_, AppCore>) -> Result<Vec<String>, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(mcp_client::tests(ws)))?)
}

#[tauri::command]
pub async fn mcp_get_test(core: State<'_, AppCore>, path: String) -> Result<McpTest, CommandError> {
    Ok(core.with_workspace(|ws, _| mcp_client::get_test(ws, &path))?)
}

#[tauri::command]
pub async fn mcp_save_test(
    core: State<'_, AppCore>,
    path: Option<String>,
    test: McpTest,
) -> Result<String, CommandError> {
    Ok(save_test(&core, path, test)?)
}

#[tauri::command]
pub async fn mcp_run_test(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<McpTestResult, CommandError> {
    Ok(run_test(&core, path, input).await?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use rt_app::SecretKey;
    use serde_json::json;

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
                "rt-desktop-mcp-{tag}-{}-{:?}",
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

    fn collection(dir: &Path, url: &str) -> PathBuf {
        let root = dir.join("collection");
        std::fs::create_dir_all(root.join("environments")).unwrap();
        std::fs::write(
            root.join("opencollection.yml"),
            "opencollection: \"1.0.0\"\ninfo:\n  name: MCP\n",
        )
        .unwrap();
        std::fs::write(
            root.join("environments/Local.yml"),
            format!("name: Local\nvariables:\n  - name: mcpUrl\n    value: {url}\n  - secret: true\n    name: token\n"),
        )
        .unwrap();
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

    fn server() -> McpServerConfig {
        McpServerConfig::Http {
            url: "{{mcpUrl}}".into(),
            headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn connect_list_call_save_and_run_with_keychain_secrets() {
        let addr = rt_testserver::spawn_mcp("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let tmp = TempDir::new("flow");
        let root = collection(&tmp.0, &format!("http://{addr}/mcp"));
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&root).unwrap();
        core.secret_set("Local", "token", "kc-mcp-secret").unwrap();
        let connections = Connections::default();

        let connected = connect(&core, &connections, server(), local())
            .await
            .unwrap();
        assert_eq!(connected.info.name, "rt-testserver");
        let listed = tools(&connections, connected.id).await.unwrap();
        assert_eq!(listed.len(), 6);
        let secret = call(&connections, connected.id, "secret".into(), json!({}))
            .await
            .unwrap();
        // The keychain's value went out (not the webview's), masked in the result.
        assert_eq!(
            serde_json::to_value(&secret.content).unwrap(),
            json!([{ "type": "text", "text": "Bearer ***" }])
        );
        disconnect(&connections, connected.id).await.unwrap();
        assert!(tools(&connections, connected.id).await.is_err());

        // Save as a test: staged, then written by save.
        let test = McpTest {
            name: "Secret header".into(),
            server: server(),
            tool: "secret".into(),
            arguments: json!({}),
            assertions: vec![rt_core::model::Assertion {
                expression: "res.body".into(),
                operator: "eq".into(),
                value: "Bearer {{token}}".into(),
                description: None,
                enabled: true,
            }],
        };
        let path = save_test(&core, None, test.clone()).unwrap();
        assert_eq!(path, "mcp/secret-header.mcp.yaml");
        assert_eq!(core.dirty().unwrap(), std::slice::from_ref(&path));
        let result = run_test(&core, path.clone(), local()).await.unwrap();
        assert!(result.passed, "{result:?}");
        core.save().unwrap();
        let text = std::fs::read_to_string(root.join(&path)).unwrap();
        assert!(text.contains("Bearer {{token}}"), "{text}");
        assert!(!text.contains("kc-mcp-secret"));

        // The stored value itself is refused.
        let mut leaky = test;
        leaky.arguments = json!({ "x": "kc-mcp-secret" });
        assert!(matches!(
            save_test(&core, Some(path), leaky),
            Err(AppError::SecretsFound(_))
        ));
    }
}
