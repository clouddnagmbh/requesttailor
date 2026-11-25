// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The search: delta debugging (ddmin) over the parts of a request, a 1-minimal check, and the
//! optional pass over values. Every probe is one real send through `send_with` (so the send
//! guard, the scripts, the cookies and the plugins are the ones of a normal send), and the
//! number of probes, the time and the number of parts are limited.

use std::collections::HashMap;

use rt_core::guard::{Effect, effect};
use rt_core::model::{Item, RequestSpec};
use rt_core::send::{SendError, SendOptions, SendReport};
use rt_core::services::Services;
use rt_core::transport::Transport;
use web_time::Instant;

use super::parts::{Plan, ValueSlot, value_slots, with_value};
use super::predicate::{failure_of, holds, validate};
use super::{
    DEFAULT_PROBES, DEFAULT_SECONDS, MAX_PARTS, MAX_PROBES, MAX_SECONDS, Predicate, ShrinkRequest,
    ShrinkResult, ShrinkStep,
};
use crate::AppError;
use crate::agents::is_production;
use crate::compare::run_snapshot;
use crate::runner::RunSnapshot;

/// Why a probe could not be sent.
enum Halt {
    /// The reason for `ShrinkResult.stopped_by`.
    Stop(String),
}

fn slot_mut<'a>(items: &'a mut [Item], path: &str) -> Option<&'a mut RequestSpec> {
    for item in items {
        match item {
            Item::Request(r) if r.path == path => return Some(&mut r.spec),
            Item::Folder(f) => {
                if let Some(found) = slot_mut(&mut f.items, path) {
                    return Some(found);
                }
            }
            Item::Request(_) => {}
        }
    }
    None
}

struct Prober<'a, T> {
    work: RunSnapshot,
    path: String,
    transport: &'a T,
    options: &'a SendOptions,
    services: &'a Services<'a>,
    cancelled: &'a dyn Fn() -> bool,
    started: Instant,
    max_probes: u32,
    max_seconds: u32,
    probes: u32,
    /// All of these must hold for a hit; set by the first probe.
    matcher: Vec<Predicate>,
    cache: HashMap<Vec<bool>, bool>,
}

impl<T: Transport> Prober<'_, T> {
    /// Sends `spec` once. A `Halt` ends the search (limit, guard, cancel).
    async fn send(&mut self, spec: &RequestSpec) -> Result<SendReport, Halt> {
        if (self.cancelled)() {
            return Err(Halt::Stop("cancelled".into()));
        }
        if self.probes >= self.max_probes {
            return Err(Halt::Stop("probes".into()));
        }
        if self.started.elapsed().as_secs() >= u64::from(self.max_seconds) {
            return Err(Halt::Stop("time".into()));
        }
        let slot = slot_mut(&mut self.work.collection.items, &self.path)
            .ok_or_else(|| Halt::Stop("error".into()))?;
        *slot = spec.clone();
        self.probes += 1;
        let paths = [self.path.clone()];
        let outcome = run_snapshot(
            &self.work,
            None,
            Some(&paths),
            self.transport,
            self.options,
            self.services,
            &mut |_| {},
            self.cancelled,
        )
        .await;
        let report = outcome
            .steps
            .into_iter()
            .next()
            .map(|s| s.report)
            .ok_or_else(|| Halt::Stop("error".into()))?;
        if let Some(SendError::Blocked { rule, .. }) = &report.error {
            return Err(Halt::Stop(format!("blocked: {rule}")));
        }
        Ok(report)
    }

    fn hit(&self, report: &SendReport) -> bool {
        !self.matcher.is_empty() && self.matcher.iter().all(|p| holds(p, report))
    }

    /// One probe: did the failure show? A rate limit that is not what we look for ends the
    /// search (the search never hammers a server that asks for a pause).
    async fn probe(&mut self, spec: &RequestSpec) -> Result<bool, Halt> {
        let report = self.send(spec).await?;
        let hit = self.hit(&report);
        if !hit && report.response.as_ref().is_some_and(|r| r.status == 429) {
            return Err(Halt::Stop("rate-limited".into()));
        }
        Ok(hit)
    }
}

/// The chunks of `items` for ddmin granularity `n`.
fn split(items: &[usize], n: usize) -> Vec<Vec<usize>> {
    let size = items.len().div_ceil(n);
    items.chunks(size.max(1)).map(<[usize]>::to_vec).collect()
}

