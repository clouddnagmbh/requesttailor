// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! External secret sources (wave 5, strand E): the values of an environment's
//! `external_secrets` (1Password, HashiCorp Vault, AWS Secrets Manager, Azure Key Vault, Google
//! Cloud Secret Manager), read with each provider's own command line tool. That needs no
//! network code of our own and uses the login the user already has (`op signin`, `vault
//! login`, `aws sso login`, `az login`, `gcloud auth login`).
//!
//! Collection files hold only the reference (product principle 3). The shells resolve the
//! references of the selected environment when sending and put the values into
//! `SendInput::secrets`, like keychain values: they are secret variables of the environment
//! layer, so the report masks them and plugins never see them. Values live in memory only:
//! [`SecretResolver`] caches them for the session (until the app quits, or `rt` exits).
//!
//! This module is platform-neutral: a reference becomes a [`SecretCommand`], and the
//! command's output becomes the value or a clear error. Running the command is the shell's
//! job ([`CommandRunner`]; [`ProcessRunner`] on native targets, the companion `rt agent` for
//! the web app), so tests run with fake runners.
//!
//! References (the provider's field in OpenCollection `externalSecrets`):
//!
//! | Provider | Field | Reference | Command |
//! | --- | --- | --- | --- |
//! | 1Password | `secretReference` | `op://vault/item/field` | `op read --no-newline <ref>` |
//! | HashiCorp Vault (server, HCP) | `path` | `<path>[#<field>]`; field defaults to the variable name | `vault kv get -format=json <path>`; `vault read -format=json <path>` for KV 2 API paths (`<mount>/data/…`) |
//! | AWS Secrets Manager | `secretName` | `<name or ARN>[#<json key>]` | `aws secretsmanager get-secret-value --secret-id=<name> --query SecretString --output text` |
//! | Azure Key Vault | `vaultName` | `<vault>[/<secret>[/<version>]]`; secret defaults to the variable name | `az keyvault secret show --vault-name=<vault> --name=<secret> --query value --output tsv` |
//! | Google Cloud | `projectId` | `<project>[/<secret>[/<version>]]` or `projects/<p>/secrets/<s>[/versions/<v>]`; secret defaults to the variable name, version to `latest` | `gcloud secrets versions access <version> --secret=<secret> --project=<project>` |
//!
//! Vault: `vault kv get` reads KV mounts of version 1 and 2 by their logical path
//! (`secret/app`). A path whose second segment is `data` (`secret/data/app`, the KV 2 API
//! path) is read with `vault read`. Both answer JSON; the field is taken from `data` (KV 1)
//! or `data.data` (KV 2).
//!
//! Every part of a reference is checked before it reaches a command line: no leading `-` (no
//! option injection), no whitespace or control characters. The program is fixed per provider;
//! nothing in a file can choose it.
//!
//! Owner: wave 5, strand E.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rt_core::model::{ExternalSecret, ExternalSecrets, SecretProvider};

/// How long one command may take. Generous: 1Password and others may show a login or
/// biometric prompt first.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// Larger outputs are refused (a secret is small; this bounds memory).
pub const MAX_OUTPUT: usize = 1024 * 1024;

/// A command to run: a fixed program and its arguments. Run without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretCommand {
    /// `op`, `vault`, `aws`, `az` or `gcloud`.
    pub program: &'static str,
    pub args: Vec<String>,
}

impl SecretCommand {
    /// The command as a user would type it (for error messages; references are not secret).
    pub fn display(&self) -> String {
        std::iter::once(self.program.to_owned())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Why a command produced no output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The program is not installed (not on `PATH`).
    NotFound,
    /// It did not finish in time and was stopped.
    Timeout,
    /// It ran and failed.
    Failed { code: Option<i32>, stderr: String },
    /// It could not be run, or its output was too large.
    Other(String),
}

/// Runs a [`SecretCommand`] and returns its standard output. Implemented by the shells.
pub trait CommandRunner: Send + Sync {
    fn run(&self, command: &SecretCommand, timeout: Duration) -> Result<Vec<u8>, RunError>;
}

/// What to run for one variable and how to read the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub command: SecretCommand,
    read: Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    /// The output is the value.
    Whole,
    /// The output is JSON; the value is this field (Vault).
    VaultField(String),
    /// The output is a JSON object; the value is this key (AWS `#key`).
    JsonKey(String),
}

