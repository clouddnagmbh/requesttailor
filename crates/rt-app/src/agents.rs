// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Coding agents on a collection (wave 8): what `rt mcp` may do, approvals, audit log.
//!
//! The rules are [`AgentPolicy`](rt_core::model::AgentPolicy) in `opencollection.yml`. What
//! this module adds is machine-local and lives in the data folder (`RT_DATA_DIR`), shared by
//! `rt mcp`, `rt approve` and the desktop app:
//!
//! - `agents/approvals/<id>.json`: a [`PendingApproval`]; `rt mcp` writes it and waits.
//! - `agents/approvals/<id>.decision`: `approved` or `denied`, written by the desktop app
//!   (approval banner) or `rt approve <id>` / `rt deny <id>`. `rt mcp` removes both files
//!   when it has the answer or gives up (timeout).
//! - `agents/audit.jsonl`: one [`AuditEntry`] per tool call, appended, never rewritten.
//!   Masked like every report: no secret values, no bodies, no header values.
//!
//! An MCP client that supports elicitation may also be asked directly; the file-based way
//! always works, so the gate does not depend on the client.
//!
//! # Files in detail (strand C)
//!
//! - Every file is written atomically: to a hidden temporary file in the same folder, then
//!   renamed (the pending approval) or hard-linked (the decision, so the first answer wins
//!   and a second one fails).
//! - Ids are 32 random hex digits ([`new_id`]); [`decide`] accepts nothing else, so an id
//!   can never name a path outside the folder.
//! - The decision file is `approved` or `denied` on the first line, optionally followed by
//!   who decided (`app`, `cli`) on the second ([`decide_as`]).
//! - While `rt mcp` waits it touches the pending file every few seconds ([`heartbeat`]). A
//!   pending file not touched for [`STALE_AFTER_MS`] belongs to a process that died; the
//!   next [`pending`] removes it (and its decision).
//!
//! Owner: wave 8 interface. Implementation: strand C.

use std::io::Write;
use std::path::{Path, PathBuf};

use rt_core::model::{AgentAccess, AgentPolicy};
use serde::{Deserialize, Serialize};

/// Whether a request writes (moved to `rt_core::guard` in wave 10, where every shell's send
/// guard can use it; re-exported here unchanged).
pub use rt_core::guard::{Effect, effect, graphql_has_mutation};

/// What the policy says for one send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum Verdict {
    Allowed,
    /// Writing request in a production environment.
    NeedsApproval,
    /// Refused with a reason the agent sees (e.g. "environment PRD allows only reading").
    Refused {
        reason: String,
    },
}

/// `access: none` → refused; `read` + write effect → refused; `write` + production
/// → needs approval; otherwise allowed.
pub fn verdict(policy: &AgentPolicy, environment: Option<&str>, effect: Effect) -> Verdict {
    let (access, production) = policy.for_environment(environment);
    match (access, effect) {
        (AgentAccess::None, _) => Verdict::Refused {
            reason: match environment {
                Some(name) => format!("environment {name} is not available to agents"),
                None => "agents may not send without an environment in this collection".into(),
            },
        },
        (AgentAccess::Read, Effect::Write) => Verdict::Refused {
            reason: format!(
                "{} allows agents only reading requests (GET, HEAD, OPTIONS, GraphQL queries)",
                match environment {
                    Some(name) => format!("environment {name}"),
                    None => "sending without an environment".to_owned(),
                }
            ),
        },
        (AgentAccess::Write, Effect::Write) if production => Verdict::NeedsApproval,
        _ => Verdict::Allowed,
    }
}

/// What the agent rules say about sending one request (`Backend.agents.check`): its
/// [`Effect`] and the [`Verdict`] of the collection's [`AgentPolicy`] for `environment`.
/// The same two functions `rt mcp` calls, so a UI extension that sends on a model's behalf
/// applies exactly the rules of `rt mcp` (minus the policy pinned at its start).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCheck {
    pub effect: Effect,
    pub verdict: Verdict,
}

/// Evaluates the request at `path` of `collection` as it is in memory (unsaved edits
/// included) against the collection's current policy.
pub fn check(
    collection: &rt_core::model::Collection,
    path: &str,
    environment: Option<&str>,
) -> Result<AgentCheck, crate::AppError> {
    let context = collection
        .request(path)
        .ok_or_else(|| crate::AppError::RequestNotFound(path.to_owned()))?;
    let effect = effect(&context.request.spec);
    Ok(AgentCheck {
        effect,
        verdict: verdict(&collection.config.agents, environment, effect),
    })
}

