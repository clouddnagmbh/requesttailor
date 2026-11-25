// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt security audit [<folder>] [-e <env>] [--var N=V]... [--format text|markdown|html|json|sarif]
//! [--out FILE] [--fail-on <severity>] [--allow-writes] [--no-scripts] [--no-plugins]
//! [--respect-limits] [--max-wait D] [--no-prompt] [--plugin FILE.wasm]...` (wave 19, strands R
//! and R2): the API security audit of a collection.
//!
//! The free audit is passive. It runs the collection like `rt run` (same runner, send guard,
//! rate limits, production banner), reads every response with the passive rules of
//! `rt_app::security` and sends nothing else: every request goes out once. Writing requests
//! are left out unless `--allow-writes`. The findings are indications, not proof; every report
//! says so. No secret or token value is ever in a report (findings carry the place and the
//! reason, request URLs lose their query).
//!
//! Exit codes: 0 no finding at or above `--fail-on` (without `--fail-on` the audit never
//! fails on findings), [`EXIT_AUDIT`] (9) a finding reached `--fail-on`, `EXIT_BLOCKED` (5)
//! the send guard refused a request (before 9), 2 usage.
//!
//! A distribution plugs in with [`SecurityAuditExtension`] (`Distribution.security_audit`):
//! it then handles the command, gets the parsed common arguments, everything else as
//! [`Args::rest`], and the passive result to merge its own findings into one report.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use rt_app::security::{AuditFormat, SecurityFinding, SecuritySettings, Severity};
use rt_core::guard::AuditGuard;
use rt_core::model::{Collection, Item, RequestSpec};
use rt_core::send::{SendError, SendReport};
use rt_format::{COLLECTION_FILE, FileSet};
use serde::Serialize;

use crate::run::{self, RunArgs};

/// Exit code of `rt security audit` when a finding reaches `--fail-on` (wave 19).
pub const EXIT_AUDIT: i32 = 9;

pub const USAGE: &str = "  rt security audit [<folder>] [-e <env>] [--var N=V]... [--format text|markdown|html|json|sarif]\n            [--out FILE] [--fail-on <severity>] [--allow-writes] [--no-scripts] [--no-plugins]\n";

pub const HELP: &str = "rt security audit checks the responses of a collection run against the OWASP API Security Top\n10: open CORS, weak tokens, missing security headers, no sign of rate limiting, TLS\nproblems and revealing error pages. It runs the collection like `rt run` (the same guard, rate\nlimits and production banner), sends every request once, reads the responses and sends\nnothing else. Writing requests are left out unless --allow-writes. Findings are\nindications, not proof. No secret or token value is printed.\n  --format F       text (default), markdown, html, json or sarif (2.1.0, for code scanning)\n  --out FILE       write the report there instead of stdout\n  --fail-on S      exit 9 when a finding is at least S (info, low, medium, high, critical)\nNever run it against production; only against APIs you are allowed to test. Active probes\ncome from a distribution of rt, not from this build.\n";

/// What every report says about its findings.
pub const DISCLAIMER: &str = "These are indications read from the responses of one run, not proof of a vulnerability. A person has to judge each one. Nothing but the requests of the collection was sent.";

/// The arguments every `rt security audit` takes, whoever handles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The run: folder (`path`), environment, `--var`s, plugin and script switches, rate
    /// limits, and the distribution's guard and observers (set by the CLI, never parsed).
    pub run: RunArgs,
    pub format: AuditFormat,
    /// `--out FILE`; without it the report goes to stdout.
    pub out: Option<PathBuf>,
    pub fail_on: Option<Severity>,
    pub allow_writes: bool,
    /// Flags and values this build does not know, in order. The free audit refuses any; a
    /// distribution's [`SecurityAuditExtension`] reads its own (e.g. `--i-am-authorized`).
    /// After the first unknown flag every value goes here, so give flags with a value as
    /// `--flag=value`.
    pub rest: Vec<OsString>,
}

