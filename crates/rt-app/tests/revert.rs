// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reverting from the journal (wave 19, strand C): the manual revert with its plan, the guard,
//! the production question and the audit entry, and the rules the core applies to what an
//! extension's compensator supplies (order, conflict guard, `If-Match`, limits).

mod journal_support;

use std::sync::Mutex;

use journal_support::{CANARY, Wire, answer, block_on, collection, snapshot};
use rt_app::agents::{AuditEntry, AuditOutcome};
use rt_app::journal::store::Journal;
use rt_app::journal::{
    Compensation, CompensationAnswer, CompensationItem, CompensationKind, JournalEntry,
    JournalSettings, MAX_STEPS, PluginCompensationRule, QueueStorage, RevertEnv, RevertOptions,
    StepResult, StepState, execute, hash_state, plan_compensations, plan_manual, plugin_rules,
    pre_read,
};
use rt_app::plugins::PluginInfo;
use rt_core::guard::{Effect, GuardVerdict, SendCheck, SendGuard};
use rt_core::model::RequestSpec;
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_format::FileSet;

fn files() -> FileSet {
    collection(
        "revert-shop",
        &[
            (
                "remove",
                "DELETE",
                "{{base}}/orders/{{journal.id}}?key={{token}}",
            ),
            ("by-location", "DELETE", "{{journal.location}}"),
            ("no-id", "GET", "{{base}}/orders"),
            ("create", "POST", "{{base}}/orders"),
        ],
    )
}

fn entry(id: u64, environment: &str) -> JournalEntry {
    JournalEntry {
        id,
        session: "s-test".into(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        environment: Some(environment.into()),
        request_path: "create.yml".into(),
        method: "POST".into(),
        url: format!("http://{environment}.test/orders"),
        status: 201,
        location: Some(format!("http://{environment}.test/orders/{id}")),
        etag: Some("\"v1\"".into()),
        resource_id: Some(id.to_string()),
        reverted_at: None,
    }
}

fn journal_with(entries: Vec<JournalEntry>) -> std::sync::Arc<Journal> {
    Journal::open(
        "revert-shop",
        std::sync::Arc::new(QueueStorage::new(entries, JournalSettings::default())),
    )
}

fn get(url: &str) -> RequestSpec {
    RequestSpec {
        name: "probe".into(),
        method: "GET".into(),
        url: url.into(),
        ..Default::default()
    }
}

fn delete(url: &str) -> RequestSpec {
    RequestSpec {
        name: "delete".into(),
        method: "DELETE".into(),
        url: url.into(),
        ..Default::default()
    }
}

fn compensation(url: &str, probe: Option<&str>, state_hash: Option<String>) -> Compensation {
    Compensation {
        kind: CompensationKind::Delete,
        request: delete(url),
        state_hash,
        probe: probe.map(get),
        description: format!("Delete {url}"),
    }
}

struct Run {
    audit: Mutex<Vec<AuditEntry>>,
}

impl Run {
    fn new() -> Self {
        Self {
            audit: Mutex::new(Vec::new()),
        }
    }
    fn exec(
        &self,
        snap: &rt_app::runner::RunSnapshot,
        wire: &Wire,
        guard: Option<&dyn SendGuard>,
        journal: Option<&Journal>,
        plan: &rt_app::journal::RevertPlan,
        confirmed: bool,
    ) -> Result<rt_app::journal::RevertOutcome, rt_app::AppError> {
        let services = Services {
            guard,
            ..Services::none()
        };
        let options = SendOptions::default();
        let env = RevertEnv {
            snapshot: snap,
            transport: wire,
            options: &options,
            services: &services,
            caller: "app",
        };
        block_on(execute(
            &env,
            journal,
            plan,
            &RevertOptions {
                production_confirmed: confirmed,
            },
            &|e| self.audit.lock().unwrap().push(e.clone()),
            &|| false,
        ))
    }
}

#[test]
fn a_manual_plan_shows_the_request_with_masked_url_effect_and_environment() {
    let snap = snapshot(&files(), "dev");
    let plan = plan_manual(&snap, &entry(7, "dev"), "remove.yml").unwrap();
    assert_eq!(plan.environment.as_deref(), Some("dev"));
    assert!(!plan.production);
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert_eq!(step.state, StepState::Planned);
    assert_eq!(step.method, "DELETE");
    assert_eq!(step.effect, Effect::Write);
    // The id is filled in, the secret in the URL is masked, nothing was sent.
    assert_eq!(step.url, "http://dev.test/orders/7?key=***");
    let json = serde_json::to_string(&plan).unwrap();
    assert!(!json.contains(CANARY), "a secret value is in the plan");
}

#[test]
fn a_manual_plan_can_use_the_location_and_refuses_a_hole() {
    let snap = snapshot(&files(), "dev");
    let plan = plan_manual(&snap, &entry(7, "dev"), "by-location.yml").unwrap();
    assert_eq!(plan.steps[0].url, "http://dev.test/orders/7");
    let mut bare = entry(7, "dev");
    bare.resource_id = None;
    let err = plan_manual(&snap, &bare, "remove.yml")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("{{journal.id}}") && err.contains("#7"),
        "{err}"
    );
    assert!(plan_manual(&snap, &entry(7, "dev"), "missing.yml").is_err());
    // A request that does not use the values is fine as it is.
    assert!(plan_manual(&snap, &bare, "no-id.yml").is_ok());
}

