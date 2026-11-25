// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt plugin sources add|remove|list | search <text> | install <id|url> [--source S] [--yes
//! --sha256 X] | update [--check] | verify | remove <id> | list` (wave 18, strand I): the
//! plugin catalogue without a server (`rt_app::catalogue`). Nothing is requested before the
//! user runs one of these. `--yes` only together with `--sha256`.
//!
//! Exit codes: 0 done, 1 a download, checksum or install failed (or `verify` found a module
//! that is not the installed one), 2 usage. Installed plugins are the ones the desktop app
//! reads (`RT_DATA_DIR/plugins` or the app's data folder); the app picks a new one up when it
//! starts.

use std::io::{BufRead, Write};

use rt_app::catalogue::disk::DiskCatalogue;
use rt_app::catalogue::{
    CatalogueEntry, CatalogueSource, CatalogueWasm, Fetched, GitSource, InstalledState,
    SourceIndex, check_updates, fetch_index, fetch_wasm, matches, plan_install, resolve_wasm_url,
    sha256_hex, verify_installed,
};
use rt_app::plugins::Permissions;
use rt_plugin::host::PluginHost;
use rt_plugin::host::dir::PluginDir;

pub const USAGE: &str = "  rt plugin sources add|remove|list | search <text> | install <id|url> [--source <S>]\n            [--yes --sha256 <HEX>] | update [--check] | verify | remove <id> | list\n";

pub const HELP: &str = "rt plugin installs plugins (WebAssembly only) from sources you add yourself: an HTTPS\nlink to a plugins.json or a Git repository that has one. There is no built-in index\nand nothing is requested until you run a command. Before installing, the SHA-256 of\nthe download must equal the index's, and the rights shown are read from the plugin\nitself; --yes skips the question only together with --sha256.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SourcesAdd(String),
    SourcesRemove(String),
    SourcesList,
    Search(String),
    Install {
        target: String,
        source: Option<String>,
        yes: bool,
        sha256: Option<String>,
    },
    Update {
        check: bool,
    },
    Verify,
    Remove(String),
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub action: Action,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let err = |e: lexopt::Error| e.to_string();
    let mut words: Vec<String> = Vec::new();
    let (mut source, mut yes, mut sha256, mut check) = (None, false, None, false);
    while let Some(arg) = parser.next().map_err(err)? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("source") => source = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("yes") => yes = true,
            Long("sha256") => sha256 = Some(parser.value().map_err(err)?.string().map_err(err)?),
            Long("check") => check = true,
            Value(v) => words.push(v.string().map_err(err)?),
            other => return Err(other.unexpected().to_string()),
        }
    }
    if yes && sha256.is_none() {
        return Err("--yes needs --sha256 <HEX>: install what you have verified".into());
    }
    let mut it = words.into_iter();
    let one = |it: &mut std::vec::IntoIter<String>, what: &str| {
        it.next().ok_or_else(|| format!("plugin: missing {what}"))
    };
    let action = match it.next().as_deref() {
        Some("sources") => match it.next().as_deref() {
            Some("add") => Action::SourcesAdd(one(&mut it, "the source URL")?),
            Some("remove") => Action::SourcesRemove(one(&mut it, "the source URL")?),
            Some("list") => Action::SourcesList,
            _ => return Err("plugin sources: expected add, remove or list".into()),
        },
        Some("search") => Action::Search(one(&mut it, "the search text")?),
        Some("install") => Action::Install {
            target: one(&mut it, "the plugin id or URL")?,
            source,
            yes,
            sha256,
        },
        Some("update") => Action::Update { check },
        Some("verify") => Action::Verify,
        Some("remove") => Action::Remove(one(&mut it, "the plugin id")?),
        Some("list") => Action::List,
        Some(other) => return Err(format!("plugin: unknown command {other:?}")),
        None => return Err("plugin: missing command".into()),
    };
    if let Some(extra) = it.next() {
        return Err(format!("plugin: unexpected argument {extra:?}"));
    }
    Ok(Some(Args { action }))
}