pub fn severity(text: &str) -> Result<Severity, String> {
    match text {
        "info" => Ok(Severity::Info),
        "low" => Ok(Severity::Low),
        "medium" => Ok(Severity::Medium),
        "high" => Ok(Severity::High),
        "critical" => Ok(Severity::Critical),
        other => Err(format!(
            "unknown severity {other:?} (info, low, medium, high or critical)"
        )),
    }
}

pub fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    // `rt security <subcommand>`: only `audit` so far; later subcommands get their own arm.
    match parser.next().map_err(err)? {
        Some(Short('h') | Long("help")) => return Ok(None),
        Some(Value(v)) if v == "audit" => {}
        _ => return Err("security: expected `rt security audit [<folder>]`".into()),
    }
    let mut args = Args {
        run: RunArgs::default(),
        format: AuditFormat::Text,
        out: None,
        fail_on: None,
        allow_writes: false,
        rest: Vec::new(),
    };
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            // Once an unknown flag was seen, plain values belong to it.
            Value(v) if !args.rest.is_empty() => args.rest.push(v),
            Short('e') | Long("env") => {
                args.run.environment = Some(parser.value().map_err(err)?.string().map_err(err)?)
            }
            Long("var") => {
                let pair = parser.value().map_err(err)?.string().map_err(err)?;
                let (name, value) = pair
                    .split_once('=')
                    .filter(|(name, _)| !name.is_empty())
                    .ok_or_else(|| format!("--var expects NAME=VALUE, got {pair:?}"))?;
                args.run.vars.push((name.to_owned(), value.to_owned()));
            }
            Long("format") => {
                args.format = match parser.value().map_err(err)?.string().map_err(err)?.as_str() {
                    "text" => AuditFormat::Text,
                    "markdown" => AuditFormat::Markdown,
                    "html" => AuditFormat::Html,
                    "json" => AuditFormat::Json,
                    "sarif" => AuditFormat::Sarif,
                    other => {
                        return Err(format!(
                            "unknown format {other:?} (text, markdown, html, json or sarif)"
                        ));
                    }
                };
            }
            Long("out") => args.out = Some(parser.value().map_err(err)?.into()),
            Long("fail-on") => {
                args.fail_on = Some(severity(
                    &parser.value().map_err(err)?.string().map_err(err)?,
                )?)
            }
            Long("allow-writes") => args.allow_writes = true,
            Long("no-scripts") => args.run.no_scripts = true,
            Long("no-plugins") => args.run.no_plugins = true,
            Long("no-prompt") => args.run.prompt = false,
            Long("plugin") => args.run.plugins.push(parser.value().map_err(err)?.into()),
            Long("respect-limits") => {
                args.run.throttle.mode = rt_core::ratelimit::ThrottleMode::Respect
            }
            Long("max-wait") => {
                args.run.throttle.max_wait_ms =
                    crate::duration_ms(&parser.value().map_err(err)?.string().map_err(err)?)?;
            }
            Value(v) if args.run.path.is_none() => args.run.path = Some(PathBuf::from(v)),
            Long(name) => {
                let mut flag = OsString::from(format!("--{name}"));
                if let Some(v) = parser.optional_value() {
                    flag.push("=");
                    flag.push(v);
                }
                args.rest.push(flag);
            }
            Short(c) => args.rest.push(format!("-{c}").into()),
            Value(v) => return Err(format!("unexpected argument {:?}", v.to_string_lossy())),
        }
    }
    Ok(Some(args))
}

// ------------------------------------------------------------------------------------------
// The result

/// One request of the run with what the passive rules found in its response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditedRequest {
    /// Request file relative to the collection root (the SARIF location).
    pub path: String,
    pub method: String,
    /// The URL without its query (a query may carry a credential).
    pub url: String,
    /// `None`: no response (the send failed).
    pub status: Option<u16>,
    pub error: Option<String>,
    pub findings: Vec<SecurityFinding>,
}

/// A request left out of the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedRequest {
    pub path: String,
    pub reason: String,
}

