// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Turns a `RequestContext` into a wire-ready `HttpRequest`: builds the variable scope,
//! interpolates, merges inherited headers and settings, resolves auth inheritance and encodes
//! the body.
//!
//! Owner: wave 2, strand C.
//!
//! Order of work, which is also the order of `used` and `unresolved`: URL (template, path
//! params, query params), headers, auth, body.
//!
//! - Scope: globals, collection, environment, folders (outermost first), request, runtime.
//!   Disabled variables and variables without a value are left out, so a secret whose value
//!   is not filled in reports as unresolved. A runtime value for a name that a file declares
//!   secret is flagged secret too.
//! - Headers: collection, then folders (outermost first), then the request; enabled only,
//!   names and values interpolated. A later layer overrides a header of the same name
//!   (case-insensitive) in place, as axios does with Bruno's merged header object.
//! - Settings: request, then folders (innermost first), then collection, then the defaults
//!   (no timeout, follow redirects, at most 5, encode the URL). `timeout_ms: Some(0)` means
//!   no timeout.
//! - Auth, URL and body rules: see the `auth`, `uri` and `body` modules.

mod auth;
mod body;
mod uri;

use std::time::Duration;

use crate::model::{Auth, KeyValue, RequestContext, Settings, Variable};
use crate::transport::{HttpRequest, SendOptions};
use crate::vars::{Scope, VarLayer, VarSource, VarUse, dedup_by_key};

/// Reads files referenced by bodies (file body, multipart file parts), relative to the
/// collection root. Native: the file system. Browser: the in-memory file set.
pub trait FileSource {
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
}

/// Inputs that are not in the collection files.
#[derive(Debug, Clone, Default)]
pub struct PrepareInput<'a> {
    /// Environment to use, by name. `None`: no environment layer.
    pub environment: Option<&'a str>,
    /// Global variables (lowest layer).
    pub globals: Vec<(String, String)>,
    /// Runtime variables (highest layer), e.g. `--var name=value`.
    pub runtime: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub request: HttpRequest,
    /// The effective auth after inheritance, before interpolation (for display).
    pub auth: Auth,
    pub scope: Scope,
    pub used: Vec<VarUse>,
    /// Placeholders that had no value anywhere, deduplicated, in first-seen order.
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrepareError {
    #[error("environment \"{0}\" not found")]
    UnknownEnvironment(String),
    #[error("auth type \"{0}\" is not supported yet")]
    UnsupportedAuth(String),
    #[error("invalid URL after variable substitution: {0}")]
    InvalidUrl(String),
    #[error("cannot read body file \"{path}\": {message}")]
    BodyFile { path: String, message: String },
}

pub fn prepare(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
) -> Result<Prepared, PrepareError> {
    let scope = build_scope(context, input)?;
    let effective_auth = auth::effective(context)?;
    let spec = &context.request.spec;

    let (request, used, unresolved) = {
        let mut interp = Interp {
            scope: &scope,
            used: Vec::new(),
            unresolved: Vec::new(),
        };
        let encode = setting(context, |s| s.encode_url).unwrap_or(true);
        let url = uri::draft(&spec.url, &spec.params, encode, &mut interp);
        let url_unresolved = interp.unresolved.clone();

        let layers = std::iter::once(&context.collection.defaults.headers)
            .chain(context.folders.iter().map(|f| &f.defaults.headers))
            .chain(std::iter::once(&spec.headers));
        let mut headers = merge_headers(layers, &mut interp);
        let api_key_query = auth::apply(&effective_auth, &mut headers, &mut interp);
        let body = body::encode(&spec.body, &mut headers, &mut interp, files)?;
        let url = url.finish(api_key_query, &url_unresolved)?;

        let options = SendOptions {
            client_identity: None,
            timeout: match setting(context, |s| s.timeout_ms) {
                None | Some(0) => None,
                Some(ms) => Some(Duration::from_millis(ms)),
            },
            follow_redirects: setting(context, |s| s.follow_redirects).unwrap_or(true),
            max_redirects: setting(context, |s| s.max_redirects).unwrap_or(5),
        };
        let request = HttpRequest {
            method: spec.method.clone(),
            url,
            headers,
            body,
            options,
        };
        let Interp {
            mut used,
            mut unresolved,
            ..
        } = interp;
        dedup_by_key(&mut used, |u| u.name.clone());
        dedup_by_key(&mut unresolved, Clone::clone);
        (request, used, unresolved)
    };

    Ok(Prepared {
        request,
        auth: effective_auth,
        scope,
        used,
        unresolved,
    })
}

