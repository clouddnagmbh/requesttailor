// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-cli`: the `rt` command line: `rt send` (wave 2), the companion `rt agent` (wave 3),
//! `rt import` (wave 4) and `rt scan` (wave 5).
//!
//! Wave 5: `rt send` runs with the same services as the apps (`services`): plugins (strand A,
//! `plugins.rs`), OAuth 2.0 token cache and terminal prompts (strand C, `oauth.rs`), cookie
//! jar (strand D, `cookies.rs`), external secrets (strand E, `secrets.rs`); `rt scan` is
//! strand E's (`scan.rs`).
//! The MCP server follows in a later wave.
//!
//! Everything the binary does is here, so it can be tested without spawning a process:
//! argument parsing, finding the collection root, the file-system [`FileSource`], exit codes.

pub mod agent;
pub mod cookies;
pub mod import;
pub mod oauth;
pub mod plugins;
pub mod run;
pub mod scan;
pub mod secrets;
pub mod services;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{REPORT_VERSION, SendError, SendOptions, SendReport, send_with};
use rt_core::transport::Transport;
use rt_format::{COLLECTION_FILE, CollectionDoc, FileSet, read_collection};

/// A response was received, whatever its HTTP status.
pub const EXIT_OK: i32 = 0;
/// No response: connection, DNS, TLS, timeout, redirect limit.
pub const EXIT_TRANSPORT: i32 = 1;
/// Bad arguments, no collection, unreadable or invalid files, request not in the collection.
pub const EXIT_USAGE: i32 = 2;
/// The request could not be prepared: environment, variables, auth or body.
pub const EXIT_PREPARE: i32 = 3;
/// A response was received but a test or assertion failed (wave 7; also `rt run`).
pub const EXIT_TESTS: i32 = 4;