/// The reason the approval banner shows for a writing request in `environment`.
pub fn approval_reason(environment: Option<&str>) -> String {
    match environment {
        Some(name) => format!("writing request against the production environment {name}"),
        None => "writing request against a production system".to_owned(),
    }
}

/// Environments an agent may see: all but those with access `none`.
pub fn visible(policy: &AgentPolicy, environment: &str) -> bool {
    policy.for_environment(Some(environment)).0 != AgentAccess::None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    /// Milliseconds since the Unix epoch.
    pub at: i64,
    /// MCP client name from `initialize` (e.g. `claude-code`), as the agent says.
    pub agent: String,
    /// Absolute path of the collection root.
    pub collection: String,
    pub collection_name: String,
    pub request_path: String,
    pub environment: Option<String>,
    pub method: String,
    /// Masked.
    pub url: String,
    /// Why approval is needed, for the banner.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditOutcome {
    /// Answered without sending (list tools, read a request).
    Read,
    Sent,
    Refused,
    Denied,
    /// No decision before the timeout; nothing was sent.
    TimedOut,
    Failed,
}

/// One line of `agents/audit.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at: i64,
    pub agent: String,
    /// MCP tool name.
    pub tool: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub outcome: AuditOutcome,
    /// `None` without approval; otherwise who decided (`app`, `cli`, `elicitation`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    /// Set when the shell's send guard refused the call (wave 10): the rule it names
    /// (`rt_core::guard::GuardVerdict::Refuse`). The outcome is then `refused`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Masked; e.g. the refusal reason or the error.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

pub fn approvals_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("approvals")
}

pub fn audit_file(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("audit.jsonl")
}

/// A pending file untouched for this long belongs to a process that is gone.
pub const STALE_AFTER_MS: u64 = 30_000;

/// How often a waiting `rt mcp` should call [`heartbeat`].
pub const HEARTBEAT_MS: u64 = 5_000;

/// Who decided when the decision file does not say.
const UNKNOWN_DECIDER: &str = "user";

/// A new approval id: 32 random hex digits from the OS-seeded hasher keys of the standard
/// library (no extra dependency; ids need to be unique, not secret — the folder is the
/// user's own).
pub fn new_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let part = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", part(), part())
}

/// Ids as [`new_id`] makes them (also accepts shorter hex ids for hand-made files).
fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn invalid_id(id: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("{id:?} is not an approval id"),
    )
}

fn pending_file(data_dir: &Path, id: &str) -> PathBuf {
    approvals_dir(data_dir).join(format!("{id}.json"))
}

fn decision_file(data_dir: &Path, id: &str) -> PathBuf {
    approvals_dir(data_dir).join(format!("{id}.decision"))
}

/// A hidden temporary file next to `target`, unique per call.
fn temp_next_to(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{}.tmp", new_id()))
}

/// Writes `bytes` to `target` atomically (temporary file, then rename).
fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_next_to(target);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Writes `approval` to `agents/approvals/<id>.json` (atomically); [`pending`] lists it
/// from now on.
pub fn request_approval(data_dir: &Path, approval: &PendingApproval) -> std::io::Result<()> {
    if !valid_id(&approval.id) {
        return Err(invalid_id(&approval.id));
    }
    let json = serde_json::to_vec_pretty(approval).map_err(std::io::Error::other)?;
    write_atomic(&pending_file(data_dir, &approval.id), &json)
}

/// Marks the pending approval `id` as still waited for (see [`STALE_AFTER_MS`]).
pub fn heartbeat(data_dir: &Path, id: &str) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new()
        .append(true)
        .open(pending_file(data_dir, id))?;
    file.set_modified(std::time::SystemTime::now())
}

/// The decision for `id` once there is one: the decision and who decided.
pub fn decision(data_dir: &Path, id: &str) -> std::io::Result<Option<(ApprovalDecision, String)>> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    let text = match std::fs::read_to_string(decision_file(data_dir, id)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut lines = text.lines().map(str::trim);
    let decision = match lines.next() {
        Some("approved") => ApprovalDecision::Approved,
        Some("denied") => ApprovalDecision::Denied,
        // Anything else is not a decision; it never approves.
        _ => return Ok(None),
    };
    let by = lines
        .next()
        .filter(|b| !b.is_empty())
        .unwrap_or(UNKNOWN_DECIDER);
    Ok(Some((decision, by.to_owned())))
}