/// Reads a Git source by cloning it with `rt-git` (the raw-file address did not answer): the
/// bytes of `plugins.json` at the ref (default `HEAD`) and the commit they were read at.
pub fn clone_index(git: &GitSource) -> Result<(Vec<u8>, String), String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let parent = std::env::temp_dir().join(format!(
        "rt-plugin-source-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let result = (|| {
        let folder = rt_git::clone(
            &git.url,
            &parent,
            &rt_git::NoCredentials,
            &rt_git::Options::default(),
        )
        .map_err(|e| e.to_string())?;
        let repo = rt_git::Repo::open(&folder, &rt_git::Options::default())
            .map_err(|e| e.to_string())?
            .ok_or("the clone is not a repository")?;
        let want = git.git_ref.as_deref().unwrap_or("HEAD");
        let commit = repo
            .rev_parse(want)
            .or_else(|_| repo.rev_parse(&format!("origin/{want}")))
            .map_err(|e| e.to_string())?;
        let text = repo
            .show(&commit, "/plugins.json")
            .map_err(|e| e.to_string())?
            .ok_or("the repository has no plugins.json in its root")?;
        Ok((text.into_bytes(), commit))
    })();
    let _ = std::fs::remove_dir_all(&parent);
    result
}

/// What the user types after a question.
type Ask<'a> = &'a mut dyn FnMut(&str) -> bool;

fn rights(p: &Permissions) -> String {
    let mut parts = Vec::new();
    if !p.network.is_empty() {
        parts.push(format!("network: {}", p.network.join(", ")));
    }
    if p.storage {
        parts.push("storage".to_owned());
    }
    if p.variables {
        parts.push("variables".to_owned());
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join("; ")
    }
}

struct Ctx<'a> {
    disk: DiskCatalogue,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start runtime: {e}"))
}

fn transport() -> Result<rt_core::transport::native::NativeTransport, String> {
    rt_core::transport::native::NativeTransport::new().map_err(|e| e.to_string())
}

/// Refreshes one source (the user ran a command that needs its index).
fn refresh(
    rt: &tokio::runtime::Runtime,
    t: &rt_core::transport::native::NativeTransport,
    source: &str,
) -> Result<Fetched, String> {
    rt.block_on(fetch_index(t, source, Some(&clone_index)))
        .map_err(|e| e.to_string())
}

/// `run` with the answers read from standard input.
pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let mut ask = |question: &str| {
        let _ = write!(std::io::stderr(), "{question} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).is_ok()
            && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    };
    run_with(args, stdout, stderr, &mut ask)
}

/// `run` with `ask` answering the questions (tests).
pub fn run_with(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write, ask: Ask<'_>) -> i32 {
    let Some(dir) = PluginDir::shared() else {
        let _ = writeln!(
            stderr,
            "rt: plugin: there is no data folder (set RT_DATA_DIR)"
        );
        return crate::EXIT_USAGE;
    };
    let mut ctx = Ctx {
        disk: DiskCatalogue::new(dir),
        out: stdout,
        err: stderr,
    };
    let result = match args.action {
        Action::SourcesAdd(url) => sources_add(&mut ctx, &url),
        Action::SourcesRemove(url) => ctx
            .disk
            .remove_source(&url)
            .map(|()| {
                let _ = writeln!(ctx.out, "Removed {url}");
                crate::EXIT_OK
            })
            .map_err(|e| Failure::usage(e.to_string())),
        Action::SourcesList => sources_list(&mut ctx),
        Action::Search(text) => search(&mut ctx, &text),
        Action::Install {
            target,
            source,
            yes,
            sha256,
        } => install(
            &mut ctx,
            &target,
            source.as_deref(),
            yes,
            sha256.as_deref(),
            ask,
        ),
        Action::Update { check } => update(&mut ctx, check, ask),
        Action::Verify => verify(&mut ctx),
        Action::Remove(id) => remove(&mut ctx, &id),
        Action::List => list(&mut ctx),
    };
    match result {
        Ok(code) => code,
        Err(f) => {
            let _ = writeln!(ctx.err, "rt: plugin: {}", f.message);
            f.code
        }
    }
}

struct Failure {
    code: i32,
    message: String,
}

impl Failure {
    fn usage(message: String) -> Self {
        Self {
            code: crate::EXIT_USAGE,
            message,
        }
    }
    fn failed(message: String) -> Self {
        Self {
            code: crate::EXIT_TRANSPORT,
            message,
        }
    }
}

type Outcome = Result<i32, Failure>;