struct Search<'p> {
    plan: &'p Plan,
}

impl Search<'_> {
    fn eff(&self, set: &[usize]) -> Vec<bool> {
        let mut kept = vec![false; self.plan.parts.len()];
        for &i in set {
            kept[i] = true;
        }
        self.plan.effective(&kept)
    }

    async fn test<T: Transport>(
        &self,
        prober: &mut Prober<'_, T>,
        set: &[usize],
    ) -> Result<bool, Halt> {
        let eff = self.eff(set);
        if let Some(&known) = prober.cache.get(&eff) {
            return Ok(known);
        }
        let spec = self.plan.build(&eff);
        let hit = prober.probe(&spec).await?;
        prober.cache.insert(eff, hit);
        Ok(hit)
    }

    /// ddmin on `cur` (a set that hits); keeps `cur` the smallest hit found so far, so a halt
    /// leaves a usable answer in it.
    async fn ddmin<T: Transport>(
        &self,
        prober: &mut Prober<'_, T>,
        cur: &mut Vec<usize>,
    ) -> Result<(), Halt> {
        if !cur.is_empty() && self.test(prober, &[]).await? {
            cur.clear();
            return Ok(());
        }
        let mut n = 2;
        while cur.len() >= 2 {
            let chunks = split(cur, n);
            let mut reduced = false;
            // Chunks alone are only tried while there are two of them: when a failure needs
            // several scattered parts, a chunk alone almost never fails and every such probe
            // is wasted. Taking one chunk away is what narrows the search then.
            if n == 2 {
                for chunk in &chunks {
                    if self.test(prober, chunk).await? {
                        *cur = chunk.clone();
                        reduced = true;
                        break;
                    }
                }
            } else {
                for chunk in &chunks {
                    let rest: Vec<usize> =
                        cur.iter().copied().filter(|i| !chunk.contains(i)).collect();
                    if self.test(prober, &rest).await? {
                        *cur = rest;
                        n = (n - 1).max(2);
                        reduced = true;
                        break;
                    }
                }
            }
            if !reduced {
                if n >= cur.len() {
                    break;
                }
                n = (n * 2).min(cur.len());
            }
        }
        Ok(())
    }

    /// Every part left, taken away alone, must make the failure go; one that does not is
    /// removed and the check goes on.
    async fn one_minimal<T: Transport>(
        &self,
        prober: &mut Prober<'_, T>,
        cur: &mut Vec<usize>,
    ) -> Result<(), Halt> {
        let mut i = 0;
        while i < cur.len() {
            let mut without = cur.clone();
            without.remove(i);
            if self.test(prober, &without).await? {
                *cur = without;
            } else {
                i += 1;
            }
        }
        Ok(())
    }
}

fn removed_steps(plan: &Plan, eff: &[bool]) -> Vec<ShrinkStep> {
    plan.parts
        .iter()
        .enumerate()
        .filter(|(i, p)| !eff[*i] && p.parent.is_none_or(|parent| eff[parent]))
        .map(|(_, p)| ShrinkStep {
            part: p.id.clone(),
            kind: p.kind.to_owned(),
        })
        .collect()
}

fn halted(
    request: RequestSpec,
    removed: Vec<ShrinkStep>,
    probes: u32,
    reason: String,
) -> ShrinkResult {
    ShrinkResult {
        request,
        removed,
        probes,
        not_reproduced: false,
        flaky: false,
        one_minimal: false,
        stopped_by: Some(reason),
    }
}