/// The audit of one run: what [`render`] turns into a report. A distribution adds its findings
/// to `requests[..].findings` (or pushes requests) and renders the whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditReport {
    pub collection: String,
    pub environment: Option<String>,
    pub requests: Vec<AuditedRequest>,
    pub skipped: Vec<SkippedRequest>,
    /// Requests the send guard refused.
    pub blocked: usize,
}

impl AuditReport {
    pub fn findings(&self) -> impl Iterator<Item = (&AuditedRequest, &SecurityFinding)> {
        self.requests
            .iter()
            .flat_map(|r| r.findings.iter().map(move |f| (r, f)))
    }

    pub fn max_severity(&self) -> Option<Severity> {
        self.findings().map(|(_, f)| f.severity).max()
    }

    /// The exit code the free audit gives: [`crate::EXIT_BLOCKED`] when the guard refused a
    /// request, [`EXIT_AUDIT`] when a finding reaches `fail_on`, else 0.
    pub fn exit_code(&self, fail_on: Option<Severity>) -> i32 {
        if self.blocked > 0 {
            return crate::EXIT_BLOCKED;
        }
        match (fail_on, self.max_severity()) {
            (Some(limit), Some(max)) if max >= limit => EXIT_AUDIT,
            _ => crate::EXIT_OK,
        }
    }
}

/// The method, URL and status as an audit shows them: no query, no fragment.
fn audited(report: &SendReport, findings: Vec<SecurityFinding>) -> AuditedRequest {
    let (method, url) = report
        .request
        .as_ref()
        .map(|r| (r.method.clone(), strip_query(&r.url)))
        .unwrap_or_default();
    AuditedRequest {
        path: report.request_path.clone(),
        method,
        url,
        status: report.response.as_ref().map(|r| r.status),
        error: report.error.as_ref().map(crate::report::error_message),
        findings,
    }
}

fn strip_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_owned()
}

fn collection_ignore(root: &std::path::Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(root.join(COLLECTION_FILE)) else {
        return Vec::new();
    };
    let files = FileSet {
        files: [(COLLECTION_FILE.to_owned(), bytes)].into(),
    };
    rt_format::checks_config::configured(&files).ignore
}

/// The values of secret variables of the collection and the `--var`s that share their names,
/// longest first: what must never reach a report.
fn secret_values(collection: &Collection, vars: &[(String, String)]) -> Vec<String> {
    let declared = collection.defaults.vars.iter().chain(
        collection
            .environments
            .iter()
            .flat_map(|e| e.variables.iter()),
    );
    let mut names: Vec<&str> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    for v in declared.filter(|v| v.secret) {
        names.push(&v.name);
        values.extend(v.value.clone());
    }
    for (name, value) in vars {
        if names.contains(&name.as_str()) {
            values.push(value.clone());
        }
    }
    values.retain(|v| !v.is_empty());
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

fn mask(text: &str, secrets: &[String]) -> String {
    secrets.iter().fold(text.to_owned(), |t, s| {
        t.replace(s.as_str(), rt_core::send::MASK)
    })
}

fn scrub_finding(f: &mut SecurityFinding, secrets: &[String]) {
    f.location = mask(&f.location, secrets);
    f.why = mask(&f.why, secrets);
    f.fix = mask(&f.fix, secrets);
}

/// The audit of a finished run.
fn collect(
    steps: &[rt_core::runner::RunStep],
    opened: &run::Opened,
    args: &Args,
    secrets: &[String],
) -> AuditReport {
    let mut settings: SecuritySettings =
        rt_app::security::read_settings(rt_plugin::host::dir::data_dir().as_deref());
    settings.disabled.extend(collection_ignore(&opened.root));
    let mut requests: Vec<AuditedRequest> = Vec::new();
    let mut blocked = 0;
    for step in steps {
        if matches!(step.report.error, Some(SendError::Blocked { .. })) {
            blocked += 1;
        }
        let mut findings = rt_app::security::passive(&step.report, &settings);
        findings.iter_mut().for_each(|f| scrub_finding(f, secrets));
        let mut item = audited(&step.report, findings);
        item.url = mask(&item.url, secrets);
        item.error = item.error.map(|e| mask(&e, secrets));
        // Iterations send a request again: the same finding is listed once.
        match requests.iter_mut().find(|r| r.path == item.path) {
            Some(known) => {
                for f in item.findings {
                    if !known.findings.contains(&f) {
                        known.findings.push(f);
                    }
                }
            }
            None => requests.push(item),
        }
    }
    AuditReport {
        collection: opened.info.collection.clone(),
        environment: args.run.environment.clone(),
        requests,
        skipped: opened
            .skipped
            .iter()
            .map(|(path, reason)| SkippedRequest {
                path: path.clone(),
                reason: reason.clone(),
            })
            .collect(),
        blocked,
    }
}

// ------------------------------------------------------------------------------------------
// The hook

/// What a distribution gets when it handles `rt security audit`.
pub struct AuditContext<'a> {
    /// The common arguments, the distribution's guard and observers in `args.run`, and the
    /// flags this build does not know in `args.rest`.
    pub args: &'a Args,
    /// What the free passive audit found. Merge your own findings into a copy and
    /// [`render`] it, or render your own report.
    pub passive: &'a AuditReport,
    /// The collection run, for sending probes. Every send goes through the core.
    pub session: AuditSession<'a>,
}