#[test]
fn executing_sends_the_step_marks_the_entry_and_writes_an_audit_entry() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(7, "dev")]);
    let plan = plan_manual(&snap, &journal.entry(7).unwrap(), "remove.yml").unwrap();
    let wire = Wire::new(|_| answer(204, &[], ""));
    let run = Run::new();
    let outcome = run
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(outcome.steps.len(), 1);
    assert_eq!(outcome.steps[0].result, StepResult::Reverted);
    // Exactly the request of the plan went out, with the real secret on the wire.
    assert_eq!(
        wire.requests(),
        [format!("DELETE http://dev.test/orders/7?key={CANARY}")]
    );
    assert!(journal.entry(7).unwrap().reverted_at.is_some());
    let audit = run.audit.lock().unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].tool, "journal-revert");
    assert_eq!(audit[0].outcome, AuditOutcome::Sent);
    assert_eq!(audit[0].status, Some(204));
    assert_eq!(
        audit[0].url.as_deref(),
        Some("http://dev.test/orders/7?key=***")
    );
    let all = serde_json::to_string(&(&outcome, &*audit)).unwrap();
    assert!(
        !all.contains(CANARY),
        "a secret value reached the outcome or the audit log"
    );
}

struct Refuse;

impl SendGuard for Refuse {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Refuse {
            rule: "no-deletes".into(),
            reason: "deleting is off".into(),
        }
    }
}

#[test]
fn the_send_guard_decides_each_step_and_a_refusal_is_audited() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(7, "dev")]);
    let plan = plan_manual(&snap, &journal.entry(7).unwrap(), "remove.yml").unwrap();
    let wire = Wire::new(|_| answer(204, &[], ""));
    let run = Run::new();
    let outcome = run
        .exec(&snap, &wire, Some(&Refuse), Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(outcome.steps[0].result, StepResult::Failed);
    assert!(outcome.steps[0].message.contains("no-deletes"));
    assert!(
        wire.requests().is_empty(),
        "nothing goes out when the guard refuses"
    );
    assert!(journal.entry(7).unwrap().reverted_at.is_none());
    let audit = run.audit.lock().unwrap();
    assert_eq!(audit[0].outcome, AuditOutcome::Refused);
    assert_eq!(audit[0].rule.as_deref(), Some("no-deletes"));
}