pub fn help() -> String {
    let limit = SendOptions::default().body_limit;
    format!(
        "\
rt {version}: send requests from OpenCollection (YAML) files

Usage:
  rt send <request-file> [options]
  rt agent [--port <PORT>] [--allow-origin <ORIGIN>]... [--allow-secrets]
  rt import <file>... --out <dir> [--format <FORMAT>] [--json]
  rt scan [<path>...] [--json]
  rt --help | --version

Options for send:
  -e, --env <NAME>          Use the environment named NAME
      --var <NAME=VALUE>    Set a runtime variable (highest precedence); repeatable
      --body-limit <BYTES>  Keep at most BYTES of the response body (default {limit})
      --pretty              Indent the JSON report
      --plugin <FILE.wasm>  Also run this plugin; repeatable
      --no-plugins          Do not run the installed plugins
      --cookie-jar <FILE>   Send cookies from FILE and store new ones in it
      --no-prompt           Fail instead of asking for an OAuth 2.0 login
      --no-scripts          Do not run the collection's scripts
  -h, --help                Show this help

rt agent runs the companion for the web app on 127.0.0.1 until stopped:
      --port <PORT>           Listen on PORT (default {agent_port})
      --allow-origin <ORIGIN> Web app origin that may pair and send; repeatable
                              (default {agent_origins})
      --allow-secrets         Let paired origins read external secrets (1Password,
                              Vault, AWS, Azure, Google Cloud) with your CLI logins

rt scan finds credentials typed into collection files (tokens, keys, passwords
in auth, headers, URLs and bodies), for pipelines and as a pre-commit hook:
      --json                Print {{\"scanned\": N, \"findings\": [...]}} instead of text
  Paths are files or folders (default: the current folder). Values that are
  only {{{{variables}}}} are fine; a line containing `rt-scan:allow` (e.g. as a
  `# rt-scan:allow` comment) is skipped. Exit code {EXIT_OK} when clean, {scan_found} with
  findings, {EXIT_USAGE} if a path cannot be read. See crates/rt-cli/README.md for a hook.

External secrets: when the environment of rt send reads secrets from a manager
(externalSecrets), rt runs its CLI (op, vault, aws, az, gcloud) with your login.
Variables given with --var are not read from the manager.

rt import converts exports of other tools into a new collection folder:
  -o, --out <DIR>           Folder to create (must not exist, or be empty)
  -f, --format <FORMAT>     postman, insomnia, openapi, curl, har or http
                            (default: detected from each file)
      --json                Print the loss report as JSON instead of text
  Several files form one collection: a Postman collection with its environment
  and globals files, .http files with http-client(.private).env.json, or
  several cURL files. Secret values are never written; set them in
  Environments. Exit code {EXIT_OK} once the collection is written, even if the report
  lists losses; {EXIT_USAGE} if the files cannot be imported.

The collection root is the nearest folder above <request-file> that contains
{COLLECTION_FILE}.

Output:
  The send report as JSON on stdout (report version {REPORT_VERSION}), also on errors
  that happen after the collection was read. Secret values are shown as ***.
  Other diagnostics go to stderr.

Exit codes:
  {EXIT_OK}  a response was received, whatever its HTTP status
  {EXIT_TRANSPORT}  transport error, no response (connect, DNS, TLS, timeout, redirects)
  {EXIT_USAGE}  usage error, no collection, invalid files, or request not in the collection
  {EXIT_PREPARE}  the request could not be prepared (environment, variables, auth, body),
     authentication failed, a plugin or a pre-request script stopped it
  {EXIT_TESTS}  a response was received but a test or assertion failed
",
        version = env!("CARGO_PKG_VERSION"),
        agent_port = agent::DEFAULT_PORT,
        agent_origins = agent::DEFAULT_ORIGINS.join(" and "),
        scan_found = scan::EXIT_FOUND,
    )
}

pub fn version() -> String {
    format!("rt {}\n", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Send(SendArgs),
    Agent(agent::AgentArgs),
    Import(import::ImportArgs),
    Scan(scan::ScanArgs),
    /// Wave 7, strand E.
    Run(run::RunArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendArgs {
    pub file: PathBuf,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
    pub body_limit: usize,
    pub pretty: bool,
    /// `--plugin <FILE.wasm>`, repeatable: run these plugins in addition to the installed
    /// ones (wave 5, strand A).
    pub plugins: Vec<PathBuf>,
    /// `--no-plugins`: do not load the installed plugins.
    pub no_plugins: bool,
    /// `--cookie-jar <FILE>`: read cookies from and store them in FILE (wave 5, strand D).
    pub cookie_jar: Option<PathBuf>,
    /// `false` with `--no-prompt`: OAuth 2.0 flows that need the user fail instead of opening
    /// the browser or showing a device code (wave 5, strand C).
    pub prompt: bool,
    /// `--no-scripts`: do not run the collection's scripts (wave 7). Declarative assertions
    /// still run.
    pub no_scripts: bool,
}

impl Default for SendArgs {
    fn default() -> Self {
        Self {
            file: PathBuf::new(),
            environment: None,
            vars: Vec::new(),
            body_limit: SendOptions::default().body_limit,
            pretty: false,
            plugins: Vec::new(),
            no_plugins: false,
            cookie_jar: None,
            prompt: true,
            no_scripts: false,
        }
    }
}

/// Parses the arguments after the program name.
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    use lexopt::prelude::*;

    let mut parser = lexopt::Parser::from_args(args);
    let err = |e: lexopt::Error| e.to_string();
    match parser.next().map_err(err)? {
        None => Err("missing command".into()),
        Some(Short('h') | Long("help")) => Ok(Command::Help),
        Some(Short('V') | Long("version")) => Ok(Command::Version),
        Some(Value(cmd)) if cmd == "send" => parse_send(&mut parser),
        Some(Value(cmd)) if cmd == "import" => {
            Ok(import::parse(&mut parser)?.map_or(Command::Help, Command::Import))
        }
        Some(Value(cmd)) if cmd == "agent" => {
            Ok(agent::parse(&mut parser)?.map_or(Command::Help, Command::Agent))
        }
        Some(Value(cmd)) if cmd == "run" => {
            Ok(run::parse(&mut parser)?.map_or(Command::Help, Command::Run))
        }
        Some(Value(cmd)) if cmd == "scan" => {
            Ok(scan::parse(&mut parser)?.map_or(Command::Help, Command::Scan))
        }
        Some(Value(cmd)) => Err(format!("unknown command {:?}", cmd.to_string_lossy())),
        Some(other) => Err(other.unexpected().to_string()),
    }
}

fn parse_send(parser: &mut lexopt::Parser) -> Result<Command, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut file = None;
    let mut args = SendArgs::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(Command::Help),
            Short('e') | Long("env") => {
                args.environment = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("body-limit") => {
                args.body_limit = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("pretty") => args.pretty = true,
            Long("plugin") => args.plugins.push(parser.value().map_err(err)?.into()),
            Long("no-plugins") => args.no_plugins = true,
            Long("cookie-jar") => args.cookie_jar = Some(parser.value().map_err(err)?.into()),
            Long("no-prompt") => args.prompt = false,
            Long("no-scripts") => args.no_scripts = true,
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("send: missing <request-file>")?;
    Ok(Command::Send(args))
}

/// Finds the collection root above `file` and the request path relative to it,
/// `/`-separated on every OS.
pub fn find_root(file: &Path) -> Result<(PathBuf, String), String> {
    let file =
        std::fs::canonicalize(file).map_err(|e| format!("cannot open {}: {e}", file.display()))?;
    if !file.is_file() {
        return Err(format!("{} is not a file", file.display()));
    }
    let root = file
        .ancestors()
        .skip(1)
        .find(|dir| dir.join(COLLECTION_FILE).is_file())
        .ok_or_else(|| {
            format!(
                "no {COLLECTION_FILE} in any folder above {}",
                file.display()
            )
        })?
        .to_path_buf();
    let relative = file.strip_prefix(&root).expect("root is an ancestor");
    let mut parts = Vec::new();
    for c in relative.components() {
        match c {
            Component::Normal(p) => parts.push(
                p.to_str()
                    .ok_or_else(|| format!("path is not UTF-8: {}", relative.display()))?,
            ),
            _ => return Err(format!("unexpected path {}", relative.display())),
        }
    }
    Ok((root, parts.join("/")))
}

/// Reads and parses the collection below `root`.
pub fn load_collection(root: &Path) -> Result<CollectionDoc, String> {
    let files =
        FileSet::from_dir(root).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
    read_collection(&files).map_err(|e| format!("{}: {e}", root.display()))
}

/// Body files from the collection folder. Paths are relative to the root, `/`-separated;
/// absolute paths, `..` and symlinks that lead outside the root are refused.
#[derive(Debug, Clone)]
pub struct DirFiles {
    root: PathBuf,
}

impl DirFiles {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        Ok(Self {
            root: std::fs::canonicalize(root)?,
        })
    }

    /// The collection root (canonical).
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl FileSource for DirFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        // `\` and drive letters count everywhere, so a collection behaves the same on every OS.
        if path.starts_with(['/', '\\']) || path.contains(':') {
            return Err("absolute paths are not allowed".into());
        }
        let mut full = self.root.clone();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => return Err("path leaves the collection folder".into()),
                p => full.push(p),
            }
        }
        // Symlinks may still point outside.
        let real = std::fs::canonicalize(&full).map_err(|e| e.to_string())?;
        if !real.starts_with(&self.root) {
            return Err("path leaves the collection folder".into());
        }
        std::fs::read(&real).map_err(|e| e.to_string())
    }
}

/// Sends one request of a loaded collection, with the services `args` ask for (plugins,
/// token cache and prompts, cookie jar) and external secrets resolved.
pub async fn send_request<T: Transport>(
    doc: &CollectionDoc,
    request_path: &str,
    args: &SendArgs,
    files: &DirFiles,
    transport: &T,
) -> SendReport {
    let input = PrepareInput {
        environment: args.environment.as_deref(),
        globals: Vec::new(),
        runtime: args.vars.clone(),
    };
    let options = SendOptions {
        iteration: None,
        body_limit: args.body_limit,
    };
    let collection =
        match secrets::resolve(doc.model(), args.environment.as_deref(), &args.vars).await {
            Ok(c) => c,
            Err(e) => return services::failed(request_path, args, SendError::Auth(e)),
        };
    let cli = match services::CliServices::new(args, files.root()) {
        Ok(s) => s,
        Err(e) => return services::failed(request_path, args, SendError::Plugin(e)),
    };
    send_with(
        &collection,
        request_path,
        &input,
        files,
        transport,
        &options,
        &cli.services(),
    )
    .await
}

pub fn exit_code(report: &SendReport) -> i32 {
    match &report.error {
        None if !rt_core::runner::passed(report) => EXIT_TESTS,
        None => EXIT_OK,
        Some(SendError::Transport(_)) => EXIT_TRANSPORT,
        Some(SendError::NotFound(_)) => EXIT_USAGE,
        Some(
            SendError::Prepare(_)
            | SendError::Auth(_)
            | SendError::Plugin(_)
            | SendError::Script(_),
        ) => EXIT_PREPARE,
    }
}

pub fn render(report: &SendReport, pretty: bool) -> String {
    let mut out = if pretty {
        serde_json::to_string_pretty(report)
    } else {
        serde_json::to_string(report)
    }
    .expect("report serializes");
    out.push('\n');
    out
}

/// The whole program: parses `args` (without the program name), runs, returns the exit code.
pub fn run<I>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let args = match parse_args(args) {
        Ok(Command::Help) => {
            let _ = stdout.write_all(help().as_bytes());
            return EXIT_OK;
        }
        Ok(Command::Version) => {
            let _ = stdout.write_all(version().as_bytes());
            return EXIT_OK;
        }
        Ok(Command::Send(args)) => args,
        Ok(Command::Agent(args)) => return agent::run(args, stderr),
        Ok(Command::Import(args)) => return import::run(args, stdout, stderr),
        Ok(Command::Scan(args)) => return scan::run(args, stdout, stderr),
        Ok(Command::Run(args)) => return run::run(args, stdout, stderr),
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{e}\nTry 'rt --help'.")),
    };
    let (root, request_path) = match find_root(&args.file) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let doc = match load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let files = match DirFiles::new(&root) {
        Ok(f) => f,
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{}: {e}", root.display())),
    };
    let transport = match rt_core::transport::native::NativeTransport::new() {
        Ok(t) => t,
        Err(e) => return fail(stderr, EXIT_TRANSPORT, &e.to_string()),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return fail(
                stderr,
                EXIT_TRANSPORT,
                &format!("cannot start runtime: {e}"),
            );
        }
    };
    let report = runtime.block_on(send_request(&doc, &request_path, &args, &files, &transport));
    let _ = stdout.write_all(render(&report, args.pretty).as_bytes());
    let _ = stdout.flush();
    exit_code(&report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_args(args: &[&str]) -> SendArgs {
        match parse_args(args.iter().copied()) {
            Ok(Command::Send(a)) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_send() {
        let a = send_args(&[
            "send",
            "a/b.yml",
            "--env",
            "Local",
            "--var",
            "x=1",
            "--var=y=a=b",
            "--var",
            "z=",
            "--body-limit",
            "100",
            "--pretty",
        ]);
        assert_eq!(a.file, PathBuf::from("a/b.yml"));
        assert_eq!(a.environment.as_deref(), Some("Local"));
        assert_eq!(
            a.vars,
            [
                ("x".to_owned(), "1".to_owned()),
                ("y".to_owned(), "a=b".to_owned()),
                ("z".to_owned(), String::new()),
            ]
        );
        assert_eq!(a.body_limit, 100);
        assert!(a.pretty);
        let d = send_args(&["send", "-e", "E", "f.yml"]);
        assert_eq!(d.environment.as_deref(), Some("E"));
        assert_eq!(d.body_limit, SendOptions::default().body_limit);
        assert!(!d.pretty);
    }

    #[test]
    fn rejects_bad_arguments() {
        for bad in [
            &[][..],
            &["frobnicate"],
            &["--nope"],
            &["send"],
            &["send", "a.yml", "b.yml"],
            &["send", "a.yml", "--var", "novalue"],
            &["send", "a.yml", "--var", "=v"],
            &["send", "a.yml", "--body-limit", "lots"],
            &["send", "a.yml", "--env"],
        ] {
            assert!(parse_args(bad.iter().copied()).is_err(), "{bad:?}");
        }
        assert_eq!(parse_args(["--version"]), Ok(Command::Version));
        assert_eq!(parse_args(["-V"]), Ok(Command::Version));
        assert_eq!(parse_args(["-h"]), Ok(Command::Help));
        assert_eq!(parse_args(["send", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn help_documents_exit_codes() {
        let h = help();
        for code in [
            "0  a response",
            "1  transport",
            "2  usage",
            "3  the request",
        ] {
            assert!(h.contains(code), "{code}");
        }
    }

    fn temp_collection(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rt-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("col/users/admin")).unwrap();
        std::fs::write(
            dir.join("col").join(COLLECTION_FILE),
            "opencollection: 1.0.0\n",
        )
        .unwrap();
        std::fs::write(dir.join("col/users/admin/get.yml"), "x").unwrap();
        std::fs::write(dir.join("col/body.txt"), "payload").unwrap();
        std::fs::write(dir.join("outside.txt"), "secret").unwrap();
        dir
    }

    #[test]
    fn finds_the_collection_root() {
        let dir = temp_collection("root");
        let (root, path) = find_root(&dir.join("col/users/admin/get.yml")).unwrap();
        assert_eq!(root, std::fs::canonicalize(dir.join("col")).unwrap());
        assert_eq!(path, "users/admin/get.yml");
        assert!(find_root(&dir.join("outside.txt")).is_err());
        assert!(find_root(&dir.join("col/missing.yml")).is_err());
        assert!(find_root(&dir.join("col/users")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_source_stays_inside_the_root() {
        let dir = temp_collection("files");
        let files = DirFiles::new(&dir.join("col")).unwrap();
        assert_eq!(files.read("body.txt").unwrap(), b"payload");
        assert_eq!(files.read("./body.txt").unwrap(), b"payload");
        assert!(files.read("../outside.txt").is_err());
        assert!(files.read("users/../../outside.txt").is_err());
        assert!(files.read("users\\..\\..\\outside.txt").is_err());
        let abs = dir.join("outside.txt");
        assert!(files.read(abs.to_str().unwrap()).is_err());
        assert!(files.read("missing.txt").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("outside.txt"), dir.join("col/link.txt")).unwrap();
            assert!(files.read("link.txt").is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn exit_codes() {
        use rt_core::transport::TransportError;
        let mut r = SendReport {
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            notes: Vec::new(),
            version: REPORT_VERSION,
            request_path: "a.yml".into(),
            environment: None,
            request: None,
            response: None,
            variables: vec![],
            unresolved: vec![],
            error: None,
        };
        assert_eq!(exit_code(&r), EXIT_OK);
        r.error = Some(SendError::Transport(TransportError::Timeout));
        assert_eq!(exit_code(&r), EXIT_TRANSPORT);
        r.error = Some(SendError::NotFound("a.yml".into()));
        assert_eq!(exit_code(&r), EXIT_USAGE);
        r.error = Some(SendError::Prepare("x".into()));
        assert_eq!(exit_code(&r), EXIT_PREPARE);
        assert!(render(&r, false).ends_with("}\n"));
        assert!(render(&r, true).contains("\n  \"version\": 1"));
    }

    #[test]
    fn usage_errors_go_to_stderr() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(["send"], &mut out, &mut err), EXIT_USAGE);
        assert!(out.is_empty());
        assert!(String::from_utf8(err).unwrap().starts_with("rt: "));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(["--version"], &mut out, &mut err), EXIT_OK);
        assert_eq!(String::from_utf8(out).unwrap(), version());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            run(["send", "/definitely/not/here.yml"], &mut out, &mut err),
            EXIT_USAGE
        );
    }
}