fn sources_add(ctx: &mut Ctx<'_>, url: &str) -> Outcome {
    let source = ctx
        .disk
        .add_source(url)
        .map_err(|e| Failure::usage(e.to_string()))?;
    let _ = writeln!(
        ctx.out,
        "Added {}\nNothing was requested. `rt plugin search <text>` reads the index.",
        source.url
    );
    Ok(crate::EXIT_OK)
}

fn source_line(s: &CatalogueSource) -> String {
    let mut line = s.url.clone();
    if let Some(name) = &s.name {
        line.push_str(&format!("  ({name})"));
    }
    if let Some(commit) = &s.commit {
        line.push_str(&format!("  index at {commit}"));
    }
    line
}

fn sources_list(ctx: &mut Ctx<'_>) -> Outcome {
    let sources = ctx.disk.sources();
    if sources.is_empty() {
        let _ = writeln!(
            ctx.out,
            "No sources. There is no built-in index; add one with `rt plugin sources add <url>`."
        );
    }
    for s in &sources {
        let _ = writeln!(ctx.out, "{}", source_line(s));
    }
    Ok(crate::EXIT_OK)
}

fn list(ctx: &mut Ctx<'_>) -> Outcome {
    let records = ctx.disk.dir().records();
    if records.is_empty() {
        let _ = writeln!(ctx.out, "No plugins installed.");
    }
    for r in records {
        let from = match &r.origin {
            Some(o) => format!("from {}  sha256 {}", o.source, o.sha256),
            None => "from a file".to_owned(),
        };
        let _ = writeln!(
            ctx.out,
            "{}  {}  {}  {}",
            r.id,
            r.version,
            if r.enabled { "on" } else { "off" },
            from
        );
    }
    Ok(crate::EXIT_OK)
}

fn remove(ctx: &mut Ctx<'_>, id: &str) -> Outcome {
    ctx.disk
        .dir()
        .remove(id)
        .map_err(|e| Failure::failed(e.to_string()))?;
    let _ = writeln!(ctx.out, "Removed {id}");
    Ok(crate::EXIT_OK)
}

fn verify(ctx: &mut Ctx<'_>) -> Outcome {
    let mut bad = false;
    let records = ctx.disk.dir().records();
    if records.is_empty() {
        let _ = writeln!(ctx.out, "No plugins installed.");
    }
    for r in records {
        let line = match (&r.origin, ctx.disk.dir().module_bytes(&r.id)) {
            (_, None) => {
                bad = true;
                "MISSING (the module is gone)".to_owned()
            }
            (None, Some(_)) => "no checksum (installed from a file)".to_owned(),
            (Some(o), Some(bytes)) if verify_installed(&o.sha256, &bytes) => {
                format!("ok  sha256 {}", o.sha256)
            }
            (Some(o), Some(bytes)) => {
                bad = true;
                format!(
                    "MISMATCH  installed from {} with sha256 {}, the module on disk is {}",
                    o.source,
                    o.sha256,
                    sha256_hex(&bytes)
                )
            }
        };
        let _ = writeln!(ctx.out, "{}  {}  {line}", r.id, r.version);
    }
    Ok(if bad {
        crate::EXIT_TRANSPORT
    } else {
        crate::EXIT_OK
    })
}

fn sources_to_read(ctx: &Ctx<'_>) -> Result<Vec<CatalogueSource>, Failure> {
    let sources = ctx.disk.sources();
    if sources.is_empty() {
        return Err(Failure::usage(
            "no sources: there is no built-in index. Add one with `rt plugin sources add <url>`"
                .into(),
        ));
    }
    Ok(sources)
}

/// Refreshes every source in `sources`; a source that fails is reported and skipped.
fn refresh_all(ctx: &mut Ctx<'_>, sources: &[CatalogueSource]) -> Result<Vec<Fetched>, Failure> {
    let rt = runtime().map_err(Failure::failed)?;
    let t = transport().map_err(Failure::failed)?;
    let mut out = Vec::new();
    for s in sources {
        match refresh(&rt, &t, &s.url) {
            Ok(f) => {
                let _ = ctx.disk.remember(&f.index.source);
                out.push(f);
            }
            Err(e) => {
                let _ = writeln!(ctx.err, "rt: plugin: {e}");
            }
        }
    }
    if out.is_empty() {
        return Err(Failure::failed("no source could be read".into()));
    }
    Ok(out)
}

