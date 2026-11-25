// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp [<collection>]`: the MCP server over stdio. Collections become tools; what an agent may do comes from `extensions.requesttailor.agents`; writing requests against production wait for approval; secrets only by name; an audit entry per call (`rt_app::agents`). Tools: `list_requests`, `get_request`, `list_environments`, `send_request`, `run_folder`.
//!
//! Wave 8, strand C. The tool contract is fixed by `tests/gate_w8.rs`; the security model is
//! in `crates/rt-cli/README.md` ("MCP server").
//!
//! - `server`: JSON-RPC 2.0 over stdio (one message per line), protocol negotiation, the
//!   message loop; every outgoing line passes [`mask::Masker`].
//! - `tools`: the five tools.
//! - `approval`: waiting for a decision (files in the data folder, MCP elicitation).
//! - `policy`: the policy in force (pinned at start, later edits can only tighten it) and
//!   the rules for variables an agent passes.
//! - `mask`: the boundary masking.

mod approval;
pub mod mask;
mod policy;
mod server;
mod tools;

use std::io::Write;
use std::path::PathBuf;

/// Default `--approval-timeout`, seconds.
pub const DEFAULT_APPROVAL_TIMEOUT: u64 = 300;

/// Default `--body-limit` of `rt mcp`: response bodies go into the agent's context, so the
/// default is much smaller than `rt send`'s.
pub const DEFAULT_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// `--help`: print [`help`] instead of serving.
    pub help: bool,
    /// The collection (a folder with `opencollection.yml`, or any folder or file below it);
    /// default: the current folder.
    pub collection: Option<PathBuf>,
    /// `--var NAME=VALUE`: runtime variables, all treated as secret.
    pub vars: Vec<(String, String)>,
    /// `--env-default NAME`: the environment when the agent names none.
    pub env_default: Option<String>,
    /// `--approval-timeout <s>`.
    pub approval_timeout: u64,
    /// `--no-scripts`: never run scripts, even in trusted collections.
    pub no_scripts: bool,
    /// `--agent-name NAME`: the agent's name in approvals and the audit log when the client
    /// sends none in `initialize`.
    pub agent_name: Option<String>,
    /// `--body-limit <BYTES>`.
    pub body_limit: usize,
    /// Wave 10: the distribution's send guard (never from the command line).
    pub guard: rt_core::guard::GuardHandle,
    /// Wave 17: the distribution's exchange observers (never from the command line).
    pub observers: rt_core::observe::ObserverSet,
    /// Wave 19: `--sequence`, `--sequence-values` (`sequence_flag.rs`).
    pub sequence: crate::sequence_flag::SequenceArgs,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            help: false,
            collection: None,
            vars: Vec::new(),
            env_default: None,
            approval_timeout: DEFAULT_APPROVAL_TIMEOUT,
            no_scripts: false,
            agent_name: None,
            body_limit: DEFAULT_BODY_LIMIT,
            guard: Default::default(),
            observers: Default::default(),
            sequence: Default::default(),
        }
    }
}

pub fn help() -> String {
    format!(
        "\
rt mcp: the collection as tools for coding agents (MCP over stdio)

Usage:
  rt mcp [<collection>] [options]

<collection> is the collection folder (or any folder or file inside it); default:
the current folder. The client (Claude Code, an IDE, ...) starts this command and
talks MCP on stdin/stdout; diagnostics go to stderr.

Tools: list_requests, get_request, list_environments, send_request, run_folder.
What an agent may do is set per environment in opencollection.yml
(extensions.requesttailor.agents: none, read or write; production). Default: read
only. Writing requests against a production environment wait for your approval:
in the app's banner, or with rt approvals / rt approve <id> / rt deny <id>.
Agents never see secret values; every call is written to the audit log
(agents/audit.jsonl in the data folder).

Options:
      --var <NAME=VALUE>        A variable for every send, e.g. a token; repeatable.
                                Values are secret: agents only see ***
      --env-default <NAME>      Environment when the agent names none
      --approval-timeout <S>    Seconds to wait for an approval (default {timeout});
                                then the request fails and nothing is sent
      --no-scripts              Do not run scripts (they run only in collections
                                you trusted in the app anyway)
      --agent-name <NAME>       Agent name for approvals and the audit log if the
                                client does not send one
      --body-limit <BYTES>      Keep at most BYTES of each response body for the
                                agent (default {limit})
      --sequence <FILE>         Write a Mermaid sequence diagram of the session to FILE
                                when it ends (and every 5 s)
      --sequence-values         Include short, masked excerpts of values
  -h, --help                    Show this help

Configure a client, e.g. Claude Code:
  claude mcp add requesttailor -- rt mcp /path/to/collection
",
        timeout = DEFAULT_APPROVAL_TIMEOUT,
        limit = DEFAULT_BODY_LIMIT,
    )
}

