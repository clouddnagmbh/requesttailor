// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp-test [<path>...]`: run saved MCP tests (`*.mcp.yaml`) against their servers, exit 4 if one fails (`rt_app::mcp_client`).
//!
//! ```text
//! rt mcp-test [<path>...] [-e NAME] [--var NAME=VALUE]...
//! ```
//!
//! Paths are test files or folders inside one collection; without a path, every test of the
//! collection the current folder belongs to runs. Tests run one after the other, in path
//! order, each with its own connection. `{{variables}}` come from the collection, the
//! environment (`-e`, with its external secrets read like `rt send` does) and `--var`.
//! stdio servers start in the collection folder (or their `cwd` below it).
//!
//! Output: `{"tests": [McpTestResult...], "passed": N, "failed": M}` as JSON on stdout,
//! secret values shown as `***`; one line per test on stderr.
//!
//! Exit codes: `EXIT_OK` all passed, `EXIT_TESTS` a test failed (assertion, `isError`
//! without assertions, a tool error, an unreadable test file), `EXIT_TRANSPORT` a server
//! could not be reached or started (takes precedence), `EXIT_USAGE` bad arguments or no
//! tests, `EXIT_PREPARE` external secrets could not be read.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_app::mcp_client::{McpTestResult, McpVars, parse_test, run_test_outcome};
use rt_format::COLLECTION_FILE;
use rt_format::mcp_test::is_test_path;

use crate::{EXIT_OK, EXIT_PREPARE, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, secrets};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    /// Test files or folders; empty: the collection of the current folder.
    pub paths: Vec<PathBuf>,
    pub environment: Option<String>,
    pub vars: Vec<(String, String)>,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;

    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args::default();
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
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
            Value(v) => args.paths.push(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(args))
}

/// The collection root at or above `path`.
fn root_of(path: &Path) -> Result<PathBuf, String> {
    let path =
        std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    path.ancestors()
        .find(|d| d.join(COLLECTION_FILE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "no {COLLECTION_FILE} in {} or any folder above",
                path.display()
            )
        })
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for c in path
        .strip_prefix(root)
        .map_err(|_| {
            format!(
                "{} is not in the collection {}",
                path.display(),
                root.display()
            )
        })?
        .components()
    {
        match c {
            Component::Normal(p) => parts.push(
                p.to_str()
                    .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))?
                    .to_owned(),
            ),
            _ => return Err(format!("unexpected path {}", path.display())),
        }
    }
    Ok(parts.join("/"))
}

/// Test files below `dir` (rules as for reading a collection: no hidden folders, no
/// `node_modules`, no nested collections, no symbolic links).
fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            if name != "node_modules" && !path.join(COLLECTION_FILE).is_file() {
                collect(root, &path, out)?;
            }
        } else if kind.is_file() && is_test_path(&name) {
            out.push(relative(root, &path)?);
        }
    }
    Ok(())
}

/// The collection root and the tests to run (relative paths, sorted, unique).
fn locate(args: &Args) -> Result<(PathBuf, Vec<String>), String> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };
    let root = root_of(&paths[0])?;
    let mut tests = Vec::new();
    for p in &paths {
        let full =
            std::fs::canonicalize(p).map_err(|e| format!("cannot open {}: {e}", p.display()))?;
        if root_of(&full)? != root {
            return Err(format!(
                "{} is in another collection than {}",
                p.display(),
                paths[0].display()
            ));
        }
        if full.is_dir() {
            collect(&root, &full, &mut tests)?;
        } else if is_test_path(&full.to_string_lossy().replace('\\', "/")) {
            tests.push(relative(&root, &full)?);
        } else {
            return Err(format!(
                "{} is not an MCP test (*.{})",
                p.display(),
                rt_app::mcp_client::TEST_EXTENSION
            ));
        }
    }
    tests.sort();
    tests.dedup();
    Ok((root, tests))
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, code: i32, msg: &str| {
        let _ = writeln!(stderr, "rt: {msg}");
        code
    };
    let (root, tests) = match locate(&args) {
        Ok(r) => r,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if tests.is_empty() {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("no MCP tests (*.mcp.yaml) in {}", root.display()),
        );
    }
    let doc = match crate::load_collection(&root) {
        Ok(d) => d,
        Err(e) => return fail(stderr, EXIT_USAGE, &e),
    };
    if let Some(env) = &args.environment
        && doc.model().environment(env).is_none()
    {
        return fail(
            stderr,
            EXIT_USAGE,
            &format!("no environment {env:?} in {}", root.display()),
        );
    }
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
    let collection = match runtime.block_on(secrets::resolve(
        doc.model(),
        args.environment.as_deref(),
        &args.vars,
    )) {
        Ok(c) => c,
        Err(e) => return fail(stderr, EXIT_PREPARE, &e),
    };
    let vars = McpVars::new(&collection, args.environment.as_deref(), &args.vars);

    let mut results: Vec<McpTestResult> = Vec::new();
    let mut unreachable = false;
    for path in &tests {
        let result = match std::fs::read(root.join(path)) {
            Err(e) => McpTestResult {
                path: path.clone(),
                passed: false,
                result: None,
                error: Some(format!("cannot read {path}: {e}")),
                assertions: Vec::new(),
            },
            Ok(bytes) => match parse_test(path, &bytes) {
                Err(e) => McpTestResult {
                    path: path.clone(),
                    passed: false,
                    result: None,
                    error: Some(e.to_string()),
                    assertions: Vec::new(),
                },
                Ok(test) => {
                    let outcome = runtime.block_on(run_test_outcome(
                        &transport,
                        path,
                        &test,
                        &vars,
                        Some(&root),
                    ));
                    unreachable |= outcome.connect_failed;
                    outcome.result
                }
            },
        };
        let line = match (&result.error, result.passed) {
            (_, true) => format!("PASS {path}"),
            (Some(e), false) => format!("FAIL {path}: {e}"),
            (None, false) => {
                let failed = result.assertions.iter().filter(|a| !a.passed).count();
                if failed > 0 {
                    format!("FAIL {path}: {failed} assertion(s) failed")
                } else {
                    format!("FAIL {path}: the tool reported an error")
                }
            }
        };
        let _ = writeln!(stderr, "{line}");
        results.push(result);
    }
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;
    let report = serde_json::json!({ "tests": results, "passed": passed, "failed": failed });
    let _ = writeln!(
        stdout,
        "{}",
        serde_json::to_string_pretty(&report).expect("JSON serializes")
    );
    if unreachable {
        EXIT_TRANSPORT
    } else if failed > 0 {
        EXIT_TESTS
    } else {
        EXIT_OK
    }
}
