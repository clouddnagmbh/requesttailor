// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The health score (wave 18, strand F): the weighted mean of the satisfaction per rule.
//! See the module comment of `lint` for the formula.

use super::{HealthScore, LintRule, RuleScore};

/// `(ok, total)` per rule, in `LintRule::ALL` order.
pub(super) type Counts = [(u32, u32); 4];

fn index(rule: LintRule) -> usize {
    LintRule::ALL
        .iter()
        .position(|r| *r == rule)
        .expect("a known rule")
}

pub(super) fn add(counts: &mut Counts, rule: LintRule, ok: bool) {
    let c = &mut counts[index(rule)];
    c.1 += 1;
    if ok {
        c.0 += 1;
    }
}

pub(super) fn health(counts: &Counts) -> HealthScore {
    let rules: Vec<RuleScore> = LintRule::ALL
        .iter()
        .zip(counts)
        .map(|(rule, (ok, total))| RuleScore {
            rule: *rule,
            ok: *ok,
            total: *total,
        })
        .collect();
    let mut weight = 0u64;
    // Work in whole parts of 1/total so that the same counts always give the same number.
    let mut sum = 0f64;
    for r in rules.iter().filter(|r| r.total > 0) {
        let w = u64::from(r.rule.weight());
        weight += w;
        sum += w as f64 * f64::from(r.ok) / f64::from(r.total);
    }
    let score = if weight == 0 {
        100
    } else {
        (100.0 * sum / weight as f64).round().clamp(0.0, 100.0) as u8
    };
    HealthScore { score, rules }
}
