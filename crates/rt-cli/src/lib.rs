// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-cli`: the `rt` command line: `rt send` (wave 2), the companion `rt agent` (wave 3),
//! `rt import` (wave 4), `rt scan` (wave 5) and `rt run` (wave 7, `run.rs`, reporters in
//! `report/`).
//!
//! Wave 5: `rt send` runs with the same services as the apps (`services`): plugins (strand A,
//! `plugins.rs`), OAuth 2.0 token cache and terminal prompts (strand C, `oauth.rs`), cookie
//! jar (strand D, `cookies.rs`), external secrets (strand E, `secrets.rs`); `rt scan` is
//! strand E's (`scan.rs`).
//! The MCP server follows in a later wave.
//!
//! Everything the binary does is here, so it can be tested without spawning a process:
//! argument parsing, finding the collection root, the file-system [`FileSource`], exit codes.

/// Wave 19, strand D: `rt access check`.
pub mod access;
pub mod agent;
pub mod approvals;
/// Wave 12, strand C: `rt bisect`.
pub mod bisect;
/// Wave 12, strand F: `rt changelog`.
pub mod changelog;
/// Wave 18, strand B: `rt send --checks`.
pub mod checks;
/// Wave 18, strand A: `rt compare-env`.
pub mod compare_env;
pub mod cookies;
/// Wave 19, strand Y: `rt send --debug`.
pub mod debug_flag;
/// Wave 12, strand A: `rt diff`.
pub mod diff;
pub mod docs;
/// Wave 8, strand B: `rt send` on gRPC requests.
pub mod grpc;
pub mod import;
/// Wave 18, strand E: `rt inbox`.
pub mod inbox;
/// Wave 19, strand C: `rt journal`.
pub mod journal;
/// Wave 18, strand F: `rt lint`.
pub mod lint;
pub mod mcp;
/// Wave 14: `rt mcp-demo`, the MCP demo server.
pub mod mcp_demo;
pub mod mcp_test;
/// Wave 12, strand B: `rt merge-driver`.
pub mod merge_driver;
pub mod mock;
/// Wave 19, strand W: `rt notebook`.
pub mod notebook;
pub mod oauth;
/// Wave 18, strand C: `rt send --all-pages`.
pub mod paging;
/// Wave 18, strand I: `rt plugin`.
pub mod plugin_cmd;
pub mod plugins;
pub mod report;
pub mod run;
pub mod scan;
pub mod scripts;
pub mod secrets;
/// Wave 19, strands R and S: `rt security audit`.
pub mod security_cmd;
pub mod send_banner;
/// Wave 19, strand G: `--sequence` of `rt send`, `rt run` and `rt mcp`.
pub mod sequence_flag;
pub mod services;
/// Wave 19, strand E: `rt shrink`.
pub mod shrink;
pub mod snapshot;
/// `rt send` on WebSocket, gRPC and SSE requests (wave 8, strand A).
pub mod stream;
/// Wave 18, strand G: `rt send --view`.
/// Wave 19, strand A: `rt send --trace`.
pub mod trace_flag;
/// Wave 19, strand F: `rt verify-consumers`.
pub mod verify_consumers;
pub mod views;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_core::guard::{Caller, GuardHandle};
use rt_core::observe::ObserverSet;
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
/// Wave 10: the send guard of the distribution refused a request; nothing was sent.
pub const EXIT_BLOCKED: i32 = 5;
/// Wave 18: `rt lint` found what it was told to fail on (a finding at `--fail-on`, or a score
/// below `--min-score`). 6 is taken by distributions (`EXIT_LOAD`).
pub const EXIT_LINT: i32 = lint::EXIT_LINT;
/// Wave 19: `rt verify-consumers` found a consumer that breaks. (`rt access check` uses
/// `EXIT_TESTS` for broken access.)
pub const EXIT_CONSUMERS: i32 = verify_consumers::EXIT_CONSUMERS;
/// Wave 19: `rt security audit` found a finding at or above `--fail-on`.
pub const EXIT_AUDIT: i32 = security_cmd::EXIT_AUDIT;

