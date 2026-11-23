// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands for the MCP client (stdio and Streamable HTTP). Wave 8, strand D.
//!
//! Commands: `mcp_connect`, `mcp_tools`, `mcp_call`, `mcp_disconnect`, `mcp_tests`,
//! `mcp_get_test`, `mcp_save_test`, `mcp_run_test`; wave 14: `mcp_resources`,
//! `mcp_read_resource`, `mcp_prompts`, `mcp_get_prompt`, `mcp_self_server`,
//! `mcp_demo_status`, `mcp_demo_start`, `mcp_demo_stop` (`McpClientApi` in
//! `apps/ui/src/commands/backend.ts`).
//!
//! - The demo server (wave 14): built into the client at `rt-demo://mcp` (nothing to do
//!   here), and on request on `127.0.0.1` with a random port ([`LocalDemo`],
//!   `rt_app::mcp_demo::serve::LocalServer`) for other MCP clients. It runs in this process
//!   until `mcp_demo_stop` or the app ends.
//! - "Connect to RequestTailor itself" (wave 14): [`self_server`] finds the `rt` command
//!   (next to the app, on the `PATH`, in the usual install folders) and offers `rt mcp` on
//!   the open collection as a stdio server, so the user sees what AI agents see.
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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

use rt_app::mcp_client::{
    self, McpCallResult, McpClient, McpError, McpPrompt, McpPromptResult, McpResource,
    McpResourceContents, McpSelfServer, McpServerConfig, McpServerInfo, McpTest, McpTestResult,
    McpTool, McpVars,
};
use rt_app::mcp_demo::serve::LocalServer;
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

/// Runs `f` with the connection `id` taken out, and puts it back.
async fn with_client<R, F, Fut>(connections: &Connections, id: u64, f: F) -> Result<R, AppError>
where
    R: Send + 'static,
    F: FnOnce(McpClient, NativeTransport) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = (McpClient, Result<R, McpError>)>,
{
    let client = connections.take(id)?;
    let (client, result) = blocking(move |t| f(client, t)).await?;
    connections.put(id, client);
    Ok(result?)
}

pub async fn resources(connections: &Connections, id: u64) -> Result<Vec<McpResource>, AppError> {
    with_client(connections, id, |mut c, t| async move {
        let r = c.resources(&t).await;
        (c, r)
    })
    .await
}

pub async fn read_resource(
    connections: &Connections,
    id: u64,
    uri: String,
) -> Result<Vec<McpResourceContents>, AppError> {
    with_client(connections, id, move |mut c, t| async move {
        let r = c.read_resource(&t, &uri).await;
        (c, r)
    })
    .await
}

pub async fn prompts(connections: &Connections, id: u64) -> Result<Vec<McpPrompt>, AppError> {
    with_client(connections, id, |mut c, t| async move {
        let r = c.prompts(&t).await;
        (c, r)
    })
    .await
}