/// Removes the pending approval `id` and its decision (answered, timed out or cancelled).
pub fn withdraw(data_dir: &Path, id: &str) -> std::io::Result<()> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    remove_if_present(&pending_file(data_dir, id))?;
    remove_if_present(&decision_file(data_dir, id))
}

/// Pending approvals, oldest first: those without a decision yet. Files of processes that
/// stopped waiting without cleaning up (not touched for [`STALE_AFTER_MS`]) are removed.
pub fn pending(data_dir: &Path) -> std::io::Result<Vec<PendingApproval>> {
    let dir = approvals_dir(data_dir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let now = std::time::SystemTime::now();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) else {
            continue;
        };
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age.as_millis() > u128::from(STALE_AFTER_MS));
        if stale {
            let _ = withdraw(data_dir, id);
            continue;
        }
        if decision_file(data_dir, id).exists() {
            continue;
        }
        let Some(approval) = std::fs::read(entry.path())
            .ok()
            .and_then(|b| serde_json::from_slice::<PendingApproval>(&b).ok())
            .filter(|a| a.id == id)
        else {
            continue;
        };
        out.push(approval);
    }
    out.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Writes the decision for `id` (as the app). Unknown or already decided ids are an error.
pub fn decide(data_dir: &Path, id: &str, decision: ApprovalDecision) -> std::io::Result<()> {
    decide_as(data_dir, id, decision, "app")
}

/// [`decide`], naming who decided (`app`, `cli`); it ends up in the audit log.
pub fn decide_as(
    data_dir: &Path,
    id: &str,
    decision: ApprovalDecision,
    by: &str,
) -> std::io::Result<()> {
    if !valid_id(id) {
        return Err(invalid_id(id));
    }
    if !pending_file(data_dir, id).is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no pending approval {id}"),
        ));
    }
    let target = decision_file(data_dir, id);
    let word = match decision {
        ApprovalDecision::Approved => "approved",
        ApprovalDecision::Denied => "denied",
    };
    let by: String = by
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let tmp = temp_next_to(&target);
    std::fs::write(&tmp, format!("{word}\n{by}\n"))?;
    // A hard link fails if the decision exists: the first answer wins, atomically.
    let linked = std::fs::hard_link(&tmp, &target);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("approval {id} is already decided"),
        )),
        Err(e) => Err(e),
    }
}

/// Appends one entry as one line. The caller masks; entries carry no bodies or header
/// values by construction.
pub fn append_audit(data_dir: &Path, entry: &AuditEntry) -> std::io::Result<()> {
    let file = audit_file(data_dir);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut line = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    line.push(b'\n');
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    // One write per line: appends of several processes do not interleave.
    out.write_all(&line)
}

/// The last `limit` entries, newest first. Lines that do not parse are skipped.
pub fn read_audit(data_dir: &Path, limit: usize) -> std::io::Result<Vec<AuditEntry>> {
    let text = match std::fs::read_to_string(audit_file(data_dir)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(limit)
        .collect())
}