/// Display name of a provider, for messages and the UI.
pub fn provider_label(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::OnePassword => "1Password",
        SecretProvider::HashicorpVaultServer => "HashiCorp Vault",
        SecretProvider::HashicorpVaultCloud => "HCP Vault",
        SecretProvider::AwsSecretsManager => "AWS Secrets Manager",
        SecretProvider::AzureKeyVault => "Azure Key Vault",
        SecretProvider::GcpSecretsManager => "Google Cloud Secret Manager",
    }
}

/// How to sign in, for "not signed in" errors.
fn login_hint(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::OnePassword => {
            "run `op signin` (or turn on the CLI integration of the 1Password app and unlock it)"
        }
        SecretProvider::HashicorpVaultServer | SecretProvider::HashicorpVaultCloud => {
            "set VAULT_ADDR (and VAULT_NAMESPACE for HCP Vault) and run `vault login`"
        }
        SecretProvider::AwsSecretsManager => {
            "run `aws sso login` or `aws configure` (and set AWS_PROFILE or AWS_REGION if needed)"
        }
        SecretProvider::AzureKeyVault => "run `az login`",
        SecretProvider::GcpSecretsManager => "run `gcloud auth login`",
    }
}

fn install_hint(provider: SecretProvider) -> &'static str {
    match provider {
        SecretProvider::OnePassword => {
            "the 1Password CLI (https://developer.1password.com/docs/cli)"
        }
        SecretProvider::HashicorpVaultServer | SecretProvider::HashicorpVaultCloud => {
            "the Vault CLI (https://developer.hashicorp.com/vault/install)"
        }
        SecretProvider::AwsSecretsManager => "the AWS CLI (https://aws.amazon.com/cli/)",
        SecretProvider::AzureKeyVault => {
            "the Azure CLI (https://learn.microsoft.com/cli/azure/install-azure-cli)"
        }
        SecretProvider::GcpSecretsManager => {
            "the Google Cloud CLI (https://cloud.google.com/sdk/docs/install)"
        }
    }
}

/// A part of a reference that goes on a command line.
fn part<'a>(
    provider: SecretProvider,
    name: &str,
    what: &str,
    s: &'a str,
) -> Result<&'a str, String> {
    let bad = |why: &str| {
        format!(
            "{}: the reference of `{name}` has an invalid {what} ({why})",
            provider_label(provider)
        )
    };
    if s.is_empty() {
        return Err(bad("empty"));
    }
    if s.starts_with('-') {
        return Err(bad("must not start with `-`"));
    }
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("no spaces or control characters"));
    }
    Ok(s)
}

