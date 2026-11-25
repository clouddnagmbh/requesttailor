// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A [`DebugInput`] for a request of a collection: what a shell does for
//! `DebuggerApi.start`. The scripts and the variables are the ones a send would give the
//! script runner (`rt_core::script::{scripts_for, request_of, variables_of}`): the request as
//! written, the scope of the environment (secret values included: the script may use them),
//! and for `after-response` and `tests` a response kept earlier.

use rt_core::model::{Collection, ScriptPhase};
use rt_core::prepare::PrepareInput;
use rt_core::script::{
    ScriptInfo, ScriptResponse, phase_name, request_of, scripts_for, variables_of,
};

use super::{DebugInput, DebugTarget};

/// What the shell hands over besides the target.
pub struct PrepareArgs<'a> {
    /// The collection with the secret values of the environment in it (a send's snapshot).
    pub collection: &'a Collection,
    /// Runtime variables (the session's, then the user's), as a send gets them.
    pub runtime: Vec<(String, String)>,
    /// The kept response, for `after-response` and `tests`.
    pub response: Option<ScriptResponse>,
}

fn phase_of(name: &str) -> Result<ScriptPhase, String> {
    [
        ScriptPhase::BeforeRequest,
        ScriptPhase::AfterResponse,
        ScriptPhase::Tests,
    ]
    .into_iter()
    .find(|p| phase_name(*p) == name)
    .ok_or_else(|| {
        format!("unknown phase \"{name}\" (use before-request, after-response or tests)")
    })
}

/// The origin name of the script a target means.
pub fn origin_of(target: &DebugTarget) -> String {
    match target.folder.as_deref() {
        None => "request".to_owned(),
        Some("") => "collection".to_owned(),
        Some(folder) => format!("folder:{folder}"),
    }
}

/// Builds the input of a debug run for `target`.
pub fn prepare(args: PrepareArgs<'_>, target: &DebugTarget) -> Result<DebugInput, String> {
    let phase = phase_of(&target.phase)?;
    let context = args
        .collection
        .request(&target.request_path)
        .ok_or_else(|| format!("request not found: {}", target.request_path))?;
    let origin = origin_of(target);
    let scripts = scripts_for(&context, phase);
    if !scripts.iter().any(|s| s.origin == origin) {
        return Err(format!(
            "there is no {} script at that level ({origin})",
            target.phase
        ));
    }
    if phase != ScriptPhase::BeforeRequest && args.response.is_none() {
        return Err(format!(
            "send the request once first: a {} script reads the response",
            target.phase
        ));
    }
    let input = PrepareInput {
        environment: target.environment.as_deref(),
        globals: Vec::new(),
        runtime: args.runtime,
    };
    let scope = rt_core::prepare::scope(&context, &input).unwrap_or_default();
    Ok(DebugInput {
        phase,
        scripts,
        request: request_of(&context.request.spec),
        response: if phase == ScriptPhase::BeforeRequest {
            None
        } else {
            args.response
        },
        variables: variables_of(&scope),
        info: ScriptInfo {
            request_path: context.request.path.clone(),
            request_name: context.request.spec.name.clone(),
            collection_name: context.collection.name.clone(),
            environment: target.environment.clone(),
            iteration: None,
        },
        origin,
        breakpoints: target.breakpoints.clone(),
        stop_on_entry: target.stop_on_entry,
    })
}
