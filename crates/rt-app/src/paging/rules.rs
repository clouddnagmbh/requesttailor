// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Which rule applies to a request, and saving one. Precedence: the request's companion
//! `<request>.paging.json`, then the collection default (`extensions.requesttailor.paging`),
//! then a plugin's `contributes.paging`. The first two are decisions of the user and always
//! win; a plugin's rule is only offered (the UI says "found by <plugin>" and the user adopts it
//! into the companion).

use rt_core::paging::{PagingRule, PagingSpec};
use rt_format::paging_config::{companion_path, read_companion, write_companion};
use serde_json_path::JsonPath;
use url::Url;

use super::{EffectivePaging, PagingOrigin, PagingRuleScope, PluginPaging};
use crate::AppError;
use crate::workspace::Workspace;

/// What a plugin's rule is matched against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchContext {
    /// The URL of the request (as sent when known, else as written).
    pub url: String,
    /// The media type of the response, if there is one.
    pub content_type: Option<String>,
}

/// `*.example.com` and exact host names, case-insensitive.
fn host_matches(pattern: &str, host: &str) -> bool {
    let (pattern, host) = (pattern.to_ascii_lowercase(), host.to_ascii_lowercase());
    match pattern.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() && host.ends_with(&format!(".{suffix}")),
        None => pattern == host,
    }
}

/// A glob with `*` for any characters.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else {
                return false;
            };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(at) = rest.find(part) else {
                return false;
            };
            rest = &rest[at + part.len()..];
        }
    }
    rest.is_empty()
}

/// Whether the plugin rule is for this request or response (all given conditions hold).
pub fn matches(rule: &rt_plugin::api::PagingMatch, context: &MatchContext) -> bool {
    let url = Url::parse(&context.url).ok();
    if !rule.hosts.is_empty() {
        let host = url.as_ref().and_then(|u| u.host_str());
        if !host.is_some_and(|h| rule.hosts.iter().any(|p| host_matches(p, h))) {
            return false;
        }
    }
    if !rule.content_types.is_empty() {
        let Some(media) = context.content_type.as_deref().map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        }) else {
            return false;
        };
        if !rule
            .content_types
            .iter()
            .any(|c| c.trim().to_ascii_lowercase() == media)
        {
            return false;
        }
    }
    if let Some(pattern) = &rule.url_pattern {
        let Some(url) = &url else {
            return false;
        };
        let mut target = url.path().to_owned();
        if let Some(q) = url.query() {
            target.push('?');
            target.push_str(q);
        }
        if !glob(pattern, &target) {
            return false;
        }
    }
    true
}

/// The rule of the request's companion: `Ok(None)` if there is none (or it says "no rule").
fn companion_rule(ws: &Workspace, path: &str) -> Result<Option<PagingSpec>, AppError> {
    let Some(bytes) = ws.companion_file(&companion_path(path)) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(bytes);
    read_companion(&text).map_err(|e| AppError::Shell(e.to_string()))
}

/// The rule in force for the request at `path`, with where it comes from. `plugins` are the
/// contributions of the active plugins; `context` is what they are matched against (without
/// it a plugin's rule never applies).
pub fn effective(
    ws: &Workspace,
    path: &str,
    plugins: &[PluginPaging],
    context: Option<&MatchContext>,
) -> Result<Option<EffectivePaging>, AppError> {
    if let Some(spec) = companion_rule(ws, path)? {
        return Ok(Some(EffectivePaging {
            spec,
            origin: PagingOrigin::Request,
        }));
    }
    if let Some(spec) = ws.doc().paging_config() {
        return Ok(Some(EffectivePaging {
            spec,
            origin: PagingOrigin::Collection,
        }));
    }
    let Some(context) = context else {
        return Ok(None);
    };
    Ok(plugins
        .iter()
        .find(|p| matches(&p.contribution.matches, context))
        .map(|p| EffectivePaging {
            spec: p.contribution.rule.clone(),
            origin: PagingOrigin::Plugin {
                plugin: p.plugin.clone(),
                id: p.contribution.id.clone(),
            },
        }))
}

/// Checks a rule: its paths are JSONPath, the merge path too, a cursor has a name.
pub fn validate(spec: &PagingSpec) -> Result<(), AppError> {
    let path = |what: &str, text: &str| -> Result<(), AppError> {
        JsonPath::parse(text)
            .map(drop)
            .map_err(|e| AppError::Shell(format!("{what} {text:?} is not a JSONPath: {e}")))
    };
    match &spec.rule {
        PagingRule::LinkHeader => {}
        PagingRule::JsonUrl { next } => path("The next-page path", next)?,
        PagingRule::Cursor { next, param } => {
            path("The cursor path", next)?;
            if param.name.trim().is_empty() {
                return Err(AppError::Shell("A cursor needs a parameter name.".into()));
            }
        }
    }
    if let Some(merge) = &spec.merge {
        path("The merge path", merge)?;
    }
    Ok(())
}

/// Saves the rule (unsaved, like every edit): into the request's companion, or as the default
/// of the collection. `None` removes it (a request's companion then says `{}`). A rule that is
/// not valid is refused.
pub fn set(
    ws: &mut Workspace,
    path: &str,
    spec: Option<&PagingSpec>,
    scope: PagingRuleScope,
) -> Result<(), AppError> {
    if let Some(spec) = spec {
        validate(spec)?;
    }
    match scope {
        PagingRuleScope::Request => {
            if ws.request(path).is_err() {
                return Err(AppError::Shell(format!("There is no request {path}.")));
            }
            let target = companion_path(path);
            if spec.is_none() && ws.companion_file(&target).is_none() {
                return Ok(());
            }
            ws.stage_companion(&target, write_companion(spec).into_bytes())
        }
        PagingRuleScope::Collection => {
            ws.doc_mut().set_paging_config(spec)?;
            ws.refresh();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_plugin::api::PagingMatch;

    fn ctx(url: &str, ct: Option<&str>) -> MatchContext {
        MatchContext {
            url: url.into(),
            content_type: ct.map(str::to_owned),
        }
    }

    #[test]
    fn plugin_rules_match_on_all_given_conditions() {
        let m = PagingMatch {
            hosts: vec!["*.example.com".into()],
            content_types: vec!["application/json".into()],
            url_pattern: Some("/api/*?*".into()),
        };
        assert!(matches(
            &m,
            &ctx(
                "https://a.example.com/api/x?y=1",
                Some("Application/JSON; charset=utf-8")
            )
        ));
        assert!(!matches(
            &m,
            &ctx("https://example.com/api/x?y=1", Some("application/json"))
        ));
        assert!(!matches(
            &m,
            &ctx("https://a.example.com/api/x?y=1", Some("text/plain"))
        ));
        assert!(!matches(
            &m,
            &ctx("https://a.example.com/other", Some("application/json"))
        ));
        assert!(!matches(&m, &ctx("https://a.example.com/api/x?y=1", None)));
        assert!(matches(&PagingMatch::default(), &ctx("not a url", None)));
    }
}