/// Milliseconds since the Unix epoch, for `at` fields.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::{
        Body, EnvironmentPolicy, MessageVariant, Protocol, RequestSpec, WebSocketSpec,
    };

    fn spec(method: &str) -> RequestSpec {
        RequestSpec {
            method: method.into(),
            ..RequestSpec::default()
        }
    }

    fn graphql(query: &str) -> RequestSpec {
        RequestSpec {
            body: Body::GraphQl {
                query: query.into(),
                variables: String::new(),
            },
            ..spec("POST")
        }
    }

    #[test]
    fn methods_decide_for_plain_http() {
        for m in ["GET", "HEAD", "OPTIONS", "TRACE", "get", " Get "] {
            assert_eq!(effect(&spec(m)), Effect::Read, "{m}");
        }
        for m in ["POST", "PUT", "PATCH", "DELETE", "MERGE", "PROPFIND", ""] {
            assert_eq!(effect(&spec(m)), Effect::Write, "{m}");
        }
    }

    #[test]
    fn graphql_mutations_write_queries_read() {
        assert_eq!(effect(&graphql("{ orders { id } }")), Effect::Read);
        assert_eq!(effect(&graphql("query Q { a }")), Effect::Read);
        assert_eq!(effect(&graphql("subscription S { a }")), Effect::Read);
        assert_eq!(
            effect(&graphql("mutation Create { add(x: 1) { id } }")),
            Effect::Write
        );
        assert_eq!(effect(&graphql("  mutation{a}")), Effect::Write);
        // Several operations: any mutation makes it writing.
        assert_eq!(
            effect(&graphql("query A { a }\nmutation B { b }")),
            Effect::Write
        );
        // The word inside selections, arguments, strings, comments, variables, directives.
        for q in [
            "{ mutation }",
            "query { a(mutation: \"x\") }",
            "query Q($mutation: Int) { a }",
            "# mutation\nquery { a }",
            "query { a(s: \"mutation\") }",
            "query { a(s: \"\"\"\nmutation \\\"\"\" x\n\"\"\") }",
            "query @mutation { a }",
            "query mutationName { a }",
            "fragment F on Mutation { a }",
        ] {
            assert_eq!(effect(&graphql(q)), Effect::Read, "{q}");
        }
        // GraphQL over GET reads the same; other methods write.
        let mut get = graphql("{ a }");
        get.method = "GET".into();
        assert_eq!(effect(&get), Effect::Read);
        let mut put = graphql("{ a }");
        put.method = "PUT".into();
        assert_eq!(effect(&put), Effect::Write);
    }

    #[test]
    fn grpc_always_writes_websocket_only_with_messages() {
        let grpc = RequestSpec {
            protocol: Protocol::Grpc(Box::default()),
            ..spec("GRPC")
        };
        assert_eq!(effect(&grpc), Effect::Write);
        let mut ws = WebSocketSpec::default();
        let listen = RequestSpec {
            protocol: Protocol::WebSocket(Box::new(ws.clone())),
            ..spec("WS")
        };
        assert_eq!(effect(&listen), Effect::Read);
        ws.messages.push(MessageVariant::default());
        let talk = RequestSpec {
            protocol: Protocol::WebSocket(Box::new(ws)),
            ..spec("WS")
        };
        assert_eq!(effect(&talk), Effect::Write);
        // SSE is a GET.
        assert_eq!(effect(&spec("GET")), Effect::Read);
    }

    fn policy() -> AgentPolicy {
        let env = |name: &str, access, production| EnvironmentPolicy {
            environment: name.into(),
            access,
            production,
        };
        AgentPolicy {
            access: AgentAccess::Read,
            environments: vec![
                env("Local", AgentAccess::Write, false),
                env("PRD", AgentAccess::Write, true),
                env("PRD-RO", AgentAccess::Read, true),
                env("Vault", AgentAccess::None, false),
            ],
        }
    }

    #[test]
    fn verdicts_follow_the_policy() {
        let p = policy();
        let refused = |v: Verdict| matches!(v, Verdict::Refused { .. });
        // none: refused whatever the effect.
        assert!(refused(verdict(&p, Some("Vault"), Effect::Read)));
        assert!(refused(verdict(&p, Some("Vault"), Effect::Write)));
        // read: reading allowed, writing refused (also in production).
        assert_eq!(verdict(&p, Some("Staging"), Effect::Read), Verdict::Allowed);
        assert!(refused(verdict(&p, Some("Staging"), Effect::Write)));
        assert_eq!(verdict(&p, Some("PRD-RO"), Effect::Read), Verdict::Allowed);
        assert!(refused(verdict(&p, Some("PRD-RO"), Effect::Write)));
        // write: allowed; in production writing needs approval, reading does not.
        assert_eq!(verdict(&p, Some("Local"), Effect::Write), Verdict::Allowed);
        assert_eq!(verdict(&p, Some("PRD"), Effect::Read), Verdict::Allowed);
        assert_eq!(
            verdict(&p, Some("PRD"), Effect::Write),
            Verdict::NeedsApproval
        );
        // Without an environment the default applies.
        assert_eq!(verdict(&p, None, Effect::Read), Verdict::Allowed);
        assert!(refused(verdict(&p, None, Effect::Write)));
        // The default policy only reads.
        let d = AgentPolicy::default();
        assert_eq!(verdict(&d, Some("X"), Effect::Read), Verdict::Allowed);
        assert!(refused(verdict(&d, Some("X"), Effect::Write)));
        let Verdict::Refused { reason } = verdict(&p, Some("Staging"), Effect::Write) else {
            unreachable!()
        };
        assert!(
            reason.contains("Staging") && reason.contains("reading"),
            "{reason}"
        );
        let Verdict::Refused { reason } = verdict(&p, Some("Vault"), Effect::Read) else {
            unreachable!()
        };
        assert!(reason.contains("Vault"), "{reason}");
        assert!(visible(&p, "PRD") && visible(&p, "Other") && !visible(&p, "Vault"));
    }

    fn collection_with(requests: Vec<(&str, RequestSpec)>) -> rt_core::model::Collection {
        use rt_core::model::{Collection, Item, RequestEntry};
        Collection {
            name: "C".into(),
            items: requests
                .into_iter()
                .map(|(path, spec)| {
                    Item::Request(RequestEntry {
                        path: path.into(),
                        spec,
                    })
                })
                .collect(),
            config: rt_core::model::CollectionConfig {
                agents: policy(),
                ..Default::default()
            },
            ..Collection::default()
        }
    }

    #[test]
    fn check_combines_effect_and_policy() {
        let ws = RequestSpec {
            protocol: Protocol::WebSocket(Box::new(WebSocketSpec {
                messages: vec![MessageVariant::default()],
                ..WebSocketSpec::default()
            })),
            ..spec("WS")
        };
        let c = collection_with(vec![
            ("get.yml", spec("GET")),
            ("post.yml", spec("POST")),
            ("query.yml", graphql("{ a }")),
            ("mutation.yml", graphql("mutation { a }")),
            (
                "grpc.yml",
                RequestSpec {
                    protocol: Protocol::Grpc(Box::default()),
                    ..spec("GRPC")
                },
            ),
            ("ws.yml", ws),
        ]);
        let got = |path: &str, env: Option<&str>| check(&c, path, env).unwrap();
        let read = |v| AgentCheck {
            effect: Effect::Read,
            verdict: v,
        };
        let write = |v| AgentCheck {
            effect: Effect::Write,
            verdict: v,
        };
        assert_eq!(got("get.yml", Some("PRD")), read(Verdict::Allowed));
        assert_eq!(got("query.yml", Some("PRD")), read(Verdict::Allowed));
        assert_eq!(got("post.yml", Some("Local")), write(Verdict::Allowed));
        assert_eq!(got("post.yml", Some("PRD")), write(Verdict::NeedsApproval));
        assert_eq!(
            got("mutation.yml", Some("PRD")),
            write(Verdict::NeedsApproval)
        );
        assert_eq!(got("grpc.yml", Some("Local")), write(Verdict::Allowed));
        assert_eq!(got("ws.yml", Some("PRD")), write(Verdict::NeedsApproval));
        assert!(matches!(
            got("post.yml", Some("PRD-RO")).verdict,
            Verdict::Refused { .. }
        ));
        assert!(matches!(
            got("get.yml", Some("Vault")).verdict,
            Verdict::Refused { .. }
        ));
        // No environment: the collection default (read).
        assert_eq!(got("get.yml", None), read(Verdict::Allowed));
        assert!(matches!(
            got("mutation.yml", None).verdict,
            Verdict::Refused { .. }
        ));
        assert!(matches!(
            check(&c, "missing.yml", None),
            Err(crate::AppError::RequestNotFound(_))
        ));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rt-app-agents-{name}-{}-{}",
            std::process::id(),
            new_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn approval(id: &str, at: i64) -> PendingApproval {
        PendingApproval {
            id: id.into(),
            at,
            agent: "claude-code".into(),
            collection: "/c".into(),
            collection_name: "Orders".into(),
            request_path: "orders/create.yml".into(),
            environment: Some("PRD".into()),
            method: "POST".into(),
            url: "https://prd.example.com/orders".into(),
            reason: approval_reason(Some("PRD")),
        }
    }

    #[test]
    fn ids_are_random_hex() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(valid_id(&a));
        for bad in [
            "",
            "../../etc",
            "abc",
            "zzzzzzzzzz",
            "a/b0000000",
            "deadbeef.json",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn approvals_round_trip_through_the_folder() {
        let dir = temp_dir("approvals");
        assert!(pending(&dir).unwrap().is_empty());
        let (a, b) = (new_id(), new_id());
        request_approval(&dir, &approval(&b, 20)).unwrap();
        request_approval(&dir, &approval(&a, 10)).unwrap();
        // Oldest first; no temporary files left behind.
        let list = pending(&dir).unwrap();
        assert_eq!(
            list.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            [&a, &b]
        );
        assert_eq!(list[0], approval(&a, 10));
        let names: Vec<String> = std::fs::read_dir(approvals_dir(&dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");

        assert_eq!(decision(&dir, &a).unwrap(), None);
        decide_as(&dir, &a, ApprovalDecision::Approved, "cli").unwrap();
        assert_eq!(
            decision(&dir, &a).unwrap(),
            Some((ApprovalDecision::Approved, "cli".into()))
        );
        // Decided: no longer pending; a second answer fails, the first stays.
        assert_eq!(pending(&dir).unwrap().len(), 1);
        let again = decide(&dir, &a, ApprovalDecision::Denied).unwrap_err();
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            decision(&dir, &a).unwrap().unwrap().0,
            ApprovalDecision::Approved
        );
        decide(&dir, &b, ApprovalDecision::Denied).unwrap();
        assert_eq!(
            decision(&dir, &b).unwrap(),
            Some((ApprovalDecision::Denied, "app".into()))
        );

        // Unknown and malformed ids.
        let unknown = decide(&dir, &new_id(), ApprovalDecision::Approved).unwrap_err();
        assert_eq!(unknown.kind(), std::io::ErrorKind::NotFound);
        let bad = decide(&dir, "../../x", ApprovalDecision::Approved).unwrap_err();
        assert_eq!(bad.kind(), std::io::ErrorKind::InvalidInput);

        withdraw(&dir, &a).unwrap();
        withdraw(&dir, &a).unwrap();
        assert!(!pending_file(&dir, &a).exists() && !decision_file(&dir, &a).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_decision_file_with_other_content_decides_nothing() {
        let dir = temp_dir("garbage");
        let id = new_id();
        request_approval(&dir, &approval(&id, 1)).unwrap();
        std::fs::write(decision_file(&dir, &id), "maybe\n").unwrap();
        assert_eq!(decision(&dir, &id).unwrap(), None);
        std::fs::write(decision_file(&dir, &id), "denied").unwrap();
        assert_eq!(
            decision(&dir, &id).unwrap(),
            Some((ApprovalDecision::Denied, UNKNOWN_DECIDER.into()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_approvals_of_dead_processes_are_cleaned_up() {
        let dir = temp_dir("stale");
        let (old, fresh) = (new_id(), new_id());
        request_approval(&dir, &approval(&old, 1)).unwrap();
        request_approval(&dir, &approval(&fresh, 2)).unwrap();
        let past =
            std::time::SystemTime::now() - std::time::Duration::from_millis(STALE_AFTER_MS + 5_000);
        std::fs::OpenOptions::new()
            .append(true)
            .open(pending_file(&dir, &old))
            .unwrap()
            .set_modified(past)
            .unwrap();
        let list = pending(&dir).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, fresh);
        assert!(!pending_file(&dir, &old).exists());
        // A heartbeat keeps a waiting approval alive.
        std::fs::OpenOptions::new()
            .append(true)
            .open(pending_file(&dir, &fresh))
            .unwrap()
            .set_modified(past)
            .unwrap();
        heartbeat(&dir, &fresh).unwrap();
        assert_eq!(pending(&dir).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn entry(tool: &str, at: i64) -> AuditEntry {
        AuditEntry {
            at,
            agent: "claude-code".into(),
            tool: tool.into(),
            collection: "/c".into(),
            request_path: None,
            environment: None,
            method: None,
            url: None,
            status: None,
            outcome: AuditOutcome::Read,
            approved_by: None,
            rule: None,
            detail: String::new(),
        }
    }

    #[test]
    fn audit_is_append_only_jsonl_newest_first() {
        let dir = temp_dir("audit");
        assert!(read_audit(&dir, 10).unwrap().is_empty());
        for i in 0..5 {
            append_audit(&dir, &entry(&format!("t{i}"), i)).unwrap();
        }
        let text = std::fs::read_to_string(audit_file(&dir)).unwrap();
        assert_eq!(text.lines().count(), 5);
        assert!(text.ends_with('\n'));
        // Empty optional fields are left out of the line.
        assert_eq!(
            text.lines().next().unwrap(),
            r#"{"at":0,"agent":"claude-code","tool":"t0","collection":"/c","outcome":"read"}"#
        );
        let last = read_audit(&dir, 2).unwrap();
        assert_eq!(
            last.iter().map(|e| e.tool.as_str()).collect::<Vec<_>>(),
            ["t4", "t3"]
        );
        // Earlier lines are never rewritten; a broken line is skipped.
        let before = std::fs::read(audit_file(&dir)).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(audit_file(&dir))
            .unwrap()
            .write_all(b"{broken\n")
            .unwrap();
        let mut sent = entry("send_request", 9);
        sent.outcome = AuditOutcome::Sent;
        sent.approved_by = Some("cli".into());
        sent.status = Some(201);
        append_audit(&dir, &sent).unwrap();
        let after = std::fs::read(audit_file(&dir)).unwrap();
        assert!(after.starts_with(&before));
        let all = read_audit(&dir, 100).unwrap();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0], sent);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