/// The command that reads `secret` from `provider`.
pub fn lookup(provider: SecretProvider, secret: &ExternalSecret) -> Result<Lookup, String> {
    let name = secret.name.as_str();
    let reference = secret.reference.trim();
    if reference.is_empty() {
        return Err(format!(
            "{}: `{name}` has no reference",
            provider_label(provider)
        ));
    }
    let p = |what: &str, s: &'_ str| part(provider, name, what, s).map(str::to_owned);
    Ok(match provider {
        SecretProvider::OnePassword => {
            if !reference.starts_with("op://") {
                return Err(format!(
                    "1Password: the reference of `{name}` must look like op://vault/item/field"
                ));
            }
            Lookup {
                command: SecretCommand {
                    program: "op",
                    args: vec![
                        "read".into(),
                        "--no-newline".into(),
                        p("reference", reference)?,
                    ],
                },
                read: Read::Whole,
            }
        }
        SecretProvider::HashicorpVaultServer | SecretProvider::HashicorpVaultCloud => {
            let (path, field) = match reference.split_once('#') {
                Some((path, field)) => (path, field),
                None => (reference, name),
            };
            let path = p("path", path.trim_matches('/'))?;
            let field = p("field", field)?;
            let api_path = path.split('/').nth(1) == Some("data");
            let mut args: Vec<String> = if api_path {
                vec!["read".into()]
            } else {
                vec!["kv".into(), "get".into()]
            };
            args.push("-format=json".into());
            args.push(path);
            Lookup {
                command: SecretCommand {
                    program: "vault",
                    args,
                },
                read: Read::VaultField(field),
            }
        }
        SecretProvider::AwsSecretsManager => {
            let (id, key) = match reference.split_once('#') {
                Some((id, key)) => (id, Some(key)),
                None => (reference, None),
            };
            let id = p("secret name", id)?;
            Lookup {
                command: SecretCommand {
                    program: "aws",
                    args: vec![
                        "secretsmanager".into(),
                        "get-secret-value".into(),
                        format!("--secret-id={id}"),
                        "--query".into(),
                        "SecretString".into(),
                        "--output".into(),
                        "text".into(),
                    ],
                },
                read: match key {
                    Some(key) => Read::JsonKey(p("JSON key", key)?),
                    None => Read::Whole,
                },
            }
        }
        SecretProvider::AzureKeyVault => {
            let mut parts = reference.trim_matches('/').splitn(3, '/');
            let vault = p("vault name", parts.next().unwrap_or_default())?;
            let secret_name = p("secret name", parts.next().unwrap_or(name))?;
            let mut args = vec![
                "keyvault".into(),
                "secret".into(),
                "show".into(),
                format!("--vault-name={vault}"),
                format!("--name={secret_name}"),
            ];
            if let Some(version) = parts.next() {
                args.push(format!("--version={}", p("version", version)?));
            }
            args.extend(["--query", "value", "--output", "tsv"].map(String::from));
            Lookup {
                command: SecretCommand {
                    program: "az",
                    args,
                },
                read: Read::Whole,
            }
        }
        SecretProvider::GcpSecretsManager => {
            let r = reference.trim_matches('/');
            let (project, secret_id, version) = if let Some(rest) = r.strip_prefix("projects/") {
                // projects/<p>/secrets/<s>[/versions/<v>]
                let segs: Vec<&str> = rest.split('/').collect();
                match segs.as_slice() {
                    [project] => (*project, name, "latest"),
                    [project, "secrets", s] => (*project, *s, "latest"),
                    [project, "secrets", s, "versions", v] => (*project, *s, *v),
                    _ => {
                        return Err(format!(
                            "Google Cloud: the reference of `{name}` must look like \
                             projects/<project>/secrets/<secret>[/versions/<version>]"
                        ));
                    }
                }
            } else {
                let mut parts = r.splitn(3, '/');
                (
                    parts.next().unwrap_or_default(),
                    parts.next().unwrap_or(name),
                    parts.next().unwrap_or("latest"),
                )
            };
            Lookup {
                command: SecretCommand {
                    program: "gcloud",
                    args: vec![
                        "secrets".into(),
                        "versions".into(),
                        "access".into(),
                        p("version", version)?,
                        format!("--secret={}", p("secret name", secret_id)?),
                        format!("--project={}", p("project", project)?),
                    ],
                },
                read: Read::Whole,
            }
        }
    })
}

/// The value from a successful command's output.
fn value_from(
    provider: SecretProvider,
    name: &str,
    read: &Read,
    out: &[u8],
) -> Result<String, String> {
    let label = provider_label(provider);
    let text = String::from_utf8(out.to_vec())
        .map_err(|_| format!("{label}: the value of `{name}` is not text"))?;
    // The tools end their output with a line break; the value does not.
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(&text);
    let pick = |object: &serde_json::Map<String, serde_json::Value>, field: &str| {
        let value = object.get(field).ok_or_else(|| {
            let mut fields: Vec<&str> = object.keys().map(String::as_str).collect();
            fields.sort_unstable();
            format!(
                "{label}: the secret for `{name}` has no field `{field}` (fields: {}); \
                 add `#<field>` to the reference",
                fields.join(", ")
            )
        })?;
        Ok(match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    };
    match read {
        Read::Whole => {
            // `--output text` prints a missing string (a binary secret) as `None`.
            if provider == SecretProvider::AwsSecretsManager && text == "None" {
                return Err(format!(
                    "{label}: the secret for `{name}` has no text value (binary secrets are not supported)"
                ));
            }
            Ok(text.to_owned())
        }
        Read::JsonKey(key) => {
            let json: serde_json::Value = serde_json::from_str(text).map_err(|_| {
                format!("{label}: the secret for `{name}` is not a JSON object, so `#{key}` cannot be read")
            })?;
            let object = json.as_object().ok_or_else(|| {
                format!("{label}: the secret for `{name}` is not a JSON object, so `#{key}` cannot be read")
            })?;
            pick(object, key)
        }
        Read::VaultField(field) => {
            let json: serde_json::Value = serde_json::from_str(text)
                .map_err(|_| format!("{label}: unexpected answer from `vault` for `{name}`"))?;
            let mut data = json
                .get("data")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| format!("{label}: the secret for `{name}` has no data"))?;
            // KV version 2 nests the secret's fields in `data.data`, next to `data.metadata`.
            if data.contains_key("metadata")
                && let Some(inner) = data.get("data").and_then(serde_json::Value::as_object)
            {
                data = inner;
            }
            pick(data, field)
        }
    }
}