/// Sends requests of the audited collection through the core's send path: variables,
/// inheritance, auth (OAuth, SigV4, secrets) are resolved by the core, the report comes back
/// masked and the guard is asked with `Purpose::Audit`. The session never hands out a secret
/// value.
pub struct AuditSession<'a> {
    opened: &'a run::Opened,
    guard: AuditGuard,
    secrets: Vec<String>,
}

impl AuditSession<'_> {
    /// The collection root on disk.
    pub fn root(&self) -> &std::path::Path {
        &self.opened.root
    }

    /// The request as written in the files (variables as `{{names}}`, auth possibly
    /// `inherit`; no secret value).
    pub fn request(&self, path: &str) -> Option<RequestSpec> {
        self.opened
            .collection
            .request(path)
            .map(|c| c.request.spec.clone())
    }

    /// The paths of the requests the free audit ran, in order.
    pub fn requests(&self) -> &[String] {
        &self.opened.plan.requests
    }

    /// Sends the request at `path` once, as written or with `spec` in its place (a probe),
    /// through `send_with`. The guard sees `Purpose::Audit`. Blocking.
    pub fn send(&self, path: &str, spec: Option<RequestSpec>) -> SendReport {
        let mut collection = self.opened.collection.clone();
        if let Some(spec) = spec
            && !replace_request(&mut collection.items, path, spec)
        {
            return crate::services::failed(
                path,
                &self.opened.send_args,
                SendError::NotFound(path.to_owned()),
            );
        }
        let mut services = self.opened.services();
        services.guard = Some(&self.guard);
        self.opened.send_with_services(&collection, path, &services)
    }

    /// Replaces every secret value of the collection in `text` with `***`.
    pub fn mask(&self, text: &str) -> String {
        mask(text, &self.secrets)
    }
}

fn replace_request(items: &mut [Item], path: &str, spec: RequestSpec) -> bool {
    for item in items {
        match item {
            Item::Request(r) if r.path == path => {
                r.spec = spec;
                return true;
            }
            Item::Folder(f) => {
                if replace_request(&mut f.items, path, spec.clone()) {
                    return true;
                }
            }
            Item::Request(_) => {}
        }
    }
    false
}

