// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --all-pages [--max-pages N]` (wave 18, strand C): follows the pages of a reading
//! request (`rt_app::paging`) and prints one report with the merged body. The flags are parsed
//! in `lib.rs` into [`PagingArgs`]; [`send_pages`] is the hook `rt send` calls instead of the
//! plain send.
//!
//! The rule comes from `<request>.paging.json` next to the request file, else the collection's
//! `extensions.requesttailor.paging`, else a plugin's `contributes.paging` (installed plugins
//! and `--plugin` files, unless `--no-plugins`). No rule is a usage error. Every page is sent
//! like `rt send` sends (variables, auth, scripts, plugin hooks, the send guard); a guard "no"
//! ends the run with what was loaded. A rule without a merge path delivers the last page (the
//! notes say how many were followed). The hard limits of `rt_app::paging` always apply:
//! `--max-pages` above 1 000 is cut to 1 000.

use rt_app::paging::rules::{MatchContext, matches};
use rt_app::paging::{
    Follow, PagingLimits, PagingMode, PagingOrigin, PagingSpec, PluginPaging, follow,
};
use rt_core::model::RequestSpec;
use rt_core::prepare::PrepareInput;
use rt_core::send::{SendOptions, SendReport, send_with};
use rt_core::services::{Note, NoteLevel};
use rt_core::transport::Transport;
use rt_format::CollectionDoc;
use rt_format::paging_config::{companion_path, read_companion};

use crate::{DirFiles, SendArgs, oauth, secrets, services};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PagingArgs {
    /// `--all-pages`.
    pub all_pages: bool,
    /// `--max-pages N`; the hard maximum of `rt_app::paging` applies.
    pub max_pages: Option<u32>,
}

/// The paging rules of the installed plugins and `--plugin` files.
fn plugin_rules(args: &SendArgs) -> Vec<PluginPaging> {
    let mut manifests = Vec::new();
    if !args.no_plugins
        && let Some(dir) = rt_plugin::host::dir::PluginDir::shared()
    {
        let (plugins, _) = dir.load();
        manifests.extend(
            plugins
                .into_iter()
                .filter(|p| p.info.enabled)
                .map(|p| p.info.manifest),
        );
    }
    for path in &args.plugins {
        if let Ok(wasm) = std::fs::read(path)
            && let Ok(manifest) = rt_plugin::host::PluginHost::inspect(&wasm)
        {
            manifests.push(manifest);
        }
    }
    manifests
        .into_iter()
        .flat_map(|m| {
            let plugin = m.id.clone();
            m.contributes
                .paging
                .into_iter()
                .map(move |contribution| PluginPaging {
                    plugin: plugin.clone(),
                    contribution,
                })
        })
        .collect()
}

fn rule_from_files(
    doc: &CollectionDoc,
    request_path: &str,
    files: &DirFiles,
) -> Result<Option<(PagingSpec, PagingOrigin)>, String> {
    let companion = files.root().join(companion_path(request_path));
    if let Ok(text) = std::fs::read_to_string(&companion)
        && let Some(spec) =
            read_companion(&text).map_err(|e| format!("{}: {e}", companion.display()))?
    {
        return Ok(Some((spec, PagingOrigin::Request)));
    }
    Ok(doc.paging_config().map(|s| (s, PagingOrigin::Collection)))
}

fn warn(report: &mut SendReport, text: String) {
    report.notes.push(Note {
        source: "paging".into(),
        level: NoteLevel::Warning,
        text,
    });
}

/// Called by `rt send` for every request that is not a session. `Ok(None)`: send as usual.
/// `Ok(Some(report))`: the merged report to print. `Err(message)`: usage error (exit 2).
pub async fn send_pages<T: Transport>(
    args: &SendArgs,
    doc: &CollectionDoc,
    request_path: &str,
    files: &DirFiles,
    transport: &T,
) -> Result<Option<SendReport>, String> {
    if !args.paging.all_pages && args.paging.max_pages.is_none() {
        return Ok(None);
    }
    if !args.paging.all_pages {
        return Err("--max-pages needs --all-pages.".into());
    }
    let spec: RequestSpec = doc
        .model()
        .request(request_path)
        .map(|c| c.request.spec.clone())
        .ok_or_else(|| format!("{request_path}: no such request"))?;
    let input = PrepareInput {
        environment: args.environment.as_deref(),
        globals: Vec::new(),
        runtime: args.vars.clone(),
    };
    let options = SendOptions {
        iteration: None,
        body_limit: args.body_limit,
    };
    let mut collection = secrets::resolve_all(
        doc.model(),
        args.environment.as_deref(),
        &args.vars,
        &oauth::collection_id(files.root()),
    )
    .await
    .map_err(|e| format!("secrets: {e}"))?;
    let cli = services::CliServices::new(args, files.root())?;

    let mut limits = PagingLimits::default();
    let mut cut = None;
    if let Some(n) = args.paging.max_pages {
        if n == 0 {
            return Err("--max-pages must be at least 1.".into());
        }
        limits.max_pages = n;
        if n > rt_app::paging::HARD_MAX_PAGES {
            cut = Some(format!(
                "--max-pages {n} is above the maximum: cut to {}.",
                rt_app::paging::HARD_MAX_PAGES
            ));
        }
    }

    let mut first = None;
    let (rule, origin) = match rule_from_files(doc, request_path, files)? {
        Some(found) => found,
        None => {
            // Only a plugin can still know a rule: send the first page to see what it is.
            let plugins = plugin_rules(args);
            let page = send_with(
                &collection,
                request_path,
                &input,
                files,
                transport,
                &options,
                &cli.services(),
            )
            .await;
            let context = MatchContext {
                url: page
                    .request
                    .as_ref()
                    .map(|r| r.url.clone())
                    .unwrap_or_default(),
                content_type: page.response.as_ref().and_then(|r| {
                    r.headers
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                        .map(|(_, v)| v.clone())
                }),
            };
            let found = plugins
                .iter()
                .find(|p| matches(&p.contribution.matches, &context))
                .map(|p| {
                    (
                        p.contribution.rule.clone(),
                        PagingOrigin::Plugin {
                            plugin: p.plugin.clone(),
                            id: p.contribution.id.clone(),
                        },
                    )
                });
            let Some(found) = found else {
                return Err(format!(
                    "no paging rule for {request_path}: add {} next to it, or a \
                     `paging` block under extensions.requesttailor in opencollection.yml",
                    companion_path(request_path)
                ));
            };
            first = Some(page);
            found
        }
    };

    let result = follow(
        Follow {
            spec: &spec,
            rule: &rule,
            mode: PagingMode::All,
            previous: None,
            limits,
            first,
        },
        async |page: RequestSpec| {
            rt_app::paging::set_spec(&mut collection, request_path, page)
                .map_err(|e| e.to_string())?;
            Ok(send_with(
                &collection,
                request_path,
                &input,
                files,
                transport,
                &options,
                &cli.services(),
            )
            .await)
        },
        &|| false,
        &mut |_| {},
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut report = result.report;
    if let PagingOrigin::Plugin { plugin, .. } = &origin {
        report.notes.push(Note {
            source: "paging".into(),
            level: NoteLevel::Info,
            text: format!("The paging rule comes from the plugin {plugin}."),
        });
    }
    if rule.merge.is_none() && result.pages > 1 {
        warn(
            &mut report,
            format!(
                "The rule has no merge path: {} pages were followed and the last one is shown.",
                result.pages
            ),
        );
    }
    if let Some(text) = cut {
        warn(&mut report, text);
    }
    Ok(Some(report))
}
