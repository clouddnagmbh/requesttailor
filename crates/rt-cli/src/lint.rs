// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt lint [<folder>] [--format text|json|github] [--min-score N] [--fail-on warn|error]
//! [--rule <id>=<level>]...` (wave 18, strand F): the collection lint of `rt_app::lint` for
//! pipelines. Exit `EXIT_LINT` (7) when a finding reaches `--fail-on` or the score is
//! below `--min-score` (or the `min_score` of the collection's settings). It reads the files and
//! sends nothing. `<folder>` is the collection folder or a folder inside a collection: the
//! report is then about that folder. Exit 2 for a folder that is no collection.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rt_app::Workspace;
use rt_app::lint::{self, LintFinding, LintLevel, LintReport, LintRule};
use rt_format::{COLLECTION_FILE, FileSet};

/// Exit code of `rt lint` when it found what it was told to fail on (wave 18).
pub const EXIT_LINT: i32 = 7;

pub const USAGE: &str = "  rt lint [<folder>] [--format text|json|github] [--min-score <N>]\n          [--fail-on warn|error] [--rule <id>=<level>]...\n";

pub const HELP: &str = "rt lint checks a collection for requests without tests, hard-coded hosts, unused\nvariables and duplicate names and prints a health score from 0 to 100. Exit code 7\nwhen a finding reaches --fail-on or the score is below --min-score. Settings live in\nopencollection.yml (extensions.requesttailor.lint); --rule id=level (off, info, warn,\nerror) overrides them. It reads files and sends nothing.\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Text,
    Json,
    /// GitHub Actions workflow commands (`::warning file=…::…`).
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub folder: PathBuf,
    pub format: Format,
    pub min_score: Option<u8>,
    pub fail_on: Option<LintLevel>,
    /// `--rule id=level`, repeatable.
    pub rules: Vec<(String, LintLevel)>,
}

fn level(text: &str) -> Result<LintLevel, String> {
    match text {
        "off" => Ok(LintLevel::Off),
        "info" => Ok(LintLevel::Info),
        "warn" => Ok(LintLevel::Warn),
        "error" => Ok(LintLevel::Error),
        other => Err(format!(
            "unknown level {other:?} (off, info, warn or error)"
        )),
    }
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut args = Args {
        folder: PathBuf::from("."),
        format: Format::Text,
        min_score: None,
        fail_on: None,
        rules: Vec::new(),
    };
    let mut folder = None;
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("format") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "github" => Format::Github,
                    other => {
                        return Err(format!("unknown format {other:?} (text, json or github)"));
                    }
                };
            }
            Long("min-score") => {
                args.min_score = Some(parser.value().map_err(err)?.parse().map_err(err)?);
            }
            Long("fail-on") => {
                let value = parser.value().map_err(err)?.string().map_err(err)?;
                match level(&value)? {
                    l @ (LintLevel::Warn | LintLevel::Error) => args.fail_on = Some(l),
                    _ => return Err("--fail-on takes warn or error".into()),
                }
            }
            Long("rule") => {
                let value = parser.value().map_err(err)?.string().map_err(err)?;
                let (id, lvl) = value
                    .split_once('=')
                    .ok_or_else(|| format!("--rule expects ID=LEVEL, got {value:?}"))?;
                args.rules.push((id.to_owned(), level(lvl)?));
            }
            Value(v) if folder.is_none() => folder = Some(PathBuf::from(v)),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if let Some(f) = folder {
        args.folder = f;
    }
    Ok(Some(args))
}

/// The collection root at or above `folder`, and the path of `folder` inside it (`""`: the
/// collection itself).
fn locate(folder: &Path) -> Result<(PathBuf, String), String> {
    let canonical = std::fs::canonicalize(folder)
        .map_err(|e| format!("cannot open {}: {e}", folder.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let root = canonical
        .ancestors()
        .find(|dir| dir.join(COLLECTION_FILE).is_file())
        .ok_or_else(|| {
            format!(
                "no {COLLECTION_FILE} in {} or any folder above it",
                folder.display()
            )
        })?
        .to_path_buf();
    let relative = canonical.strip_prefix(&root).expect("root is an ancestor");
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => p.to_str(),
            _ => None,
        })
        .collect();
    Ok((root, parts.join("/")))
}

fn within(path: &str, folder: &str) -> bool {
    folder.is_empty() || path == folder || path.starts_with(&format!("{folder}/"))
}