/// A distribution's `rt security audit` (`Distribution.security_audit`). When set it handles
/// the whole command after the free passive pass ran; return the exit code. Print the report
/// with [`write_report`] (or your own) and respect `ctx.args.fail_on` via
/// [`AuditReport::exit_code`].
pub trait SecurityAuditExtension {
    fn run(&self, ctx: &AuditContext<'_>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32;
}

// ------------------------------------------------------------------------------------------
// The command

/// The free passive audit, then (when `extension` is set) the distribution's part.
pub fn run(
    args: Args,
    extension: Option<&dyn SecurityAuditExtension>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    if extension.is_none()
        && let Some(first) = args.rest.first()
    {
        let first = first.to_string_lossy();
        let why = if first.starts_with("--i-am-authorized") {
            "active probes come from a distribution of rt; this build only reads the responses of the run"
        } else {
            "unknown flag"
        };
        let _ = writeln!(
            stderr,
            "rt: security audit: {first}: {why}\nTry 'rt --help'."
        );
        return crate::EXIT_USAGE;
    }
    let allow_writes = args.allow_writes;
    let filter = move |collection: &Collection, path: &str| -> Result<(), String> {
        match collection.request(path) {
            Some(c)
                if !allow_writes
                    && rt_core::guard::effect(&c.request.spec) == rt_core::guard::Effect::Write =>
            {
                Err("writing request (use --allow-writes)".to_owned())
            }
            _ => Ok(()),
        }
    };
    let opened = match run::open(&args.run, stderr, Some(&filter)) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let (_summary, steps, _text) = opened.execute(stderr);
    let secrets = secret_values(&opened.collection, &args.run.vars);
    let passive = collect(&steps, &opened, &args, &secrets);
    let ctx = AuditContext {
        args: &args,
        passive: &passive,
        session: AuditSession {
            opened: &opened,
            guard: AuditGuard::new(args.run.guard.clone()),
            secrets,
        },
    };
    match extension {
        Some(extension) => extension.run(&ctx, stdout, stderr),
        None => {
            if let Err(code) = write_report(&args, &passive, stdout, stderr) {
                return code;
            }
            passive.exit_code(args.fail_on)
        }
    }
}

/// Renders `report` in `args.format` to `args.out` or stdout. `Err(code)`: the file could not
/// be written (message on `stderr`).
pub fn write_report(
    args: &Args,
    report: &AuditReport,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), i32> {
    let text = render(report, args.format);
    match &args.out {
        None => {
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
            Ok(())
        }
        Some(file) => run::write_file(file, &text).map_err(|e| {
            let _ = writeln!(
                stderr,
                "rt: cannot write the report {}: {e}",
                file.display()
            );
            crate::EXIT_USAGE
        }),
    }
}

// ------------------------------------------------------------------------------------------
// Formats

struct Occurrence<'a> {
    request: &'a AuditedRequest,
    finding: &'a SecurityFinding,
}

struct Group<'a> {
    rule: &'a str,
    title: String,
    category: &'static str,
    severity: Severity,
    items: Vec<Occurrence<'a>>,
}

fn category_id(c: rt_app::security::OwaspCategory) -> &'static str {
    use rt_app::security::OwaspCategory::*;
    match c {
        Api1 => "API1:2023",
        Api2 => "API2:2023",
        Api3 => "API3:2023",
        Api4 => "API4:2023",
        Api5 => "API5:2023",
        Api6 => "API6:2023",
        Api7 => "API7:2023",
        Api8 => "API8:2023",
        Api9 => "API9:2023",
        Api10 => "API10:2023",
    }
}

fn category_title(id: &str) -> &'static str {
    match id {
        "API1:2023" => "Broken Object Level Authorization",
        "API2:2023" => "Broken Authentication",
        "API3:2023" => "Broken Object Property Level Authorization",
        "API4:2023" => "Unrestricted Resource Consumption",
        "API5:2023" => "Broken Function Level Authorization",
        "API6:2023" => "Unrestricted Access to Sensitive Business Flows",
        "API7:2023" => "Server Side Request Forgery",
        "API8:2023" => "Security Misconfiguration",
        "API9:2023" => "Improper Inventory Management",
        "API10:2023" => "Unsafe Consumption of APIs",
        _ => "",
    }
}