/// Shrinks the request `request.path` of `snapshot` (the collection as edited in memory, with
/// the secret values of the environment) to the smallest one that still fails.
///
/// Rules: a writing request needs `confirmed_writes` and is never shrunk against a production
/// environment; reading requests may be. The send guard answers for every probe through the
/// services; a refusal ends the search with `stopped_by` `blocked: <rule>`. Only parts the
/// request has are removed. The result never holds a value of a secret: it is the request as
/// written (templates), and `removed` names parts, not values.
pub async fn shrink<T: Transport>(
    snapshot: &RunSnapshot,
    request: &ShrinkRequest,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<ShrinkResult, AppError> {
    validate(&request.predicate).map_err(AppError::Shell)?;
    let original = snapshot
        .collection
        .request(&request.path)
        .ok_or_else(|| AppError::RequestNotFound(request.path.clone()))?
        .request
        .spec
        .clone();
    let max_probes = match request.max_probes {
        0 => DEFAULT_PROBES,
        n => n.min(MAX_PROBES),
    };
    let max_seconds = match request.max_seconds {
        0 => DEFAULT_SECONDS,
        n => n.min(MAX_SECONDS),
    };
    if effect(&original) == Effect::Write {
        let policy = &snapshot.collection.config.agents;
        if is_production(policy, snapshot.environment.as_deref()) {
            return Err(AppError::Shell(
                "A writing request is never shrunk against a production environment.".into(),
            ));
        }
        if !request.confirmed_writes {
            return Err(AppError::Shell(format!(
                "This sends up to {max_probes} requests that write. Confirm to continue."
            )));
        }
    }
    let plan = Plan::new(&original, &request.keep, MAX_PARTS);
    let search = Search { plan: &plan };
    let mut prober = Prober {
        work: snapshot.clone(),
        path: request.path.clone(),
        transport,
        options,
        services,
        cancelled,
        started: Instant::now(),
        max_probes,
        max_seconds,
        probes: 0,
        matcher: Vec::new(),
        cache: HashMap::new(),
    };
    let untouched = |probes: u32| ShrinkResult {
        request: original.clone(),
        removed: Vec::new(),
        probes,
        not_reproduced: false,
        flaky: false,
        one_minimal: false,
        stopped_by: None,
    };

    // The failure must show with the request as it is, twice.
    let first = match prober.send(&original).await {
        Ok(report) => report,
        Err(Halt::Stop(reason)) => {
            return Ok(halted(original.clone(), Vec::new(), prober.probes, reason));
        }
    };
    prober.matcher = match &request.predicate {
        Predicate::LastFailure => failure_of(&first),
        other => vec![other.clone()],
    };
    if !prober.hit(&first) {
        return Ok(ShrinkResult {
            not_reproduced: true,
            ..untouched(prober.probes)
        });
    }
    match prober.probe(&original).await {
        Ok(true) => {}
        Ok(false) => {
            return Ok(ShrinkResult {
                flaky: true,
                ..untouched(prober.probes)
            });
        }
        Err(Halt::Stop(reason)) => {
            return Ok(halted(original.clone(), Vec::new(), prober.probes, reason));
        }
    }

    let all: Vec<usize> = (0..plan.parts.len())
        .filter(|i| !plan.parts[*i].protected)
        .collect();
    let mut cur = all.clone();
    let mut stopped = None;
    let mut minimal = false;
    match search.ddmin(&mut prober, &mut cur).await {
        Ok(()) => match search.one_minimal(&mut prober, &mut cur).await {
            Ok(()) => minimal = true,
            Err(Halt::Stop(reason)) => stopped = Some(reason),
        },
        Err(Halt::Stop(reason)) => stopped = Some(reason),
    }

    let eff = search.eff(&cur);
    let mut current = plan.build(&eff);
    let mut removed = removed_steps(&plan, &eff);
    let mut flaky = false;

    // The answer is confirmed once more, so a failure that comes and goes is told.
    if stopped.is_none() && !cur.is_empty() && cur.len() != all.len() {
        match prober.probe(&current).await {
            Ok(true) => {}
            Ok(false) => flaky = true,
            Err(Halt::Stop(reason)) => stopped = Some(reason),
        }
    }

    // The second pass: shorter values.
    if request.simplify_values && stopped.is_none() && !flaky {
        let slots: Vec<ValueSlot> = value_slots(&current, &request.keep);
        'slots: for slot in &slots {
            for alternative in &slot.alternatives {
                let Some(candidate) = with_value(&current, slot, alternative) else {
                    continue;
                };
                match prober.probe(&candidate).await {
                    Ok(true) => {
                        current = candidate;
                        removed.push(ShrinkStep {
                            part: slot.id.clone(),
                            kind: "value".into(),
                        });
                        break;
                    }
                    Ok(false) => {}
                    Err(Halt::Stop(reason)) => {
                        stopped = Some(reason);
                        break 'slots;
                    }
                }
            }
        }
    }

    Ok(ShrinkResult {
        request: current,
        removed,
        probes: prober.probes,
        not_reproduced: false,
        flaky,
        one_minimal: minimal && !flaky && stopped.is_none(),
        stopped_by: stopped,
    })
}
