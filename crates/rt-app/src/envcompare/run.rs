// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run of an environment comparison (wave 18, strand A); rules at the module
//! documentation of [`super`].

use std::collections::BTreeSet;

use rt_core::guard::{Effect, effect};
use rt_core::runner::{RunStep, RunSummary};
use rt_core::script::VarChange;
use rt_core::send::{ReportBody, SendOptions};
use rt_core::services::Services;
use rt_core::transport::Transport;

use super::{EnvCompareRequest, EnvComparison, EnvPair, SkippedRequest};
use crate::AppError;
use crate::agents::is_production;
use crate::compare::{compare_steps, mask_comparisons, run_paths, run_snapshot, secret_values};
use crate::runner::RunSnapshot;

/// Why a writing request is not sent.
const SKIPPED_WRITES: &str = "skipped (writes)";

/// Environment values shorter than this are not replaced by `{{name}}` with
/// `ignore_env_values` (a value like `1` or `200` would be everywhere).
const MIN_IGNORED_VALUE: usize = 4;

/// One environment of the comparison: the collection run snapshot with that environment's
/// secret values, and the services of the send. Cookies, tokens and session variables are
/// scoped by the environment name inside `send_with`, so one set of services keeps them apart.
pub struct EnvRun<'a> {
    pub snapshot: RunSnapshot,
    pub services: Services<'a>,
}

fn add(sum: &mut RunSummary, one: &RunSummary) {
    sum.requests += one.requests;
    sum.passed += one.passed;
    sum.failed += one.failed;
    sum.skipped += one.skipped;
    sum.tests_passed += one.tests_passed;
    sum.tests_failed += one.tests_failed;
    sum.assertions_passed += one.assertions_passed;
    sum.assertions_failed += one.assertions_failed;
    sum.duration_ms += one.duration_ms;
    sum.stopped |= one.stopped;
}

/// The non-secret values of the environment's own variables, longest first, with names.
fn own_values(snapshot: &RunSnapshot) -> Vec<(String, String)> {
    let Some(name) = snapshot.environment.as_deref() else {
        return Vec::new();
    };
    let Some(env) = snapshot.collection.environments.iter().find(|e| e.name == name) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut values: Vec<(String, String)> = env
        .variables
        .iter()
        .filter(|v| !v.secret && v.enabled)
        .filter_map(|v| Some((v.name.clone(), v.value.clone()?)))
        .filter(|(_, value)| value.len() >= MIN_IGNORED_VALUE)
        .filter(|(_, value)| seen.insert(value.clone()))
        .collect();
    // The host (and port) of a URL value counts too: a body names `dev.example.com` bare.
    let hosts: Vec<(String, String)> = values
        .iter()
        .filter_map(|(name, value)| {
            let rest = value.split_once("://")?.1;
            let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
            (host.len() >= MIN_IGNORED_VALUE).then(|| (format!("{name}.host"), host.to_owned()))
        })
        .filter(|(_, host)| seen.insert(host.clone()))
        .collect();
    values.extend(hosts);
    values.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    values
}

/// Replaces the environment's own values in the response with `{{name}}`, so what only
/// differs because the environments differ does not show.
fn normalize(step: &mut RunStep, values: &[(String, String)]) {
    let Some(response) = &mut step.report.response else {
        return;
    };
    let swap = |text: &str| {
        let mut text = text.to_owned();
        for (name, value) in values {
            text = text.replace(value.as_str(), &format!("{{{{{name}}}}}"));
        }
        text
    };
    for (_, value) in &mut response.headers {
        *value = swap(value);
    }
    if let ReportBody::Utf8(text) = &mut response.body {
        *text = swap(text);
    }
}

/// Keeps what a step changed in the runtime layer for the next request of the same
/// environment (a login that sets a token), as a run does.
fn carry(snapshot: &mut RunSnapshot, changes: &[VarChange]) {
    use rt_core::model::VarScope;
    for change in changes {
        if change.scope != VarScope::Runtime {
            continue;
        }
        snapshot.vars.retain(|(name, _)| name != &change.name);
        if let Some(value) = &change.value {
            snapshot.vars.push((change.name.clone(), value.clone()));
        }
    }
}