/// Parses the arguments after `rt mcp`. Never `Ok(None)`: `--help` is [`Args::help`], so
/// `rt mcp --help` prints this command's help.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => args.help = true,
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {:?}", redact(&pair)))?;
                args.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("env-default") => {
                args.env_default = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("approval-timeout") => {
                args.approval_timeout = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Long("no-scripts") => args.no_scripts = true,
            Long(name @ ("sequence" | "sequence-values")) => {
                let name = name.to_owned();
                crate::sequence_flag::parse_option(&name, parser, &mut args.sequence)?;
            }
            Long("agent-name") => {
                args.agent_name = Some(parser.value().map_err(err)?.string().map_err(err)?);
            }
            Long("body-limit") => {
                args.body_limit = parser.value().map_err(err)?.parse().map_err(err)?;
            }
            Value(v) if args.collection.is_none() => args.collection = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

/// A `--var` argument without `=` may be a bare secret: never echo it whole.
fn redact(s: &str) -> String {
    let shown: String = s.chars().take(3).collect();
    format!("{shown}…")
}

/// Serves MCP on the process's stdin and `stdout` until stdin closes.
pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if args.help {
        let _ = stdout.write_all(help().as_bytes());
        return crate::EXIT_OK;
    }
    if let Err(e) = crate::sequence_flag::check(&args.sequence) {
        let _ = writeln!(stderr, "rt: {e}");
        return crate::EXIT_USAGE;
    }
    let lines = server::stdin_lines();
    // Wave 19: the diagram is rewritten every 5 s and once more when the session ends.
    let sequence = args.sequence.clone();
    let periodic = crate::sequence_flag::Periodic::start(&sequence);
    let code = match server::Server::new(args, stdout, stderr) {
        Ok(mut server) => {
            server.serve(lines);
            crate::EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(stderr, "rt mcp: {e}");
            crate::EXIT_USAGE
        }
    };
    if let Some(p) = periodic {
        p.stop();
    }
    crate::sequence_flag::finish(&sequence, stderr, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Args, String> {
        let mut p = lexopt::Parser::from_args(args.iter().copied());
        parse(&mut p).map(|a| a.unwrap())
    }

    #[test]
    fn parses_options() {
        let a = parse_args(&[
            "col",
            "--var",
            "token=x=y",
            "--env-default",
            "Local",
            "--approval-timeout",
            "5",
            "--no-scripts",
            "--agent-name",
            "bot",
            "--body-limit",
            "100",
        ])
        .unwrap();
        assert_eq!(a.collection, Some(PathBuf::from("col")));
        assert_eq!(a.vars, [("token".to_owned(), "x=y".to_owned())]);
        assert_eq!(a.env_default.as_deref(), Some("Local"));
        assert_eq!(a.approval_timeout, 5);
        assert!(a.no_scripts);
        assert_eq!(a.agent_name.as_deref(), Some("bot"));
        assert_eq!(a.body_limit, 100);
        let d = parse_args(&[]).unwrap();
        assert_eq!(d, Args::default());
        assert!(parse_args(&["--help"]).unwrap().help);
    }

    #[test]
    fn rejects_bad_options_without_echoing_secrets() {
        let e = parse_args(&["--var", "supersecretvalue"]).unwrap_err();
        assert!(!e.contains("supersecretvalue"), "{e}");
        assert!(parse_args(&["--approval-timeout", "soon"]).is_err());
        assert!(parse_args(&["a", "b"]).is_err());
        assert!(parse_args(&["--nope"]).is_err());
    }

    #[test]
    fn help_names_the_tools_and_the_client_snippet() {
        let h = help();
        for want in ["send_request", "run_folder", "rt approve", "claude mcp add"] {
            assert!(h.contains(want), "{want}");
        }
    }
}