/// Findings per rule, the worst rules first.
fn groups(report: &AuditReport) -> Vec<Group<'_>> {
    let catalog = rt_app::security::rules();
    let mut out: Vec<Group<'_>> = Vec::new();
    for (request, finding) in report.findings() {
        let i = match out.iter().position(|g| g.rule == finding.rule) {
            Some(i) => i,
            None => {
                let title = catalog
                    .iter()
                    .find(|r| r.id == finding.rule)
                    .map(|r| r.title.clone())
                    .unwrap_or_else(|| finding.rule.clone());
                out.push(Group {
                    rule: &finding.rule,
                    title,
                    category: category_id(finding.category),
                    severity: finding.severity,
                    items: Vec::new(),
                });
                out.len() - 1
            }
        };
        out[i].items.push(Occurrence { request, finding });
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.rule.cmp(b.rule)));
    out
}

/// The OWASP categories of `groups` in the order of the Top 10.
fn categories<'a>(groups: &[Group<'a>]) -> Vec<&'a str> {
    let number = |c: &str| {
        c.trim_start_matches("API")
            .split(':')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
    };
    let mut c: Vec<&str> = groups.iter().map(|g| g.category).collect();
    c.sort_by_key(|c| number(c));
    c.dedup();
    c
}

fn counts(report: &AuditReport) -> [(Severity, usize); 5] {
    let mut c = [
        (Severity::Critical, 0),
        (Severity::High, 0),
        (Severity::Medium, 0),
        (Severity::Low, 0),
        (Severity::Info, 0),
    ];
    for (_, f) in report.findings() {
        if let Some(slot) = c.iter_mut().find(|(s, _)| *s == f.severity) {
            slot.1 += 1;
        }
    }
    c
}

/// The report in `format`. Pure; contains no secret value as long as the report does not.
pub fn render(report: &AuditReport, format: AuditFormat) -> String {
    match format {
        AuditFormat::Text => text(report),
        AuditFormat::Markdown => markdown(report),
        AuditFormat::Html => html(report),
        AuditFormat::Json => json(report),
        AuditFormat::Sarif => sarif(report),
    }
}

fn target(request: &AuditedRequest) -> String {
    let status = request
        .status
        .map_or_else(|| "no response".to_owned(), |s| s.to_string());
    format!("{} {} ({status})", request.method, request.url)
}

fn summary_line(report: &AuditReport) -> String {
    let n = report.findings().count();
    format!(
        "{} request{} audited, {} finding{}",
        report.requests.len(),
        if report.requests.len() == 1 { "" } else { "s" },
        n,
        if n == 1 { "" } else { "s" }
    )
}

fn title_line(report: &AuditReport) -> String {
    match &report.environment {
        Some(env) => format!("{} (environment {env})", report.collection),
        None => report.collection.clone(),
    }
}

fn text(report: &AuditReport) -> String {
    let mut out = format!(
        "RequestTailor security audit: {}\n{DISCLAIMER}\n{}\n",
        title_line(report),
        summary_line(report)
    );
    if report.blocked > 0 {
        out.push_str(&format!(
            "{} request(s) were refused by the send guard and are not audited.\n",
            report.blocked
        ));
    }
    for s in &report.skipped {
        out.push_str(&format!("left out: {} ({})\n", s.path, s.reason));
    }
    for g in groups(report) {
        out.push_str(&format!(
            "\n[{}] {} {} ({}) in {} place(s)\n",
            severity_name(g.severity),
            g.rule,
            g.title,
            g.category,
            g.items.len()
        ));
        for o in &g.items {
            out.push_str(&format!(
                "  - {} {}\n      {}: {}\n",
                o.request.path,
                target(o.request),
                o.finding.location,
                o.finding.why
            ));
        }
        if let Some(first) = g.items.first() {
            out.push_str(&format!("  Fix: {}\n", first.finding.fix));
        }
    }
    out
}