fn search(ctx: &mut Ctx<'_>, text: &str) -> Outcome {
    let sources = sources_to_read(ctx)?;
    let fetched = refresh_all(ctx, &sources)?;
    let mut found = 0;
    for f in &fetched {
        for e in f.index.index.plugins.iter().filter(|e| matches(e, text)) {
            found += 1;
            let _ = writeln!(
                ctx.out,
                "{}  {}  {}  [{}]\n    {}\n    rights as the index says: {}",
                e.id,
                e.version,
                e.name,
                f.index
                    .source
                    .name
                    .as_deref()
                    .unwrap_or(&f.index.source.url),
                e.description,
                rights(&e.permissions)
            );
        }
    }
    if found == 0 {
        let _ = writeln!(ctx.out, "No plugin matches {text:?}.");
    }
    Ok(crate::EXIT_OK)
}

/// A module someone gave an address for, without an index: the entry is built from the
/// module's own manifest.
fn entry_of_module(wasm: &[u8], expected: &str, url: &str) -> Result<CatalogueEntry, Failure> {
    let m = PluginHost::inspect(wasm)
        .map_err(|e| Failure::failed(format!("{url}: this is not a usable plugin: {e}")))?;
    Ok(CatalogueEntry {
        id: m.id,
        name: m.name,
        version: m.version,
        api: m.api,
        description: m.description,
        license: m.license,
        homepage: m.homepage,
        wasm: CatalogueWasm {
            url: url.to_owned(),
            sha256: expected.to_owned(),
            size: wasm.len() as u64,
        },
        permissions: m.permissions,
    })
}

fn install(
    ctx: &mut Ctx<'_>,
    target: &str,
    source: Option<&str>,
    yes: bool,
    sha256: Option<&str>,
    ask: Ask<'_>,
) -> Outcome {
    let expected = sha256.map(|s| s.trim().to_ascii_lowercase());
    if let Some(s) = &expected
        && (s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Failure::usage(
            "--sha256 is the 64 hexadecimal digits of the module's SHA-256".into(),
        ));
    }
    let rt = runtime().map_err(Failure::failed)?;
    let t = transport().map_err(Failure::failed)?;
    let direct = target.starts_with("https://") || target.starts_with("http://");
    let (source_url, entry, wasm_url, bytes) = if direct {
        let bytes = rt
            .block_on(fetch_wasm(&t, target))
            .map_err(|e| Failure::failed(e.to_string()))?;
        // Without an index the checksum is the one the user gave, or the one just computed
        // (shown in the question; `--yes` cannot be used then).
        let sum = expected.clone().unwrap_or_else(|| sha256_hex(&bytes));
        let entry = entry_of_module(&bytes, &sum, target)?;
        (target.to_owned(), entry, target.to_owned(), bytes)
    } else {
        let candidates: Vec<String> = match source {
            Some(s) => vec![s.to_owned()],
            None => sources_to_read(ctx)?.into_iter().map(|s| s.url).collect(),
        };
        let mut hits: Vec<(Fetched, CatalogueEntry)> = Vec::new();
        for url in &candidates {
            match refresh(&rt, &t, url) {
                Ok(f) => {
                    let _ = ctx.disk.remember(&f.index.source);
                    if let Some(e) = f.index.index.plugins.iter().find(|e| e.id == target) {
                        let e = e.clone();
                        hits.push((f, e));
                    }
                }
                Err(e) => {
                    let _ = writeln!(ctx.err, "rt: plugin: {e}");
                }
            }
        }
        if hits.len() > 1 {
            return Err(Failure::usage(format!(
                "{target} is in more than one source; choose with --source <url>"
            )));
        }
        let Some((fetched, entry)) = hits.pop() else {
            return Err(Failure::failed(format!(
                "no source lists a plugin {target:?}"
            )));
        };
        if let Some(sum) = &expected
            && *sum != entry.wasm.sha256
        {
            return Err(Failure::failed(format!(
                "{target}: the index lists the checksum {}, not the {sum} you verified. Nothing was installed.",
                entry.wasm.sha256
            )));
        }
        let url = resolve_wasm_url(&fetched.index_url, &entry)
            .map_err(|e| Failure::failed(e.to_string()))?;
        let bytes = rt
            .block_on(fetch_wasm(&t, &url))
            .map_err(|e| Failure::failed(e.to_string()))?;
        (fetched.index.source.url, entry, url, bytes)
    };
    finish_install(ctx, &source_url, &entry, &wasm_url, &bytes, yes, ask)
}

