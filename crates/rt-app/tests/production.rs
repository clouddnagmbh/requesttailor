// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand D: one source for "is this environment production" (`is_production`).
//! The policy flag decides when the collection says anything about the environment; the name
//! heuristic is the fallback for environments the policy does not list.

use rt_app::agents::{is_production, looks_like_production};
use rt_core::model::{AgentAccess, AgentPolicy, EnvironmentPolicy};

fn policy(entries: &[(&str, bool)]) -> AgentPolicy {
    AgentPolicy {
        access: AgentAccess::Read,
        environments: entries
            .iter()
            .map(|(name, production)| EnvironmentPolicy {
                environment: (*name).to_owned(),
                access: AgentAccess::Write,
                production: *production,
            })
            .collect(),
    }
}

#[test]
fn the_flag_marks_production_whatever_the_name() {
    let p = policy(&[("Customer-A", true)]);
    assert!(is_production(&p, Some("Customer-A")));
    assert!(!is_production(&p, Some("Customer-B")));
}

#[test]
fn an_unlisted_environment_falls_back_to_the_name() {
    let p = AgentPolicy::default();
    for name in ["PRD", "prod", "Production", "eu-prod-2", "Live", "live-eu"] {
        assert!(is_production(&p, Some(name)), "{name}");
    }
    for name in ["Local", "Staging", "QA", "delivery", "", "alive"] {
        assert!(!is_production(&p, Some(name)), "{name}");
    }
}

#[test]
fn a_listed_environment_without_the_flag_is_not_production_even_if_named_so() {
    let p = policy(&[("prod-sandbox", false)]);
    assert!(!is_production(&p, Some("prod-sandbox")));
}

#[test]
fn no_environment_is_never_production() {
    assert!(!is_production(&policy(&[]), None));
}

#[test]
fn the_name_heuristic_matches_the_ui() {
    // apps/ui/src/environments/ops.ts: /prod|prd|\blive\b/i
    assert!(looks_like_production("PRD"));
    assert!(looks_like_production("my-live"));
    assert!(!looks_like_production("my_live"));
    assert!(!looks_like_production("deliver"));
    assert!(!looks_like_production("lively"));
}