pub fn help() -> String {
    let limit = SendOptions::default().body_limit;
    format!(
        "\
rt {version}: send requests from OpenCollection (YAML) files

Usage:
  rt send <request-file> [options]
  rt run [<folder>] [options]
  rt agent [--port <PORT>] [--allow-origin <ORIGIN>]... [--allow-secrets]
  rt import <file>... --out <dir> [--format <FORMAT>] [--json]
  rt import openapi <file|url>... --out <dir> [--names operation-id]
            [--group tag|path|flat] [--keep-spec] [--json]
  rt scan [<path>...] [--json]
  rt snapshot update [<path>...] [--all] [-e <NAME>] [--var <NAME=VALUE>]...
  rt mcp-test [<path>...] [-e <NAME>] [--var <NAME=VALUE>]...
{mcp_demo_usage}{git_usage}{check_usage}{insight_usage}  rt --help | --version

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
      --timing              Also print one timing bar with the share of server wait (stderr)
      --watch               Send again whenever the request, its body file, the folder files,
                            opencollection.yml, the environment or a companion file changes;
                            prints one line per send (--json: one report per line). Only
                            reading requests, and never against production, unless
                            --allow-writes (never against production). Ctrl-C exits with the
                            code of the last send.
      --all-pages           Follow the pages of a reading request and merge them (rule of the
                            request, the collection or a plugin; limits always apply)
      --max-pages <N>       With --all-pages: at most N pages (default 20, hard maximum 1000)
      --checks              Passive checks of the response (headers, cookies, CORS, tokens),
                            one line per observation on stderr and in the report's checks
      --view <NAME>         Print what the saved view NAME of the request picks, not the report
      --view-expr <EXPR>    The same with an expression (JSONPath) of your own
      --respect-limits      Wait for rate limits (also with rt run); --max-wait <T> bounds one
                            wait (default 60s, hard maximum 600s)
      --max-wait <T>        See --respect-limits
{trace_help}{sequence_help}{debug_help}      --allow-writes        With --watch: also send writing requests (not to production)
  -h, --help                Show this help

Against a production environment (the collection's agent policy, or a name with prod, prd or
live) rt send and rt run print a PRODUCTION: line on stderr and add it to the report's notes.
They never refuse because of it.

Sessions (WebSocket and gRPC requests, HTTP requests with --sse): rt send opens
the session, sends the selected message and prints the message history as JSON
(StreamLog). By default it closes after the first received message or 10 s.
      --sse                 Read an HTTP request's response as Server-Sent Events
      --message <TITLE>     Send this message variant instead of the selected one
      --duration <SECONDS>  Keep the session open this long
      --until-close         Keep the session open until the server closes it

rt run sends the requests of a collection, or of one folder of it, in order
(by seq, then name), for pipelines. <folder> defaults to the current folder.
Variables set by scripts carry over to the next requests. Options as for send
(-e, --var, --body-limit, --plugin, --no-plugins, --cookie-jar, --no-prompt,
--no-scripts), and:
      --data <FILE>         CSV or JSON rows; one iteration per row, the row's
                            values as variables
      --iterations <N>      Run N times (with --data: N rows, cycling)
      --delay <MS>          Wait MS milliseconds between requests
      --bail                Stop at the first failed request, test or assertion
      --reporter <NAME[=FILE]>
                            text, json or junit; repeatable. Without FILE a
                            report goes to stdout (one at most). Live text is
                            always on stderr.
      --out <FILE>          Write the reporter just before it to FILE
  The JSON report is {{\"version\": 1, \"summary\": ..., \"steps\": [...]}} with
  each step's send report; JUnit has one testsuite per request and iteration
  and one testcase per test and assertion. Exit code {EXIT_OK} when everything
  passed, {EXIT_TESTS} when a request, test or assertion failed, {EXIT_USAGE} for usage
  errors, a folder without requests or a report that cannot be written.

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

rt snapshot update writes the snapshot files (<request>.snap.json) of the requests
anew from a fresh response; `rt run` and `rt send` only compare, they never write.
      --all                 Every request that has a snapshot assertion
  -e, --env <NAME>, --var <NAME=VALUE> as for send. Exit code {EXIT_OK} when written,
  {EXIT_USAGE} for usage errors.

rt mcp-test runs saved MCP tests (*.mcp.yaml, saved in the app) against their
servers: connect (stdio or Streamable HTTP), call the tool, check the assertions.
  Paths are test files or folders (default: the collection of the current
  folder). -e and --var as for send. Prints {{\"tests\": [...], \"passed\": N,
  \"failed\": M}} as JSON, secret values as ***. Exit code {EXIT_OK} when all passed,
  {EXIT_TESTS} when a test failed, {EXIT_TRANSPORT} when a server could not be reached or started.

{mcp_demo_help}
External secrets: when the environment of rt send reads secrets from a manager
(externalSecrets), rt runs its CLI (op, vault, aws, az, gcloud) with your login.
Variables given with --var are not read from the manager.

rt import converts exports of other tools into a new collection folder:
  -o, --out <DIR>           Folder to create (must not exist, or be empty)
  -f, --format <FORMAT>     postman, insomnia, openapi, curl, har or http
                            (default: detected from each file)
      --json                Print the loss report as JSON instead of text
  Several files form one collection: a Postman collection with its environment
  and globals files, .http files with http-client(.private).env.json,
  several cURL files, or a multi-file OpenAPI spec. Secret values are never
  written; set them in Environments. Exit code {EXIT_OK} once the collection is
  written, even if the report lists losses; {EXIT_USAGE} if the files cannot be
  imported.

rt import openapi imports an OpenAPI 3.x or Swagger 2.0 spec with choices:
  <file>...                 The spec; several files are one multi-file spec (the
                            files its $refs name), a .zip holds one
  <url>                     Fetch the spec and the files its relative $refs name
                            ({EXIT_TRANSPORT} if it cannot be fetched)
      --names operation-id  Name requests after the operationId (default: summary)
      --group <BY>          Folders by tag (default), path (first segment) or flat
      --keep-spec           Keep the spec in the collection (openapi.yaml/.json)
                            for the contract check

{git_help}
{check_help}
{insight_help}
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
  {EXIT_BLOCKED}  a policy of this installation refused the request; nothing was sent
  {EXIT_LINT}  rt lint found what it was told to fail on
  {EXIT_CONSUMERS}  rt verify-consumers found a consumer that breaks
  {EXIT_AUDIT}  rt security audit found a finding at or above --fail-on
",
        version = env!("CARGO_PKG_VERSION"),
        agent_port = agent::DEFAULT_PORT,
        agent_origins = agent::DEFAULT_ORIGINS.join(" and "),
        scan_found = scan::EXIT_FOUND,
        mcp_demo_usage = mcp_demo::USAGE,
        mcp_demo_help = mcp_demo::help(),
        // Wave 12: each command's usage and help live in its own module.
        git_usage = [
            diff::USAGE,
            merge_driver::USAGE,
            bisect::USAGE,
            changelog::USAGE
        ]
        .concat(),
        check_usage = [
            compare_env::USAGE,
            lint::USAGE,
            inbox::USAGE,
            plugin_cmd::USAGE
        ]
        .concat(),
        insight_usage = [
            access::USAGE,
            shrink::USAGE,
            verify_consumers::USAGE,
            journal::USAGE,
            security_cmd::USAGE,
            notebook::USAGE
        ]
        .concat(),
        insight_help = [
            access::HELP,
            shrink::HELP,
            verify_consumers::HELP,
            journal::HELP,
            security_cmd::HELP,
            notebook::HELP
        ]
        .join("\n"),
        trace_help = trace_flag::HELP,
        debug_help = debug_flag::HELP,
        sequence_help = sequence_flag::HELP,
        check_help = [compare_env::HELP, lint::HELP, inbox::HELP, plugin_cmd::HELP].join("\n"),
        git_help = [
            diff::HELP,
            merge_driver::HELP,
            bisect::HELP,
            changelog::HELP
        ]
        .join("\n"),
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
    /// Wave 17, strand B: `rt snapshot update`.
    Snapshot(snapshot::SnapshotArgs),
    /// Wave 7, strand E.
    Run(run::RunArgs),
    /// Wave 8, strand C: `rt mcp`.
    Mcp(mcp::Args),
    /// Wave 8, strand C: `rt approvals`, `rt approve <id>`, `rt deny <id>`.
    Approvals(approvals::Args),
    Approve(approvals::Args),
    Deny(approvals::Args),
    /// Wave 8, strand D: `rt mcp-test`.
    McpTest(mcp_test::Args),
    /// Wave 14: `rt mcp-demo`.
    McpDemo(mcp_demo::Args),
    /// Wave 8, strand F: `rt mock`, `rt docs`.
    Mock(mock::Args),
    Docs(docs::Args),
    /// Wave 12, strand A: `rt diff`.
    Diff(diff::Args),
    /// Wave 12, strand B: `rt merge-driver`.
    MergeDriver(merge_driver::Args),
    /// Wave 12, strand C: `rt bisect`.
    Bisect(bisect::Args),
    /// Wave 12, strand F: `rt changelog`.
    Changelog(changelog::Args),
    /// Wave 18, strand A: `rt compare-env`.
    CompareEnv(compare_env::Args),
    /// Wave 18, strand F: `rt lint`.
    Lint(lint::Args),
    /// Wave 18, strand E: `rt inbox`.
    Inbox(inbox::Args),
    /// Wave 18, strand I: `rt plugin`.
    Plugin(plugin_cmd::Args),
    /// Wave 19, strand D: `rt access check`.
    Access(access::Args),
    /// Wave 19, strand E: `rt shrink`.
    Shrink(shrink::Args),
    /// Wave 19, strand F: `rt verify-consumers`.
    VerifyConsumers(verify_consumers::Args),
    /// Wave 19, strand C: `rt journal`.
    Journal(journal::Args),
    /// Wave 19, strands R and S: `rt security audit`.
    Security(security_cmd::Args),
    /// Wave 19, strand W: `rt notebook run`.
    Notebook(notebook::Args),
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
    /// Session options (wave 8): `--sse`, `--message`, `--duration`, `--until-close`.
    pub session: stream::SessionArgs,
    /// Wave 10: the distribution's send guard (never from the command line; see
    /// [`run_distribution`]).
    pub guard: GuardHandle,
    /// Wave 17: the distribution's exchange observers (never from the command line; see
    /// [`run_distribution`]).
    pub observers: ObserverSet,
    /// Wave 10: who sends, for the guard: [`Caller::Cli`], or [`Caller::Agent`] in `rt mcp`.
    pub caller: Caller,
    /// Wave 17: `--watch`, `--allow-writes`, `--timing`, `--json` (`watch.rs`).
    pub watch: watch::WatchArgs,
    /// Wave 18: `--all-pages`, `--max-pages` (`paging.rs`).
    pub paging: paging::PagingArgs,
    /// Wave 18: `--checks` (`checks.rs`).
    pub checks: bool,
    /// Wave 18: `--view`, `--view-expr` (`views.rs`).
    pub view: views::ViewArgs,
    /// Wave 18: `--respect-limits`, `--max-wait` (used by the page follower and `rt run`).
    pub throttle: rt_core::ratelimit::Throttle,
    /// Wave 19: `--trace` (`trace_flag.rs`).
    pub trace: bool,
    /// Wave 19: `--sequence`, `--sequence-values` (`sequence_flag.rs`).
    pub sequence: sequence_flag::SequenceArgs,
    /// Wave 19: `--debug`, `--break` (`debug_flag.rs`).
    pub debug: debug_flag::DebugArgs,
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
            session: stream::SessionArgs::default(),
            guard: GuardHandle::none(),
            observers: ObserverSet::new(),
            caller: Caller::Cli,
            watch: watch::WatchArgs::default(),
            paging: paging::PagingArgs::default(),
            checks: false,
            view: views::ViewArgs::default(),
            throttle: rt_core::ratelimit::Throttle::default(),
            trace: false,
            sequence: Default::default(),
            debug: Default::default(),
        }
    }
}

/// Parses the arguments after the program name.
pub fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    parse_args_with(args, &[])
}

/// [`parse_args`] with the changelog writers of a distribution.
pub fn parse_args_with<I>(
    args: I,
    changelog_writers: &[&dyn rt_app::contract::ChangelogWriter],
) -> Result<Command, String>
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
        Some(Value(cmd)) if cmd == "snapshot" => {
            Ok(snapshot::parse(&mut parser)?.map_or(Command::Help, Command::Snapshot))
        }
        Some(Value(cmd)) if cmd == "mcp" => {
            Ok(mcp::parse(&mut parser)?.map_or(Command::Help, Command::Mcp))
        }
        Some(Value(cmd)) if cmd == "approvals" => {
            Ok(approvals::parse(&mut parser)?.map_or(Command::Help, Command::Approvals))
        }
        Some(Value(cmd)) if cmd == "approve" => {
            Ok(approvals::parse(&mut parser)?.map_or(Command::Help, Command::Approve))
        }
        Some(Value(cmd)) if cmd == "deny" => {
            Ok(approvals::parse(&mut parser)?.map_or(Command::Help, Command::Deny))
        }
        Some(Value(cmd)) if cmd == "mcp-test" => {
            Ok(mcp_test::parse(&mut parser)?.map_or(Command::Help, Command::McpTest))
        }
        Some(Value(cmd)) if cmd == "mcp-demo" => {
            Ok(mcp_demo::parse(&mut parser)?.map_or(Command::Help, Command::McpDemo))
        }
        Some(Value(cmd)) if cmd == "mock" => {
            Ok(mock::parse(&mut parser)?.map_or(Command::Help, Command::Mock))
        }
        Some(Value(cmd)) if cmd == "docs" => {
            Ok(docs::parse(&mut parser)?.map_or(Command::Help, Command::Docs))
        }
        Some(Value(cmd)) if cmd == "diff" => {
            Ok(diff::parse(&mut parser)?.map_or(Command::Help, Command::Diff))
        }
        Some(Value(cmd)) if cmd == "merge-driver" => {
            Ok(merge_driver::parse(&mut parser)?.map_or(Command::Help, Command::MergeDriver))
        }
        Some(Value(cmd)) if cmd == "bisect" => {
            Ok(bisect::parse(&mut parser)?.map_or(Command::Help, Command::Bisect))
        }
        Some(Value(cmd)) if cmd == "changelog" => {
            Ok(changelog::parse(&mut parser, changelog_writers)?
                .map_or(Command::Help, Command::Changelog))
        }
        Some(Value(cmd)) if cmd == "compare-env" => {
            Ok(compare_env::parse(&mut parser)?.map_or(Command::Help, Command::CompareEnv))
        }
        Some(Value(cmd)) if cmd == "lint" => {
            Ok(lint::parse(&mut parser)?.map_or(Command::Help, Command::Lint))
        }
        Some(Value(cmd)) if cmd == "inbox" => {
            Ok(inbox::parse(&mut parser)?.map_or(Command::Help, Command::Inbox))
        }
        Some(Value(cmd)) if cmd == "access" => {
            Ok(access::parse(&mut parser)?.map_or(Command::Help, Command::Access))
        }
        Some(Value(cmd)) if cmd == "shrink" => {
            Ok(shrink::parse(&mut parser)?.map_or(Command::Help, Command::Shrink))
        }
        Some(Value(cmd)) if cmd == "verify-consumers" => {
            Ok(verify_consumers::parse(&mut parser)?
                .map_or(Command::Help, Command::VerifyConsumers))
        }
        Some(Value(cmd)) if cmd == "security" => {
            Ok(security_cmd::parse(&mut parser)?.map_or(Command::Help, Command::Security))
        }
        Some(Value(cmd)) if cmd == "notebook" => {
            Ok(notebook::parse(&mut parser)?.map_or(Command::Help, Command::Notebook))
        }
        Some(Value(cmd)) if cmd == "journal" => {
            Ok(journal::parse(&mut parser)?.map_or(Command::Help, Command::Journal))
        }
        Some(Value(cmd)) if cmd == "plugin" => {
            Ok(plugin_cmd::parse(&mut parser)?.map_or(Command::Help, Command::Plugin))
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
            Long("watch") => args.watch.watch = true,
            Long("allow-writes") => args.watch.allow_writes = true,
            Long("timing") => args.watch.timing = true,
            Long("json") => args.watch.json = true,
            Long("all-pages") => args.paging.all_pages = true,
            Long("max-pages") => {
                args.paging.max_pages = Some(parser.value().map_err(err)?.parse().map_err(err)?);
            }
            Long("checks") => args.checks = true,
            Long("view") => {
                args.view.name = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Long("view-expr") => {
                args.view.expression = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("respect-limits") => {
                args.throttle.mode = rt_core::ratelimit::ThrottleMode::Respect
            }
            Long("max-wait") => {
                args.throttle.max_wait_ms =
                    crate::duration_ms(&parser.value().map_err(err)?.string().map_err(err)?)?;
            }
            Long("trace") => args.trace = true,
            Long("debug") => args.debug.debug = true,
            Long("break") => args
                .debug
                .breakpoints
                .push(parser.value().map_err(err)?.parse().map_err(err)?),
            Long(name @ ("sequence" | "sequence-values")) => {
                let name = name.to_owned();
                sequence_flag::parse_option(&name, parser, &mut args.sequence)?;
            }
            Long(name @ ("sse" | "message" | "duration" | "until-close")) => {
                let name = name.to_owned();
                stream::parse_option(&name, parser, &mut args.session)?;
            }
            Value(v) if file.is_none() => file = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    args.file = file.ok_or("send: missing <request-file>")?;
    if args.watch.allow_writes && !args.watch.watch {
        return Err("--allow-writes only applies with --watch".into());
    }
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
    let collection = match secrets::resolve_all(
        doc.model(),
        args.environment.as_deref(),
        &args.vars,
        &oauth::collection_id(files.root()),
    )
    .await
    {
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
        Some(SendError::Blocked { .. }) => EXIT_BLOCKED,
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
/// A subcommand a distribution adds to `rt` (wave 9). The builds of this repository add none.
pub struct CliExtension {
    /// The subcommand, e.g. `example`; must not shadow a built-in one.
    pub name: &'static str,
    /// One line for `rt --help`.
    pub summary: &'static str,
    /// Gets the arguments after the subcommand and returns the exit code.
    pub run: fn(Vec<OsString>, &mut dyn Write, &mut dyn Write) -> i32,
}

/// `rt` with the subcommands of a distribution: a first argument naming an extension runs it,
/// everything else is [`run`]. `rt --help` lists the extensions after the built-in commands.
pub fn run_with<I>(
    args: I,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    extensions: &[CliExtension],
) -> i32
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    run_distribution(
        args,
        stdout,
        stderr,
        &Distribution {
            extensions,
            ..Distribution::default()
        },
    )
}

/// What a distribution adds to `rt` (waves 9 and 10). The builds of this repository add
/// nothing.
#[derive(Default)]
pub struct Distribution<'a> {
    /// Subcommands ([`run_with`]).
    pub extensions: &'a [CliExtension],
    /// Asked before every request `rt send`, `rt run` and `rt mcp` send
    /// (`rt_core::guard`). A refused request is not sent: `rt send` and `rt run` exit with
    /// [`EXIT_BLOCKED`], `rt mcp` answers the tool call with the reason and records the rule
    /// in the audit log.
    pub guard: GuardHandle,
    /// Handed every finished send of `rt send`, `rt run`, `rt mcp` and `rt agent` (every
    /// `/v1/send`, without collection and request path; wave 17, `rt_core::observe`). A
    /// distribution that starts the companion itself uses [`agent::run_with`].
    /// They only ever see masked exchanges and must be quick. The builds of this repository
    /// have none.
    pub observers: ObserverSet,
    /// Sections `rt changelog` appends after the OpenAPI and collection changes (since wave
    /// 13), in this order.
    pub changelog: &'a [&'a dyn rt_app::contract::ChangelogSection],
    /// Other ways to write the changelog, each chosen with its own flag of `rt changelog`
    /// (since wave 16, `rt_app::contract::ChangelogWriter`). Without writers every
    /// such flag is an unknown flag.
    pub changelog_writers: &'a [&'a dyn rt_app::contract::ChangelogWriter],
    /// Hooks `rt mock --hook NAME[=ARG]` can switch on (since wave 18, [`mock::MockHook`]).
    /// Without entries every `--hook` is an error.
    pub mock_hooks: &'a [mock::MockHookEntry],
}

/// [`run_with`] with everything a distribution adds.
pub fn run_distribution<I>(
    args: I,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    distribution: &Distribution<'_>,
) -> i32
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let extensions = distribution.extensions;
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if let Some(x) = args
        .first()
        .and_then(|first| extensions.iter().find(|x| first.as_os_str() == x.name))
    {
        return (x.run)(args[1..].to_vec(), stdout, stderr);
    }
    let wants_help =
        matches!(args.first().and_then(|a| a.to_str()), Some("-h" | "--help")) && args.len() == 1;
    if wants_help && !extensions.is_empty() {
        let mut text = help();
        text.push_str("\nMore commands:\n");
        for x in extensions {
            text.push_str(&format!("  rt {:<12} {}\n", x.name, x.summary));
        }
        let _ = stdout.write_all(text.as_bytes());
        return EXIT_OK;
    }
    run_guarded(
        args,
        stdout,
        stderr,
        (&distribution.guard, &distribution.observers),
        (distribution.changelog, distribution.changelog_writers),
        distribution.mock_hooks,
    )
}

pub fn run<I>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    run_guarded(
        args,
        stdout,
        stderr,
        (&GuardHandle::none(), &ObserverSet::new()),
        (&[], &[]),
        &[],
    )
}