/// Interpolates templates against one scope and collects what was used and missing.
struct Interp<'s> {
    scope: &'s Scope,
    used: Vec<VarUse>,
    unresolved: Vec<String>,
}

impl Interp<'_> {
    fn text(&mut self, template: &str) -> String {
        if !template.contains("{{") {
            return template.to_owned();
        }
        let i = self.scope.interpolate(template);
        self.used.extend(i.used);
        self.unresolved.extend(i.unresolved);
        i.value
    }
}

fn build_scope(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
) -> Result<Scope, PrepareError> {
    let environment = input
        .environment
        .map(|name| {
            context
                .collection
                .environment(name)
                .ok_or_else(|| PrepareError::UnknownEnvironment(name.to_owned()))
        })
        .transpose()?;

    let from_file = |source: VarSource, vars: &[Variable]| VarLayer {
        source,
        vars: vars
            .iter()
            .filter(|v| v.enabled)
            .filter_map(|v| Some((v.name.clone(), v.value.clone()?, v.secret)))
            .collect(),
    };
    let from_input = |source: VarSource, vars: &[(String, String)]| VarLayer {
        source,
        vars: vars
            .iter()
            .map(|(n, v)| (n.clone(), v.clone(), false))
            .collect(),
    };

    let mut layers = vec![
        from_input(VarSource::Global, &input.globals),
        from_file(VarSource::Collection, &context.collection.defaults.vars),
    ];
    if let Some(env) = environment {
        layers.push(from_file(
            VarSource::Environment {
                name: env.name.clone(),
            },
            &env.variables,
        ));
    }
    for folder in &context.folders {
        layers.push(from_file(
            VarSource::Folder {
                path: folder.path.clone(),
            },
            &folder.defaults.vars,
        ));
    }
    layers.push(from_file(VarSource::Request, &context.request.spec.vars));

    // A value given at run time for a name declared secret in a file stays secret.
    let declared_secret = |name: &str| {
        context
            .collection
            .defaults
            .vars
            .iter()
            .chain(environment.iter().flat_map(|e| &e.variables))
            .chain(context.folders.iter().flat_map(|f| &f.defaults.vars))
            .chain(&context.request.spec.vars)
            .any(|v| v.secret && v.name == name)
    };
    let mut runtime = from_input(VarSource::Runtime, &input.runtime);
    for (name, _, secret) in &mut runtime.vars {
        *secret = declared_secret(name);
    }
    layers.push(runtime);

    Ok(Scope { layers })
}

/// The first value of a setting from request, folders (innermost first), collection.
fn setting<T>(context: &RequestContext<'_>, get: impl Fn(&Settings) -> Option<T>) -> Option<T> {
    get(&context.request.spec.settings)
        .or_else(|| {
            context
                .folders
                .iter()
                .rev()
                .find_map(|f| get(&f.defaults.settings))
        })
        .or_else(|| get(&context.collection.defaults.settings))
}

fn merge_headers<'a>(
    layers: impl Iterator<Item = &'a Vec<KeyValue>>,
    interp: &mut Interp<'_>,
) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for header in layers.flatten().filter(|h| h.enabled && !h.name.is_empty()) {
        let name = interp.text(&header.name);
        let value = interp.text(&header.value);
        if name.is_empty() {
            continue;
        }
        match headers
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        {
            Some(existing) => existing.1 = value,
            None => headers.push((name, value)),
        }
    }
    headers
}

/// Sets a header, replacing every header of the same name (case-insensitive). Keeps the
/// position of the first one it replaces.
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    match headers
        .iter()
        .position(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        Some(first) => {
            headers[first] = (name.to_owned(), value);
            let mut index = 0;
            headers.retain(|(n, _)| {
                let keep = index <= first || !n.eq_ignore_ascii_case(name);
                index += 1;
                keep
            });
        }
        None => headers.push((name.to_owned(), value)),
    }
}

#[cfg(test)]
mod tests;