/// Runs the requests of `request` in every environment, one request at a time (first the
/// reference, then the others, so the time between them stays small), and compares each other
/// environment with the reference. Nothing is stored.
///
/// Errors: fewer than two environments, an unknown or repeated environment, and an
/// environment marked as production while `production_confirmed` is not set.
pub async fn compare_environments<T: Transport>(
    runs: &[EnvRun<'_>],
    request: &EnvCompareRequest,
    transport: &T,
    options: &SendOptions,
    cancelled: &dyn Fn() -> bool,
) -> Result<EnvComparison, AppError> {
    let names = &request.environments;
    if names.len() < 2 || runs.len() != names.len() {
        return Err(AppError::Shell(
            "comparing environments needs at least two environments".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for (name, run) in names.iter().zip(runs) {
        if !seen.insert(name) {
            return Err(AppError::Shell(format!(
                "environment {name:?} is listed twice"
            )));
        }
        if !run
            .snapshot
            .collection
            .environments
            .iter()
            .any(|e| &e.name == name)
        {
            return Err(AppError::EnvironmentNotFound(name.clone()));
        }
    }
    let policy = &runs[0].snapshot.collection.config.agents;
    if !request.production_confirmed
        && let Some(name) = names.iter().find(|n| is_production(policy, Some(n)))
    {
        return Err(AppError::Shell(format!(
            "{name} is a production environment: sending its reads needs confirmation"
        )));
    }

    let mut snapshots: Vec<RunSnapshot> = runs.iter().map(|r| r.snapshot.clone()).collect();
    let ignored: Vec<Vec<(String, String)>> = if request.ignore_env_values {
        snapshots.iter().map(own_values).collect()
    } else {
        vec![Vec::new(); snapshots.len()]
    };
    let paths = run_paths(
        &snapshots[0],
        request.folder.as_deref().filter(|f| !f.is_empty()),
        request.paths.as_deref(),
    );

    let mut steps: Vec<Vec<RunStep>> = vec![Vec::new(); runs.len()];
    let mut summaries = vec![RunSummary::default(); runs.len()];
    let mut sent = Vec::new();
    let mut skipped = Vec::new();
    for path in &paths {
        if cancelled() {
            summaries.iter_mut().for_each(|s| s.stopped = true);
            break;
        }
        let Some(context) = snapshots[0].collection.request(path) else {
            continue;
        };
        if effect(&context.request.spec) == Effect::Write {
            skipped.push(SkippedRequest {
                path: path.clone(),
                name: context.request.spec.name.clone(),
                reason: SKIPPED_WRITES.into(),
            });
            continue;
        }
        sent.push(path.clone());
        for i in 0..runs.len() {
            let only = [path.clone()];
            let outcome = run_snapshot(
                &snapshots[i],
                None,
                Some(&only),
                transport,
                options,
                &runs[i].services,
                &mut |_| {},
                cancelled,
            )
            .await;
            add(&mut summaries[i], &outcome.summary);
            for mut step in outcome.steps {
                carry(&mut snapshots[i], &step.report.var_changes);
                normalize(&mut step, &ignored[i]);
                steps[i].push(step);
            }
        }
    }

    let name_of = |path: &str| {
        snapshots[0]
            .collection
            .request(path)
            .map(|c| c.request.spec.name.clone())
    };
    let secrets: Vec<String> = {
        let mut all: Vec<String> = snapshots.iter().flat_map(secret_values).collect();
        // Values given for a secret variable at run time (`--var token=…`) are secrets too.
        for snapshot in &snapshots {
            let c = &snapshot.collection;
            let declared: BTreeSet<&str> = c
                .defaults
                .vars
                .iter()
                .chain(c.environments.iter().flat_map(|e| e.variables.iter()))
                .filter(|v| v.secret)
                .map(|v| v.name.as_str())
                .collect();
            all.extend(
                snapshot
                    .vars
                    .iter()
                    .filter(|(name, value)| declared.contains(name.as_str()) && !value.is_empty())
                    .map(|(_, value)| value.clone()),
            );
        }
        all.sort_by_key(|s| std::cmp::Reverse(s.len()));
        all.dedup();
        all
    };
    let mut pairs = Vec::new();
    for i in 1..runs.len() {
        let mut requests = compare_steps(&sent, name_of, name_of, &steps[0], &steps[i]);
        mask_comparisons(&mut requests, &secrets);
        pairs.push(EnvPair {
            environment: names[i].clone(),
            requests,
            reference_summary: summaries[0].clone(),
            summary: summaries[i].clone(),
        });
    }
    Ok(EnvComparison {
        reference: names[0].clone(),
        pairs,
        skipped,
    })
}