/// The parsed command with the distribution's guard and observers on every command that
/// sends.
fn with_guard(command: Command, (guard, observers): (&GuardHandle, &ObserverSet)) -> Command {
    match command {
        Command::Send(mut a) => {
            a.guard = guard.clone();
            a.observers = observers.clone();
            Command::Send(a)
        }
        Command::Run(mut a) => {
            a.guard = guard.clone();
            a.observers = observers.clone();
            Command::Run(a)
        }
        Command::Mcp(mut a) => {
            a.guard = guard.clone();
            a.observers = observers.clone();
            Command::Mcp(a)
        }
        other => other,
    }
}

fn run_guarded<I>(
    args: I,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    hooks: (&GuardHandle, &ObserverSet),
    (changelog_sections, changelog_writers): (
        &[&dyn rt_app::contract::ChangelogSection],
        &[&dyn rt_app::contract::ChangelogWriter],
    ),
    mock_hooks: &[mock::MockHookEntry],
) -> i32
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let for_changelog = args.first().is_some_and(|a| a == "changelog");
    let args = match parse_args_with(args, changelog_writers).map(|c| with_guard(c, hooks)) {
        Ok(Command::Help) => {
            let _ = stdout.write_all(help().as_bytes());
            if for_changelog {
                let _ = stdout.write_all(changelog::writers_help(changelog_writers).as_bytes());
            }
            return EXIT_OK;
        }
        Ok(Command::Version) => {
            let _ = stdout.write_all(version().as_bytes());
            return EXIT_OK;
        }
        Ok(Command::Send(args)) if args.watch.watch => {
            return watch::run(args, stdout, stderr);
        }
        Ok(Command::Send(args)) => args,
        Ok(Command::Agent(args)) => return agent::run_with(args, hooks.1.clone(), stderr),
        Ok(Command::Import(args)) => return import::run(args, stdout, stderr),
        Ok(Command::Scan(args)) => return scan::run(args, stdout, stderr),
        Ok(Command::Snapshot(args)) => return snapshot::run(args, stdout, stderr),
        Ok(Command::Run(args)) => return run::run(args, stdout, stderr),
        Ok(Command::Mcp(args)) => return mcp::run(args, stdout, stderr),
        Ok(Command::Approvals(args)) => return approvals::run(args, stdout, stderr),
        Ok(Command::Approve(args)) => {
            return approvals::decide(
                args,
                rt_app::agents::ApprovalDecision::Approved,
                stdout,
                stderr,
            );
        }
        Ok(Command::Deny(args)) => {
            return approvals::decide(
                args,
                rt_app::agents::ApprovalDecision::Denied,
                stdout,
                stderr,
            );
        }
        Ok(Command::McpTest(args)) => return mcp_test::run(args, stdout, stderr),
        Ok(Command::McpDemo(args)) => return mcp_demo::run(args, stderr),
        Ok(Command::Mock(args)) => return mock::run_with(args, mock_hooks, stdout, stderr),
        Ok(Command::Docs(args)) => return docs::run(args, stdout, stderr),
        Ok(Command::Diff(args)) => return diff::run(args, stdout, stderr),
        Ok(Command::MergeDriver(args)) => return merge_driver::run(args, stdout, stderr),
        Ok(Command::Bisect(args)) => return bisect::run(args, stdout, stderr),
        Ok(Command::Changelog(args)) => {
            return changelog::run(args, stdout, stderr, changelog_sections, changelog_writers);
        }
        Ok(Command::CompareEnv(args)) => return compare_env::run(args, stdout, stderr),
        Ok(Command::Lint(args)) => return lint::run(args, stdout, stderr),
        Ok(Command::Inbox(args)) => return inbox::run(args, stdout, stderr),
        Ok(Command::Plugin(args)) => return plugin_cmd::run(args, stdout, stderr),
        Ok(Command::Access(args)) => return access::run(args, stdout, stderr),
        Ok(Command::Shrink(args)) => return shrink::run(args, stdout, stderr),
        Ok(Command::VerifyConsumers(args)) => return verify_consumers::run(args, stdout, stderr),
        Ok(Command::Journal(args)) => return journal::run(args, stdout, stderr),
        Ok(Command::Security(args)) => return security_cmd::run(args, stdout, stderr),
        Ok(Command::Notebook(args)) => return notebook::run(args, stdout, stderr),
        Err(e) => return fail(stderr, EXIT_USAGE, &format!("{e}\nTry 'rt --help'.")),
    };
    // Wave 19: `--trace` (A) and `--sequence` (G); each refuses until its strand builds it.
    if let Err(e) = trace_flag::check(args.trace)
        .and_then(|()| sequence_flag::check(&args.sequence))
        .and_then(|()| debug_flag::check(&args.debug))
    {
        return fail(stderr, EXIT_USAGE, &e);
    }
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
    // Wave 8: WebSocket and gRPC requests, and HTTP requests with `--sse`, are sessions.
    let session = doc
        .model()
        .request(&request_path)
        .is_some_and(|c| stream::is_session(&c.request.spec, &args.session));
    if session {
        let output = runtime.block_on(stream::run_session(
            &doc,
            &request_path,
            &args,
            &files,
            &transport,
        ));
        let _ = stdout.write_all(output.render(args.pretty).as_bytes());
        let _ = stdout.flush();
        return output.exit_code();
    }
    // Wave 18: the hooks of paging (C), checks (B) and views (G); each is a plain send, a plain
    // report and the usual output until its strand fills it in.
    let paged = match runtime.block_on(paging::send_pages(
        &args,
        &doc,
        &request_path,
        &files,
        &transport,
    )) {
        Ok(p) => p,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    let mut report = match paged {
        Some(report) => report,
        None => runtime.block_on(send_request(&doc, &request_path, &args, &files, &transport)),
    };
    if let Err(e) = checks::attach(&args, &mut report, stderr) {
        return fail(stderr, EXIT_USAGE, &e);
    }
    // Wave 17: a production environment is said out loud, never refused.
    if let Some(text) =
        send_banner::banner_for(doc.model(), &request_path, args.environment.as_deref())
    {
        let _ = writeln!(stderr, "rt: {text}");
        send_banner::add_note(&mut report, &text);
    }
    if args.watch.timing
        && let Some(line) = send_banner::timing_line(&report)
    {
        let _ = writeln!(stderr, "{line}");
    }
    match views::render(&args, &report) {
        Ok(Some(text)) => {
            let _ = stdout.write_all(text.as_bytes());
        }
        Ok(None) => {
            let _ = stdout.write_all(render(&report, args.pretty).as_bytes());
        }
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    }
    let _ = stdout.flush();
    exit_code(&report)
}