#[test]
fn production_needs_the_users_explicit_confirmation() {
    let snap = snapshot(&files(), "prod");
    let journal = journal_with(vec![entry(7, "prod")]);
    let plan = plan_manual(&snap, &journal.entry(7).unwrap(), "remove.yml").unwrap();
    assert!(plan.production);
    let wire = Wire::new(|_| answer(204, &[], ""));
    let run = Run::new();
    let err = run
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap_err();
    assert!(err.to_string().contains("PRODUCTION"), "{err}");
    assert!(wire.requests().is_empty());
    // A plan that claims it is not production does not fool the core.
    let mut lying = plan.clone();
    lying.production = false;
    assert!(
        run.exec(&snap, &wire, None, Some(&journal), &lying, false)
            .is_err()
    );
    let outcome = run
        .exec(&snap, &wire, None, Some(&journal), &plan, true)
        .unwrap();
    assert_eq!(outcome.steps[0].result, StepResult::Reverted);
    assert_eq!(
        run.audit.lock().unwrap()[0].approved_by.as_deref(),
        Some("app")
    );
}

#[test]
fn a_plan_for_another_environment_is_refused() {
    let snap = snapshot(&files(), "dev");
    let plan = plan_manual(&snapshot(&files(), "prod"), &entry(7, "prod"), "remove.yml").unwrap();
    let wire = Wire::new(|_| answer(204, &[], ""));
    assert!(
        Run::new()
            .exec(&snap, &wire, None, None, &plan, true)
            .is_err()
    );
    assert!(wire.requests().is_empty());
}

#[test]
fn compensation_plans_go_newest_first_and_give_reasons() {
    let snap = snapshot(&files(), "dev");
    let mut done = entry(4, "dev");
    done.reverted_at = Some(1);
    let journal = journal_with(vec![
        entry(1, "dev"),
        entry(2, "dev"),
        entry(3, "dev"),
        done,
    ]);
    let items = vec![
        CompensationItem {
            entry_id: 1,
            answer: CompensationAnswer::Compensation {
                compensation: Box::new(compensation("{{base}}/orders/1", None, None)),
            },
        },
        CompensationItem {
            entry_id: 2,
            answer: CompensationAnswer::NotReversible {
                reason: "RPC call".into(),
            },
        },
        CompensationItem {
            entry_id: 3,
            answer: CompensationAnswer::Compensation {
                compensation: Box::new(compensation("{{base}}/orders/3?key={{token}}", None, None)),
            },
        },
        CompensationItem {
            entry_id: 4,
            answer: CompensationAnswer::Compensation {
                compensation: Box::new(compensation("{{base}}/orders/4", None, None)),
            },
        },
    ];
    let plan = plan_compensations(&snap, &journal, &items).unwrap();
    let ids: Vec<_> = plan.steps.iter().map(|s| s.entry_id).collect();
    assert_eq!(ids, [4, 3, 2, 1], "reverse order of writing");
    assert_eq!(plan.steps[0].reason.as_deref(), Some("already reverted"));
    assert_eq!(plan.steps[2].reason.as_deref(), Some("RPC call"));
    assert_eq!(plan.steps[1].url, "http://dev.test/orders/3?key=***");
    let wire = Wire::new(|_| answer(204, &[], ""));
    let outcome = Run::new()
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap();
    let results: Vec<_> = outcome.steps.iter().map(|s| s.result).collect();
    assert_eq!(
        results,
        [
            StepResult::SkippedNotReversible,
            StepResult::Reverted,
            StepResult::SkippedNotReversible,
            StepResult::Reverted
        ]
    );
    let sent = wire.requests();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains("/orders/3") && sent[1].ends_with("/orders/1"));
}

#[test]
fn a_compensation_that_would_write_a_masked_secret_back_is_not_reversible() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(1, "dev")]);
    let mut comp = compensation("{{base}}/orders/1", None, None);
    comp.kind = CompensationKind::Restore;
    comp.request.method = "PATCH".into();
    comp.request.body = rt_core::model::Body::Raw {
        kind: rt_core::model::RawKind::Json,
        data: "{\"password\":\"***\"}".into(),
    };
    let items = [CompensationItem {
        entry_id: 1,
        answer: CompensationAnswer::Compensation {
            compensation: Box::new(comp),
        },
    }];
    let plan = plan_compensations(&snap, &journal, &items).unwrap();
    assert_eq!(plan.steps[0].state, StepState::NotReversible);
    assert!(plan.steps[0].reason.as_deref().unwrap().contains("secret"));
}