fn markdown(report: &AuditReport) -> String {
    let mut out = format!(
        "# Security audit: {}\n\n> {DISCLAIMER}\n\n{}.\n\n",
        md(&title_line(report)),
        summary_line(report)
    );
    out.push_str("| Severity | Findings |\n| --- | --- |\n");
    for (s, n) in counts(report) {
        out.push_str(&format!("| {} | {n} |\n", severity_name(s)));
    }
    if report.blocked > 0 {
        out.push_str(&format!(
            "\n{} request(s) were refused by the send guard and are not audited.\n",
            report.blocked
        ));
    }
    if !report.skipped.is_empty() {
        out.push_str("\nLeft out:\n\n");
        for s in &report.skipped {
            out.push_str(&format!("- `{}`: {}\n", s.path, md(&s.reason)));
        }
    }
    let groups = groups(report);
    for c in categories(&groups) {
        out.push_str(&format!("\n## {c} {}\n", category_title(c)));
        for g in groups.iter().filter(|g| g.category == c) {
            out.push_str(&format!(
                "\n### {} `{}` ({})\n\n",
                md(&g.title),
                g.rule,
                severity_name(g.severity)
            ));
            for o in &g.items {
                out.push_str(&format!(
                    "- `{}` {}: {} {}\n",
                    o.request.path,
                    md(&target(o.request)),
                    md(&o.finding.location),
                    md(&o.finding.why)
                ));
            }
            if let Some(first) = g.items.first() {
                out.push_str(&format!("\nFix: {}\n", md(&first.finding.fix)));
            }
        }
    }
    out
}