/// A duration as `rt` flags write it: `90`, `90s`, `1500ms` or `2m` → milliseconds. Bare
/// numbers are seconds.
pub(crate) fn duration_ms(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (number, unit) = match text.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => text.split_at(i),
        None => (text, "s"),
    };
    let value: u64 = number
        .parse()
        .map_err(|_| format!("expected a duration like 30s, 1500ms or 2m, got {text:?}"))?;
    let factor = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        _ => {
            return Err(format!(
                "expected a duration like 30s, 1500ms or 2m, got {text:?}"
            ));
        }
    };
    Ok(value.saturating_mul(factor))
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
    fn extensions_run_their_subcommand_and_appear_in_help() {
        fn hello(args: Vec<OsString>, out: &mut dyn Write, _: &mut dyn Write) -> i32 {
            let _ = write!(out, "hello {}", args.len());
            7
        }
        let x = [CliExtension {
            name: "hello",
            summary: "say hello",
            run: hello,
        }];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run_with(["hello", "a", "b"], &mut out, &mut err, &x), 7);
        assert_eq!(String::from_utf8(out).unwrap(), "hello 2");

        let mut out = Vec::new();
        assert_eq!(run_with(["--help"], &mut out, &mut err, &x), EXIT_OK);
        let help = String::from_utf8(out).unwrap();
        assert!(help.starts_with(&super::help()));
        assert!(help.contains("rt hello"), "{help}");
        assert!(help.contains("say hello"), "{help}");

        // Without extensions `rt` is unchanged.
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert_eq!(run_with(["--help"], &mut a, &mut err, &[]), EXIT_OK);
        run(["--help"], &mut b, &mut err);
        assert_eq!(a, b);
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
            checks: Vec::new(),
            rate_limit: None,
            trace: None,
            segments: Vec::new(),
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