/// Patterns in a tool's error output that mean "not signed in".
fn needs_login(provider: SecretProvider, stderr: &str) -> bool {
    let e = stderr.to_ascii_lowercase();
    let any = |patterns: &[&str]| patterns.iter().any(|p| e.contains(p));
    match provider {
        SecretProvider::OnePassword => any(&[
            "not currently signed in",
            "no accounts configured",
            "session expired",
            "authorization prompt dismissed",
            "account is not signed in",
            "signin",
        ]),
        SecretProvider::HashicorpVaultServer | SecretProvider::HashicorpVaultCloud => any(&[
            "missing client token",
            "permission denied",
            "token is expired",
            "vault_addr",
            "connection refused",
        ]),
        SecretProvider::AwsSecretsManager => any(&[
            "unable to locate credentials",
            "expiredtoken",
            "token has expired",
            "sso session",
            "invalidclienttokenid",
            "you must specify a region",
        ]),
        SecretProvider::AzureKeyVault => any(&["az login", "not logged in", "expired"]),
        SecretProvider::GcpSecretsManager => any(&[
            "gcloud auth login",
            "reauthentication",
            "do not currently have an active account",
            "credentials",
        ]),
    }
}

/// The first non-empty line of `stderr`, cut to a readable length.
fn first_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no error message");
    let mut out: String = line.chars().take(300).collect();
    if line.chars().count() > 300 {
        out.push('…');
    }
    out
}

/// The message for a command that failed.
pub fn failure(
    provider: SecretProvider,
    name: &str,
    command: &SecretCommand,
    error: &RunError,
    timeout: Duration,
) -> String {
    let label = provider_label(provider);
    match error {
        RunError::NotFound => format!(
            "{label}: `{}` was not found, so `{name}` cannot be read. Install {} and sign in.",
            command.program,
            install_hint(provider)
        ),
        RunError::Timeout => format!(
            "{label}: `{}` did not answer within {} s for `{name}` (is it waiting for a login? {}).",
            command.program,
            timeout.as_secs(),
            login_hint(provider)
        ),
        RunError::Failed { stderr, .. } if needs_login(provider, stderr) => format!(
            "{label}: not signed in, or no access to `{name}`: {}. To sign in, {}.",
            first_line(stderr),
            login_hint(provider)
        ),
        RunError::Failed { code, stderr } => format!(
            "{label}: `{}` failed for `{name}`{}: {}",
            command.display(),
            code.map(|c| format!(" (exit code {c})"))
                .unwrap_or_default(),
            first_line(stderr)
        ),
        RunError::Other(m) => format!(
            "{label}: cannot run `{}` for `{name}`: {m}",
            command.program
        ),
    }
}

/// Reads one variable's value.
pub fn resolve_one(
    runner: &dyn CommandRunner,
    provider: SecretProvider,
    secret: &ExternalSecret,
    timeout: Duration,
) -> Result<String, String> {
    let lookup = lookup(provider, secret)?;
    let out = runner
        .run(&lookup.command, timeout)
        .map_err(|e| failure(provider, &secret.name, &lookup.command, &e, timeout))?;
    value_from(provider, &secret.name, &lookup.read, &out)
}