#[test]
fn the_plan_is_limited_and_environments_are_not_mixed() {
    let snap = snapshot(&files(), "dev");
    let many: Vec<_> = (1..=(MAX_STEPS as u64 + 5))
        .map(|i| entry(i, "dev"))
        .collect();
    let journal = journal_with(many);
    let items: Vec<_> = (1..=(MAX_STEPS as u64 + 5))
        .map(|i| CompensationItem {
            entry_id: i,
            answer: CompensationAnswer::NotReversible { reason: "x".into() },
        })
        .collect();
    let plan = plan_compensations(&snap, &journal, &items).unwrap();
    assert_eq!(plan.steps.len(), MAX_STEPS);
    assert!(plan.truncated);
    assert_eq!(plan.steps[0].entry_id, MAX_STEPS as u64 + 5);
    let mixed = journal_with(vec![entry(1, "dev"), entry(2, "prod")]);
    let items = [1, 2].map(|i| CompensationItem {
        entry_id: i,
        answer: CompensationAnswer::NotReversible { reason: "x".into() },
    });
    assert!(plan_compensations(&snap, &mixed, &items).is_err());
    let unknown = [CompensationItem {
        entry_id: 99,
        answer: CompensationAnswer::NotReversible { reason: "x".into() },
    }];
    assert!(plan_compensations(&snap, &mixed, &unknown).is_err());
}

fn guarded_plan(
    snap: &rt_app::runner::RunSnapshot,
    journal: &Journal,
    expected: &str,
) -> rt_app::journal::RevertPlan {
    let hash = hash_state(expected);
    let items = [CompensationItem {
        entry_id: 1,
        answer: CompensationAnswer::Compensation {
            compensation: Box::new(compensation(
                "{{base}}/orders/1",
                Some("{{base}}/orders/1"),
                Some(hash),
            )),
        },
    }];
    plan_compensations(snap, journal, &items).unwrap()
}

#[test]
fn a_resource_changed_by_someone_else_is_skipped_never_overwritten() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(1, "dev")]);
    // What the server held right after our write.
    let plan = guarded_plan(&snap, &journal, "{\"id\":1,\"name\":\"ours\"}");
    // Somebody else changed it since.
    let wire = Wire::new(|_| answer(200, &[("ETag", "\"v9\"")], "{\"id\":1,\"name\":\"theirs\"}"));
    let outcome = Run::new()
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(outcome.steps[0].result, StepResult::SkippedConflict);
    assert_eq!(
        wire.requests().len(),
        1,
        "only the read went out, no DELETE"
    );
    assert!(wire.requests()[0].starts_with("GET "));
    assert!(journal.entry(1).unwrap().reverted_at.is_none());
}

#[test]
fn an_unchanged_resource_is_put_back_with_if_match_and_checked_afterwards() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(1, "dev")]);
    let plan = guarded_plan(&snap, &journal, "{\"name\":\"ours\",\"id\":1}");
    // The key order differs from the hashed text on purpose: the hash is of the JSON.
    let deleted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = deleted.clone();
    let wire = Wire::new(move |r| {
        if r.method == "DELETE" {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            return answer(204, &[], "");
        }
        if flag.load(std::sync::atomic::Ordering::SeqCst) {
            answer(404, &[], "")
        } else {
            answer(200, &[("ETag", "\"v2\"")], "{\"id\":1,\"name\":\"ours\"}")
        }
    });
    let outcome = Run::new()
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(outcome.steps[0].result, StepResult::Reverted, "{outcome:?}");
    let sent = wire.requests();
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(
        sent[0].starts_with("GET ")
            && sent[1].starts_with("DELETE ")
            && sent[2].starts_with("GET ")
    );
    let headers = wire.headers_of(1);
    assert!(
        headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("if-match") && v == "\"v2\""),
        "{headers:?}"
    );
    assert!(journal.entry(1).unwrap().reverted_at.is_some());
}