/// What the report says about `folder` only.
fn narrow(mut report: LintReport, folder: &str) -> LintReport {
    if folder.is_empty() {
        return report;
    }
    if let Some(f) = report.folders.iter().find(|f| f.folder == folder) {
        report.score = f.score.clone();
    }
    report.folders.retain(|f| within(&f.folder, folder));
    report
        .findings
        .retain(|f| within(f.path.trim_end_matches('/'), folder));
    report
}

fn word(level: LintLevel) -> &'static str {
    level.word()
}

fn shown(path: &str) -> &str {
    if path.is_empty() {
        "(collection)"
    } else {
        path
    }
}

fn text(report: &LintReport, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "{}: health score {}/100",
        report.collection, report.score.score
    );
    for r in &report.score.rules {
        let _ = writeln!(out, "  {:<24}{}/{}", r.rule.id(), r.ok, r.total);
    }
    for f in &report.findings {
        let _ = writeln!(
            out,
            "{:<5} {}  {}  {}",
            word(f.level),
            f.rule.id(),
            shown(&f.path),
            f.message
        );
    }
    let count = |l| report.findings.iter().filter(|f| f.level == l).count();
    let _ = writeln!(
        out,
        "{} findings ({} error, {} warn, {} info), {} ignored.",
        report.findings.len(),
        count(LintLevel::Error),
        count(LintLevel::Warn),
        count(LintLevel::Info),
        report.ignored
    );
}

/// The file a finding belongs to, as the annotations of a pipeline want it.
fn file_of(prefix: &Path, f: &LintFinding) -> String {
    let rel = if f.path.is_empty() {
        COLLECTION_FILE.to_owned()
    } else if f.path.ends_with('/') {
        format!("{}folder.yml", f.path)
    } else {
        f.path.clone()
    };
    prefix.join(rel).to_string_lossy().replace('\\', "/")
}

fn escape(text: &str, property: bool) -> String {
    let mut out = text
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    if property {
        out = out.replace(':', "%3A").replace(',', "%2C");
    }
    out
}

fn github(report: &LintReport, prefix: &Path, out: &mut dyn Write) {
    for f in &report.findings {
        let kind = match f.level {
            LintLevel::Error => "error",
            LintLevel::Warn => "warning",
            _ => "notice",
        };
        let _ = writeln!(
            out,
            "::{kind} file={},title={}::{}",
            escape(&file_of(prefix, f), true),
            escape(f.rule.id(), true),
            escape(&f.message, false)
        );
    }
    let _ = writeln!(
        out,
        "::notice title=RequestTailor lint::Health score {}/100",
        report.score.score
    );
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let fail = |stderr: &mut dyn Write, message: String| {
        let _ = writeln!(stderr, "rt: lint: {message}");
        crate::EXIT_USAGE
    };
    let (root, sub) = match locate(&args.folder) {
        Ok(found) => found,
        Err(e) => return fail(stderr, e),
    };
    let files = match FileSet::from_dir(&root) {
        Ok(f) => f,
        Err(e) => return fail(stderr, format!("cannot read {}: {e}", root.display())),
    };
    let ws = match Workspace::open(files) {
        Ok(ws) => ws,
        Err(e) => return fail(stderr, format!("{}: {e}", root.display())),
    };
    let mut settings = lint::settings(&ws);
    for (id, level) in &args.rules {
        let Some(rule) = LintRule::from_id(id) else {
            return fail(stderr, format!("unknown rule {id:?}"));
        };
        settings.rules.insert(rule.id().to_owned(), *level);
    }
    let min_score = args.min_score.or(settings.min_score);
    let report = narrow(lint::lint_with(&ws, &settings), &sub);

    match args.format {
        Format::Text => text(&report, stdout),
        Format::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string(&report).unwrap_or_else(|_| "{}".into())
            );
        }
        Format::Github => {
            // Annotations name files as the pipeline sees them: below the folder as given.
            let depth = sub.split('/').filter(|p| !p.is_empty()).count();
            let prefix = args
                .folder
                .ancestors()
                .nth(depth)
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let prefix = if prefix == Path::new(".") {
                PathBuf::new()
            } else {
                prefix
            };
            github(&report, &prefix, stdout);
        }
    }
    let _ = stdout.flush();

    let by_level = args
        .fail_on
        .is_some_and(|min| report.findings.iter().any(|f| f.level >= min));
    let by_score = min_score.is_some_and(|min| report.score.score < min);
    if by_score && args.format == Format::Text {
        let _ = writeln!(
            stderr,
            "rt: lint: health score {} is below the minimum of {}",
            report.score.score,
            min_score.unwrap_or(0)
        );
    }
    if by_level || by_score { EXIT_LINT } else { 0 }
}