/// The variables of `secrets` that are resolved: enabled, with a name.
pub fn active(secrets: &ExternalSecrets) -> impl Iterator<Item = &ExternalSecret> {
    secrets
        .variables
        .iter()
        .filter(|v| v.enabled && !v.name.trim().is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    /// [`provider_label`] (the model type is not `Hash`).
    provider: &'static str,
    reference: String,
    name: String,
}

impl CacheKey {
    fn new(provider: SecretProvider, secret: &ExternalSecret) -> Self {
        Self {
            provider: provider_label(provider),
            reference: secret.reference.trim().to_owned(),
            name: secret.name.clone(),
        }
    }
}

/// Resolves external secrets with a [`CommandRunner`] and keeps the values in memory for the
/// session (never on disk). The key is provider, reference and variable name, so editing a
/// reference reads it anew.
pub struct SecretResolver {
    runner: Arc<dyn CommandRunner>,
    timeout: Duration,
    cache: Mutex<HashMap<CacheKey, String>>,
}

impl std::fmt::Debug for SecretResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the values.
        f.debug_struct("SecretResolver")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl SecretResolver {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self::with_timeout(runner, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(runner: Arc<dyn CommandRunner>, timeout: Duration) -> Self {
        Self {
            runner,
            timeout,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, String>> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The values of the active variables of `secrets`, except those named in `skip` (they
    /// have a value already, e.g. from the keychain). Reads what is not cached yet; stops at
    /// the first error. Blocks while a command runs: call it off the UI thread.
    pub fn resolve(
        &self,
        secrets: &ExternalSecrets,
        skip: &[String],
    ) -> Result<Vec<(String, String)>, String> {
        let mut values = Vec::new();
        for secret in active(secrets).filter(|s| !skip.contains(&s.name)) {
            let key = CacheKey::new(secrets.provider, secret);
            let cached = self.cache().get(&key).cloned();
            let value = match cached {
                Some(v) => v,
                None => {
                    let v =
                        resolve_one(self.runner.as_ref(), secrets.provider, secret, self.timeout)?;
                    self.cache().insert(key, v.clone());
                    v
                }
            };
            values.push((secret.name.clone(), value));
        }
        Ok(values)
    }

    /// The values that are cached already, without running anything (for previews).
    pub fn cached(&self, secrets: &ExternalSecrets) -> Vec<(String, String)> {
        let cache = self.cache();
        active(secrets)
            .filter_map(|s| {
                cache
                    .get(&CacheKey::new(secrets.provider, s))
                    .map(|v| (s.name.clone(), v.clone()))
            })
            .collect()
    }

    /// Forgets every value.
    pub fn clear(&self) {
        self.cache().clear();
    }
}

/// Native runner: starts the provider's program from `PATH` (no shell), with no input, and
/// stops it at the timeout. On Windows the Azure and Google Cloud CLIs are `.cmd` scripts.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Default)]
pub struct ProcessRunner {
    /// Look for the programs only in this folder (tests); `None`: `PATH`.
    pub dir: Option<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ProcessRunner {
    fn program(&self, name: &str) -> std::path::PathBuf {
        let file = if cfg!(windows) && matches!(name, "az" | "gcloud") {
            format!("{name}.cmd")
        } else {
            name.to_owned()
        };
        match &self.dir {
            Some(dir) => dir.join(file),
            None => std::path::PathBuf::from(file),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CommandRunner for ProcessRunner {
    fn run(&self, command: &SecretCommand, timeout: Duration) -> Result<Vec<u8>, RunError> {
        use std::io::Read as _;
        use std::process::{Command, Stdio};

        let mut child = Command::new(self.program(command.program))
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => RunError::NotFound,
                _ => RunError::Other(e.to_string()),
            })?;
        // Both pipes are read on their own threads, so a chatty program cannot block on a
        // full pipe while we wait for it.
        fn drain(
            pipe: Option<impl std::io::Read + Send + 'static>,
            limit: usize,
        ) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let result = match pipe {
                    Some(p) => p.take(limit as u64 + 1).read_to_end(&mut buf).map(|_| buf),
                    None => Ok(buf),
                };
                let _ = tx.send(result);
            });
            rx
        }
        let stdout = drain(child.stdout.take(), MAX_OUTPUT);
        let stderr = drain(child.stderr.take(), 64 * 1024);
        let started = web_time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return Err(RunError::Other(e.to_string())),
            }
        };
        // The program has exited; its pipes close unless it left a child holding them.
        let rest = timeout.saturating_sub(started.elapsed()) + Duration::from_secs(1);
        let out = stdout
            .recv_timeout(rest)
            .map_err(|_| RunError::Timeout)?
            .map_err(|e| RunError::Other(e.to_string()))?;
        let err = stderr
            .recv_timeout(Duration::from_secs(1))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        if out.len() > MAX_OUTPUT {
            return Err(RunError::Other("the output is too large".into()));
        }
        if status.success() {
            Ok(out)
        } else {
            Err(RunError::Failed {
                code: status.code(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests;