#[test]
fn a_failed_check_afterwards_is_reported_and_5xx_is_retried_a_few_times() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(1, "dev")]);
    let plan = guarded_plan(&snap, &journal, "{\"id\":1}");
    // The DELETE answers 503 twice, then 204; but the resource is still there afterwards.
    let tries = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter = tries.clone();
    let wire = Wire::new(move |r| {
        if r.method == "DELETE" {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return if n < 2 {
                answer(503, &[], "")
            } else {
                answer(204, &[], "")
            };
        }
        answer(200, &[], "{\"id\":1}")
    });
    let outcome = Run::new()
        .exec(&snap, &wire, None, Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(outcome.steps[0].result, StepResult::Failed);
    assert!(
        outcome.steps[0].message.contains("check afterwards"),
        "{outcome:?}"
    );
    assert!(journal.entry(1).unwrap().reverted_at.is_none());
    // A server that never recovers stops after the limit.
    let always = Wire::new(|r| {
        if r.method == "DELETE" {
            answer(500, &[], "")
        } else {
            answer(200, &[], "{\"id\":1}")
        }
    });
    let outcome = Run::new()
        .exec(&snap, &always, None, Some(&journal), &plan, false)
        .unwrap();
    assert_eq!(outcome.steps[0].result, StepResult::Failed);
    let deletes = always
        .requests()
        .iter()
        .filter(|r| r.starts_with("DELETE"))
        .count();
    assert_eq!(deletes, 6, "one try and five retries");
}

#[test]
fn a_probe_that_writes_is_not_accepted() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(vec![entry(1, "dev")]);
    let mut comp = compensation("{{base}}/orders/1", None, None);
    comp.probe = Some(delete("{{base}}/orders/1"));
    let items = [CompensationItem {
        entry_id: 1,
        answer: CompensationAnswer::Compensation {
            compensation: Box::new(comp),
        },
    }];
    let plan = plan_compensations(&snap, &journal, &items).unwrap();
    assert_eq!(plan.steps[0].state, StepState::NotReversible);
}

#[test]
fn a_read_before_a_write_is_kept_in_memory_for_the_next_entry_with_that_url() {
    let snap = snapshot(&files(), "dev");
    let journal = journal_with(Vec::new());
    journal
        .set_settings(JournalSettings {
            enabled: true,
            id_path: None,
        })
        .unwrap();
    let wire = Wire::new(|_| answer(200, &[("ETag", "\"v1\"")], "{\"id\":1,\"name\":\"before\"}"));
    let services = Services::none();
    let options = SendOptions::default();
    let env = RevertEnv {
        snapshot: &snap,
        transport: &wire,
        options: &options,
        services: &services,
        caller: "app",
    };
    let state = block_on(pre_read(
        &env,
        &journal,
        &get("{{base}}/orders/1?key={{token}}"),
    ))
    .unwrap();
    assert_eq!(state.status, 200);
    assert_eq!(state.etag.as_deref(), Some("\"v1\""));
    // Only reading requests may be used for it.
    assert!(block_on(pre_read(&env, &journal, &delete("{{base}}/orders/1"))).is_err());
}

#[test]
fn plugin_rules_come_from_enabled_plugins_only() {
    use rt_plugin::api::{
        CompensationContribution, CompensationMatch, CompensationRule, Contributions, Manifest,
    };
    let rule = CompensationContribution {
        id: "post-delete".into(),
        matches: CompensationMatch::default(),
        rule: CompensationRule::default(),
    };
    let info = |id: &str, enabled: bool| {
        let mut manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "version": "1.0.0", "api": 1, "license": "Apache-2.0"
        }))
        .unwrap();
        manifest.contributes = Contributions {
            compensations: vec![rule.clone()],
            ..Default::default()
        };
        PluginInfo {
            manifest,
            granted: true,
            enabled,
        }
    };
    let rules = plugin_rules(&[info("on", true), info("off", false)]);
    assert_eq!(
        rules,
        [PluginCompensationRule {
            plugin: "on".into(),
            rule
        }]
    );
}