/// Text that stays text in Markdown (no HTML, no emphasis, no links).
fn md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' | '>' | '&' | '`' | '*' | '_' | '[' | ']' | '|' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn html(report: &AuditReport) -> String {
    let mut out = String::from(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Security audit</title>\n<style>\n:root{color-scheme:light dark;--bg:#fff;--fg:#1c1f23;--mut:#5b6470;--line:#d8dde3}\n@media (prefers-color-scheme:dark){:root{--bg:#16191d;--fg:#e6e9ec;--mut:#9aa4af;--line:#323940}}\nbody{margin:0 auto;max-width:56rem;padding:1.5rem 1rem;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif}\nh1,h2,h3{line-height:1.25}.note{color:var(--mut);border-left:3px solid var(--line);padding-left:.75rem}\ntable{border-collapse:collapse}td,th{border:1px solid var(--line);padding:.25rem .75rem;text-align:left}\ncode{font:.9em ui-monospace,monospace}li{margin:.4rem 0}\n.sev{font-weight:600;text-transform:uppercase;font-size:.8em}\n</style></head><body>\n",
    );
    out.push_str(&format!(
        "<h1>Security audit: {}</h1>\n<p class=\"note\">{}</p>\n<p>{}.</p>\n",
        esc(&title_line(report)),
        esc(DISCLAIMER),
        esc(&summary_line(report))
    ));
    out.push_str("<table><thead><tr><th>Severity</th><th>Findings</th></tr></thead><tbody>\n");
    for (s, n) in counts(report) {
        out.push_str(&format!(
            "<tr><td>{}</td><td>{n}</td></tr>\n",
            severity_name(s)
        ));
    }
    out.push_str("</tbody></table>\n");
    if report.blocked > 0 {
        out.push_str(&format!(
            "<p>{} request(s) were refused by the send guard and are not audited.</p>\n",
            report.blocked
        ));
    }
    if !report.skipped.is_empty() {
        out.push_str("<p>Left out:</p><ul>\n");
        for s in &report.skipped {
            out.push_str(&format!(
                "<li><code>{}</code>: {}</li>\n",
                esc(&s.path),
                esc(&s.reason)
            ));
        }
        out.push_str("</ul>\n");
    }
    let groups = groups(report);
    for c in categories(&groups) {
        out.push_str(&format!("<h2>{} {}</h2>\n", esc(c), esc(category_title(c))));
        for g in groups.iter().filter(|g| g.category == c) {
            out.push_str(&format!(
                "<h3>{} <code>{}</code> <span class=\"sev\">{}</span></h3>\n<ul>\n",
                esc(&g.title),
                esc(g.rule),
                severity_name(g.severity)
            ));
            for o in &g.items {
                out.push_str(&format!(
                    "<li><code>{}</code> {}<br>{}: {}</li>\n",
                    esc(&o.request.path),
                    esc(&target(o.request)),
                    esc(&o.finding.location),
                    esc(&o.finding.why)
                ));
            }
            out.push_str("</ul>\n");
            if let Some(first) = g.items.first() {
                out.push_str(&format!("<p>Fix: {}</p>\n", esc(&first.finding.fix)));
            }
        }
    }
    out.push_str("</body></html>\n");
    out
}

fn json(report: &AuditReport) -> String {
    let groups = groups(report);
    let by_severity: serde_json::Map<String, serde_json::Value> = counts(report)
        .iter()
        .map(|(s, n)| (severity_name(*s).to_owned(), (*n).into()))
        .collect();
    let value = serde_json::json!({
        "version": 1,
        "tool": "rt security audit",
        "note": DISCLAIMER,
        "collection": report.collection,
        "environment": report.environment,
        "summary": {
            "requests": report.requests.len(),
            "findings": report.findings().count(),
            "blocked": report.blocked,
            "bySeverity": by_severity,
        },
        "rules": groups.iter().map(|g| serde_json::json!({
            "id": g.rule,
            "title": g.title,
            "category": g.category,
            "severity": severity_name(g.severity),
            "count": g.items.len(),
        })).collect::<Vec<_>>(),
        "findings": report.findings().map(|(r, f)| serde_json::json!({
            "rule": f.rule,
            "category": category_id(f.category),
            "severity": severity_name(f.severity),
            "request": r.path,
            "method": r.method,
            "url": r.url,
            "status": r.status,
            "location": f.location,
            "why": f.why,
            "fix": f.fix,
        })).collect::<Vec<_>>(),
        "requests": report.requests.iter().map(|r| serde_json::json!({
            "path": r.path,
            "method": r.method,
            "url": r.url,
            "status": r.status,
            "error": r.error,
            "findings": r.findings.len(),
        })).collect::<Vec<_>>(),
        "skipped": report.skipped,
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

/// SARIF 2.1.0: one rule per rule id (with its OWASP tag), one result per finding and request
/// with the request file as location.
fn sarif(report: &AuditReport) -> String {
    let groups = groups(report);
    let level = |s: Severity| match s {
        Severity::Info | Severity::Low => "note",
        Severity::Medium => "warning",
        Severity::High | Severity::Critical => "error",
    };
    // GitHub's `security-severity`: a CVSS-like score as a string.
    let score = |s: Severity| match s {
        Severity::Info => "0.0",
        Severity::Low => "3.0",
        Severity::Medium => "5.5",
        Severity::High => "8.0",
        Severity::Critical => "9.5",
    };
    let rules: Vec<_> = groups
        .iter()
        .map(|g| {
            serde_json::json!({
                "id": g.rule,
                "name": g.title,
                "shortDescription": { "text": g.title },
                "fullDescription": { "text": format!("{} {} ({}). {DISCLAIMER}", g.category, category_title(g.category), g.title) },
                "defaultConfiguration": { "level": level(g.severity) },
                "properties": {
                    "tags": ["security", format!("OWASP-{}", g.category)],
                    "security-severity": score(g.severity),
                },
            })
        })
        .collect();
    let results: Vec<_> = report
        .findings()
        .map(|(r, f)| {
            serde_json::json!({
                "ruleId": f.rule,
                "ruleIndex": groups.iter().position(|g| g.rule == f.rule).unwrap_or(0),
                "level": level(f.severity),
                "message": { "text": format!("{} {} Fix: {}", f.location, f.why, f.fix) },
                "locations": [{
                    "physicalLocation": { "artifactLocation": { "uri": r.path } },
                    "logicalLocations": [{ "name": format!("{} {}", r.method, r.url), "kind": "resource" }],
                }],
                "properties": { "category": category_id(f.category), "severity": severity_name(f.severity) },
            })
        })
        .collect();
    let value = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "rt security audit",
                "rules": rules,
            } },
            "results": results,
            "properties": { "note": DISCLAIMER, "collection": report.collection },
        }],
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}