pub async fn get_prompt(
    connections: &Connections,
    id: u64,
    name: String,
    args: Vec<(String, String)>,
) -> Result<McpPromptResult, AppError> {
    with_client(connections, id, move |mut c, t| async move {
        let r = c.get_prompt(&t, &name, &args).await;
        (c, r)
    })
    .await
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

// ---- wave 14: the demo server on a local port, and `rt mcp` itself ----

/// The demo server on a loopback port, at most one per app.
#[derive(Default)]
pub struct LocalDemo(Mutex<Option<LocalServer>>);

impl LocalDemo {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<LocalServer>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its URL while it runs.
    pub fn status(&self) -> Option<String> {
        self.lock().as_ref().map(LocalServer::url)
    }

    /// Starts it on a free port, or returns the running one's URL.
    pub fn start(&self) -> Result<String, AppError> {
        let mut slot = self.lock();
        if let Some(s) = slot.as_ref() {
            return Ok(s.url());
        }
        let server = LocalServer::start(0)
            .map_err(|e| AppError::Shell(format!("cannot start the demo server: {e}")))?;
        let url = server.url();
        *slot = Some(server);
        Ok(url)
    }

    pub fn stop(&self) {
        if let Some(s) = self.lock().take() {
            s.stop();
        }
    }
}

/// The app's demo server (it ends with the process).
static LOCAL_DEMO: LazyLock<LocalDemo> = LazyLock::new(LocalDemo::default);

#[cfg(windows)]
const RT: &str = "rt.exe";
#[cfg(not(windows))]
const RT: &str = "rt";

/// The `rt` command: `"rt"` when it is on the `PATH` (portable in saved tests), else the
/// full path of one next to the app or in a usual install folder.
fn find_rt(
    path_var: Option<&std::ffi::OsStr>,
    exe: Option<&Path>,
    home: Option<&Path>,
) -> Option<String> {
    let is_file = |p: &Path| p.is_file();
    if let Some(path) = path_var
        && std::env::split_paths(path).any(|d| is_file(&d.join(RT)))
    {
        return Some("rt".into());
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe.and_then(Path::parent) {
        candidates.push(dir.join(RT));
    }
    if let Some(home) = home {
        candidates.push(home.join(".cargo/bin").join(RT));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin").join(RT));
    candidates.push(PathBuf::from("/usr/local/bin").join(RT));
    candidates
        .into_iter()
        .find(|p| is_file(p))
        .map(|p| p.display().to_string())
}

/// How to connect to `rt mcp` on the collection at `root`.
pub fn self_server(root: Option<&Path>, rt: Option<String>) -> McpSelfServer {
    match (root, rt) {
        (None, _) => McpSelfServer {
            server: None,
            hint: "Open a collection first: rt mcp offers the open collection to AI agents.".into(),
        },
        (Some(_), None) => McpSelfServer {
            server: None,
            hint: "The rt command line was not found next to the app, on the PATH, in ~/.cargo/bin, /opt/homebrew/bin or /usr/local/bin. Install it (see the download page or `cargo install --path crates/rt-cli`) and try again; AI assistants start it as `rt mcp /path/to/collection`.".into(),
        },
        (Some(_), Some(command)) => McpSelfServer {
            server: Some(McpServerConfig::Stdio {
                command,
                args: vec!["mcp".into(), "--agent-name".into(), "RequestTailor app".into()],
                env: Vec::new(),
                cwd: None,
            }),
            hint: "Starts rt mcp on this collection, as an AI assistant would: its tools are your requests, under the collection's agent rules, and every call goes to the audit log.".into(),
        },
    }
}

#[tauri::command]
pub async fn mcp_resources(id: u64) -> Result<Vec<McpResource>, CommandError> {
    Ok(resources(&CONNECTIONS, id).await?)
}

#[tauri::command]
pub async fn mcp_read_resource(
    id: u64,
    uri: String,
) -> Result<Vec<McpResourceContents>, CommandError> {
    Ok(read_resource(&CONNECTIONS, id, uri).await?)
}

#[tauri::command]
pub async fn mcp_prompts(id: u64) -> Result<Vec<McpPrompt>, CommandError> {
    Ok(prompts(&CONNECTIONS, id).await?)
}

#[tauri::command]
pub async fn mcp_get_prompt(
    id: u64,
    name: String,
    args: Vec<(String, String)>,
) -> Result<McpPromptResult, CommandError> {
    Ok(get_prompt(&CONNECTIONS, id, name, args).await?)
}

#[tauri::command]
pub async fn mcp_self_server(core: State<'_, AppCore>) -> Result<McpSelfServer, CommandError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let exe = std::env::current_exe().ok();
    let rt = find_rt(
        std::env::var_os("PATH").as_deref(),
        exe.as_deref(),
        home.as_deref(),
    );
    Ok(self_server(core.root().as_deref(), rt))
}

#[tauri::command]
pub async fn mcp_demo_status() -> Result<Option<String>, CommandError> {
    Ok(LOCAL_DEMO.status())
}

#[tauri::command]
pub async fn mcp_demo_start() -> Result<String, CommandError> {
    Ok(LOCAL_DEMO.start()?)
}

#[tauri::command]
pub async fn mcp_demo_stop() -> Result<(), CommandError> {
    LOCAL_DEMO.stop();
    Ok(())
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
    async fn the_local_demo_server_starts_answers_and_stops() {
        let demo = LocalDemo::default();
        assert_eq!(demo.status(), None);
        let url = demo.start().unwrap();
        assert!(
            url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"),
            "{url}"
        );
        assert_eq!(demo.start().unwrap(), url, "one server at a time");
        assert_eq!(demo.status().as_deref(), Some(url.as_str()));

        // The client reaches it over real HTTP, browses everything and calls a tool.
        let tmp = TempDir::new("demo");
        let root = collection(&tmp.0, &url);
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&root).unwrap();
        let connections = Connections::default();
        let connected = connect(&core, &connections, server_without_auth(), local())
            .await
            .unwrap();
        assert_eq!(connected.info.name, rt_app::mcp_demo::SERVER_NAME);
        assert_eq!(tools(&connections, connected.id).await.unwrap().len(), 10);
        assert_eq!(
            resources(&connections, connected.id).await.unwrap().len(),
            3
        );
        let readme = read_resource(&connections, connected.id, "demo://readme".into())
            .await
            .unwrap();
        assert!(readme[0].text.as_deref().unwrap().contains("demo server"));
        assert_eq!(prompts(&connections, connected.id).await.unwrap().len(), 2);
        let prompt = get_prompt(
            &connections,
            connected.id,
            "weather_report".into(),
            vec![("city".into(), "Oslo".into())],
        )
        .await
        .unwrap();
        assert_eq!(prompt.messages.len(), 2);
        let weather = call(
            &connections,
            connected.id,
            "fake_weather".into(),
            json!({ "city": "Vienna" }),
        )
        .await
        .unwrap();
        assert_eq!(weather.structured.unwrap()["conditions"], "sunny");
        disconnect(&connections, connected.id).await.unwrap();

        demo.stop();
        assert_eq!(demo.status(), None);
        assert!(
            connect(&core, &connections, server_without_auth(), local())
                .await
                .is_err(),
            "stopped"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_built_in_demo_needs_no_server() {
        let tmp = TempDir::new("builtin");
        let root = collection(&tmp.0, rt_app::mcp_demo::DEMO_URL);
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&root).unwrap();
        let connections = Connections::default();
        let connected = connect(&core, &connections, server_without_auth(), local())
            .await
            .unwrap();
        let r = call(
            &connections,
            connected.id,
            "countdown".into(),
            json!({ "from": 2 }),
        )
        .await
        .unwrap();
        assert_eq!(r.notifications.len(), 4);
    }

    #[test]
    fn rt_is_found_on_the_path_or_next_to_the_app() {
        let tmp = TempDir::new("rt");
        let bin = tmp.0.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(
            find_rt(Some(bin.as_os_str()), None, None).filter(|c| c == "rt"),
            None
        );
        std::fs::write(bin.join(RT), "").unwrap();
        assert_eq!(
            find_rt(Some(bin.as_os_str()), None, None).as_deref(),
            Some("rt")
        );
        let app = tmp.0.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join(RT), "").unwrap();
        let found = find_rt(None, Some(&app.join("requesttailor")), None).unwrap();
        assert_eq!(PathBuf::from(found), app.join(RT));

        let root = Path::new("/c");
        let offered = self_server(Some(root), Some("rt".into()));
        assert!(matches!(
            offered.server,
            Some(McpServerConfig::Stdio { ref command, ref args, .. }) if command == "rt" && args[0] == "mcp"
        ));
        assert!(self_server(Some(root), None).server.is_none());
        assert!(
            self_server(None, Some("rt".into()))
                .hint
                .contains("Open a collection")
        );
    }

    fn server_without_auth() -> McpServerConfig {
        McpServerConfig::Http {
            url: "{{mcpUrl}}".into(),
            headers: Vec::new(),
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