/// Checks the module, shows what it asks for and installs after the answer.
fn finish_install(
    ctx: &mut Ctx<'_>,
    source: &str,
    entry: &CatalogueEntry,
    wasm_url: &str,
    bytes: &[u8],
    yes: bool,
    ask: Ask<'_>,
) -> Outcome {
    let installed: Option<InstalledState> = ctx.disk.installed_state(&entry.id);
    let plan = plan_install(source, wasm_url, entry, bytes, installed.as_ref())
        .map_err(|e| Failure::failed(e.to_string()))?;
    let _ = writeln!(
        ctx.out,
        "{} {} ({}){}\n  source   {}\n  sha256   {}\n  rights   {}  (read from the plugin itself)",
        entry.id,
        entry.version,
        entry.name,
        if plan.update { "  update" } else { "" },
        source,
        plan.sha256,
        rights(&plan.permissions)
    );
    for w in &plan.warnings {
        let _ = writeln!(ctx.out, "  warning  {w}");
    }
    if plan.more_permissions {
        let _ = writeln!(
            ctx.out,
            "  This update asks for more than you granted before."
        );
    }
    // `--yes` is only allowed with a verified checksum (the parser), and never skips the
    // question about more rights.
    let confirmed = if yes && !plan.more_permissions {
        true
    } else {
        ask(&format!("Install {} {}?", entry.id, entry.version))
    };
    if !confirmed {
        let _ = writeln!(ctx.out, "Not installed.");
        return Ok(crate::EXIT_USAGE);
    }
    ctx.disk
        .install(bytes, source, &plan.sha256)
        .map_err(|e| Failure::failed(e.to_string()))?;
    let _ = writeln!(
        ctx.out,
        "Installed {} {}. The app picks it up when it starts.",
        entry.id, entry.version
    );
    Ok(crate::EXIT_OK)
}

fn update(ctx: &mut Ctx<'_>, check: bool, ask: Ask<'_>) -> Outcome {
    let installed = ctx.disk.installed_sources();
    if installed.is_empty() {
        let _ = writeln!(
            ctx.out,
            "Nothing was installed from a source; nothing to update."
        );
        return Ok(crate::EXIT_OK);
    }
    let mut wanted: Vec<CatalogueSource> = Vec::new();
    for i in &installed {
        if !wanted.iter().any(|s| s.url == i.source) {
            wanted.push(CatalogueSource {
                url: i.source.clone(),
                commit: None,
                name: None,
            });
        }
    }
    let fetched = refresh_all(ctx, &wanted)?;
    let indexes: Vec<SourceIndex> = fetched.iter().map(|f| f.index.clone()).collect();
    let updates = check_updates(&indexes, &installed);
    if updates.is_empty() {
        let _ = writeln!(ctx.out, "Everything is up to date.");
        return Ok(crate::EXIT_OK);
    }
    for u in &updates {
        let _ = writeln!(
            ctx.out,
            "{}  {} -> {}  ({})",
            u.plugin, u.installed, u.available, u.source
        );
    }
    if check {
        return Ok(crate::EXIT_OK);
    }
    let rt = runtime().map_err(Failure::failed)?;
    let t = transport().map_err(Failure::failed)?;
    let mut code = crate::EXIT_OK;
    for u in updates {
        let Some(f) = fetched.iter().find(|f| f.index.source.url == u.source) else {
            continue;
        };
        let Some(entry) = f.index.index.plugins.iter().find(|e| e.id == u.plugin) else {
            continue;
        };
        let step = resolve_wasm_url(&f.index_url, entry)
            .map_err(|e| Failure::failed(e.to_string()))
            .and_then(|url| {
                let bytes = rt
                    .block_on(fetch_wasm(&t, &url))
                    .map_err(|e| Failure::failed(e.to_string()))?;
                finish_install(ctx, &u.source, entry, &url, &bytes, false, ask)
            });
        match step {
            Ok(c) if c != crate::EXIT_OK => code = c,
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(ctx.err, "rt: plugin: {}", e.message);
                code = e.code;
            }
        }
    }
    Ok(code)
}
