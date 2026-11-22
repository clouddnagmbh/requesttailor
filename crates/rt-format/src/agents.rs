// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.agents` of `opencollection.yml` ↔ [`AgentPolicy`] (wave 8,
//! strand C). Format and rules: `rt_core::model::agents`.
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     agents:
//!       access: read
//!       environments:
//!         Local: { access: write }
//!         PRD: { access: write, production: true }
//! ```
//!
//! Reading is lenient but never more permissive than the file says:
//! - a missing `agents` is the default policy (`access: read`, no environments);
//! - an unknown `access` value reads as `read`; an environment without `access` gets the
//!   default `access`; an environment written as a scalar (`PRD: none`) is its access;
//! - `production` is true unless it is missing or exactly `false`.
//!
//! Writing ([`CollectionDoc::set_agent_policy`]) changes only what differs: an unchanged
//! environment entry keeps its text, a changed one is written as `{ access: …, production:
//! true }`, new ones go at the end. The default policy removes `agents` (and an
//! `extensions.requesttailor` or `extensions` that becomes empty). Every edit is read back
//! and refused if it does not give the policy.

use rt_core::model::{AgentAccess, AgentPolicy, EnvironmentPolicy};

use crate::yaml::{Entry, Map, Node, Scalar, Style, Val};
use crate::{COLLECTION_FILE, CollectionDoc, FormatError, Source};

/// Top-level keys of `opencollection.yml` in Bruno's order (as in `auth/certs.rs`).
const ROOT_ORDER: &[&str] = &[
    "opencollection",
    "info",
    "config",
    "request",
    "docs",
    "bundled",
    "extensions",
];

/// Keys of `extensions.requesttailor` in the order RequestTailor writes them.
const EXTENSION_ORDER: &[&str] = &["auth", "agents", "http3"];

const AGENTS_ORDER: &[&str] = &["access", "environments"];

fn access_word(a: AgentAccess) -> &'static str {
    match a {
        AgentAccess::None => "none",
        AgentAccess::Read => "read",
        AgentAccess::Write => "write",
    }
}

fn parse_access(node: Option<&Node>) -> Option<AgentAccess> {
    match node.and_then(Node::text)? {
        "none" => Some(AgentAccess::None),
        "read" => Some(AgentAccess::Read),
        "write" => Some(AgentAccess::Write),
        // Unknown words never grant writing.
        _ => Some(AgentAccess::Read),
    }
}

fn agents_node(root: &Node) -> Option<&Node> {
    root.get("extensions")?.get("requesttailor")?.get("agents")
}

/// One environment entry, with `default` for a missing `access`.
fn read_environment(name: &str, value: &Node, default: AgentAccess) -> EnvironmentPolicy {
    if value.is_map() {
        EnvironmentPolicy {
            environment: name.to_owned(),
            access: parse_access(value.get("access")).unwrap_or(default),
            production: value
                .get("production")
                .is_some_and(|p| p.as_bool() != Some(false)),
        }
    } else {
        EnvironmentPolicy {
            environment: name.to_owned(),
            access: parse_access(Some(value)).unwrap_or(default),
            production: false,
        }
    }
}

/// The policy of `opencollection.yml` (`root` is that file's root node).
pub(crate) fn read(root: &Node) -> AgentPolicy {
    let Some(agents) = agents_node(root).filter(|n| n.is_map()) else {
        return AgentPolicy::default();
    };
    let access = parse_access(agents.get("access")).unwrap_or_default();
    let environments = match agents.get("environments") {
        Some(Node::Map(m)) => m
            .entries
            .iter()
            .map(|e| read_environment(&e.key.value, &e.value, access))
            .collect(),
        _ => Vec::new(),
    };
    AgentPolicy {
        access,
        environments,
    }
}

/// `{ access: write, production: true }`.
fn entry_text(e: &EnvironmentPolicy) -> String {
    if e.production {
        format!("{{ access: {}, production: true }}", access_word(e.access))
    } else {
        format!("{{ access: {} }}", access_word(e.access))
    }
}

/// A plain scalar written as `raw` (also a whole flow mapping).
fn verbatim(raw: String, owner_indent: usize) -> Node {
    Node::Scalar(Scalar {
        value: raw.clone(),
        style: Style::Plain,
        raw,
        owner_indent: owner_indent as isize,
        in_flow: false,
        is_key: false,
        comment_after: false,
    })
}

/// A new entry `name: { … }` at the end of the block mapping `map`.
fn push_entry(map: &mut Map, e: &EnvironmentPolicy, nl: &str) {
    let indent = map.indent;
    let mut key = Scalar {
        value: String::new(),
        style: Style::Plain,
        raw: String::new(),
        owner_indent: indent as isize,
        in_flow: false,
        is_key: true,
        comment_after: false,
    };
    // Quotes names that need them, as Bruno would.
    key.set_str(&e.environment, nl);
    map.entries.push(Entry {
        lead: format!("{nl}{}", " ".repeat(indent)),
        key,
        sep: ": ".to_owned(),
        value: verbatim(entry_text(e), indent),
    });
}

/// Brings the block mapping `environments` to `wanted` (read with `default`).
fn sync_environments(map: &mut Map, wanted: &[EnvironmentPolicy], default: AgentAccess, nl: &str) {
    map.entries
        .retain(|e| wanted.iter().any(|w| w.environment == e.key.value));
    for w in wanted {
        match map
            .entries
            .iter_mut()
            .find(|e| e.key.value == w.environment)
        {
            Some(entry) if read_environment(&w.environment, &entry.value, default) == *w => {}
            Some(entry) => {
                entry.sep = ": ".to_owned();
                entry.value = verbatim(entry_text(w), map.indent);
            }
            None => push_entry(map, w, nl),
        }
    }
}

/// The same policy, ignoring the order of the environments.
fn same(a: &AgentPolicy, b: &AgentPolicy) -> bool {
    let sorted = |p: &AgentPolicy| {
        let mut e = p.environments.clone();
        e.sort_by(|x, y| x.environment.cmp(&y.environment));
        e
    };
    a.access == b.access && sorted(a) == sorted(b)
}

fn is_default(p: &AgentPolicy) -> bool {
    p.access == AgentAccess::Read && p.environments.is_empty()
}

impl CollectionDoc {
    /// Sets what coding agents may do (`extensions.requesttailor.agents` of
    /// `opencollection.yml`; rules in `rt_core::model::agents`). Only what differs is
    /// rewritten (see the `agents` module of this crate); the edit is read back and refused
    /// if it does not give `policy`. Environment names must be unique.
    ///
    /// Owner: wave 8, strand C.
    pub fn set_agent_policy(&mut self, policy: &AgentPolicy) -> Result<(), FormatError> {
        let invalid = |message: String| FormatError::Invalid {
            path: COLLECTION_FILE.to_owned(),
            message,
        };
        for (i, e) in policy.environments.iter().enumerate() {
            if e.environment.is_empty() {
                return Err(invalid("an agent rule needs an environment name".into()));
            }
            if policy.environments[..i]
                .iter()
                .any(|o| o.environment == e.environment)
            {
                return Err(invalid(format!(
                    "the environment {} has two agent rules",
                    e.environment
                )));
            }
        }
        if same(&self.model.config.agents, policy) {
            return Ok(());
        }
        let Some(Source::Yaml(doc)) = self.files.get(COLLECTION_FILE) else {
            return Err(invalid("the collection file cannot be edited".into()));
        };
        let mut doc = doc.clone();
        let nl = doc.newline;
        let not_block = |key: &str| {
            invalid(format!(
                "`{key}` is not a block mapping and cannot be edited"
            ))
        };

        // Phase 1: the `agents` mapping with its `access`, creating parents as needed.
        {
            let root = doc
                .root
                .block_map_mut()
                .ok_or_else(|| invalid("the file is not a YAML mapping".into()))?;
            let access = Val::Plain(access_word(policy.access).to_owned());
            if is_default(policy) {
                remove_agents(root).map_err(&not_block)?;
            } else {
                match root.get_mut("extensions") {
                    None => root.put(
                        "extensions",
                        &Val::Map(vec![(
                            "requesttailor",
                            Val::Map(vec![("agents", Val::Map(vec![("access", access)]))]),
                        )]),
                        ROOT_ORDER,
                        true,
                        nl,
                    ),
                    Some(ext) => {
                        let ext = ext.block_map_mut().ok_or_else(|| not_block("extensions"))?;
                        match ext.get_mut("requesttailor") {
                            None => ext.put(
                                "requesttailor",
                                &Val::Map(vec![("agents", Val::Map(vec![("access", access)]))]),
                                &[],
                                false,
                                nl,
                            ),
                            Some(rt) => {
                                let rt = rt
                                    .block_map_mut()
                                    .ok_or_else(|| not_block("extensions.requesttailor"))?;
                                match rt.get_mut("agents").and_then(Node::block_map_mut) {
                                    Some(agents) => {
                                        agents.put("access", &access, AGENTS_ORDER, false, nl);
                                        if policy.environments.is_empty() {
                                            agents.remove("environments");
                                        }
                                    }
                                    // Missing, or a flow mapping or scalar: written anew.
                                    None => rt.put(
                                        "agents",
                                        &Val::Map(vec![("access", access)]),
                                        EXTENSION_ORDER,
                                        false,
                                        nl,
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut doc = crate::yaml::reparse(&doc).map_err(invalid)?;

        // Phase 2: the environments.
        if !policy.environments.is_empty() {
            let agents = doc
                .root
                .block_map_mut()
                .and_then(|r| r.get_mut("extensions"))
                .and_then(Node::block_map_mut)
                .and_then(|e| e.get_mut("requesttailor"))
                .and_then(Node::block_map_mut)
                .and_then(|r| r.get_mut("agents"))
                .and_then(Node::block_map_mut)
                .ok_or_else(|| invalid("internal error: `agents` was not written".into()))?;
            match agents.get_mut("environments").and_then(Node::block_map_mut) {
                Some(map) => sync_environments(map, &policy.environments, policy.access, nl),
                None => {
                    // Missing or not a block mapping: a placeholder entry, then the real ones.
                    agents.put(
                        "environments",
                        &Val::Map(vec![("x", Val::Plain("x".into()))]),
                        AGENTS_ORDER,
                        false,
                        nl,
                    );
                    doc = crate::yaml::reparse(&doc).map_err(invalid)?;
                    let map = doc
                        .root
                        .block_map_mut()
                        .and_then(|r| r.get_mut("extensions"))
                        .and_then(Node::block_map_mut)
                        .and_then(|e| e.get_mut("requesttailor"))
                        .and_then(Node::block_map_mut)
                        .and_then(|r| r.get_mut("agents"))
                        .and_then(Node::block_map_mut)
                        .and_then(|a| a.get_mut("environments"))
                        .and_then(Node::block_map_mut)
                        .ok_or_else(|| {
                            invalid("internal error: `environments` was not written".into())
                        })?;
                    let placeholder = map.entries.remove(0);
                    for (i, w) in policy.environments.iter().enumerate() {
                        push_entry(map, w, nl);
                        if i == 0 {
                            // The first entry takes over the placeholder's line break.
                            map.entries[0].lead = placeholder.lead.clone();
                        }
                    }
                }
            }
            doc = crate::yaml::reparse(&doc).map_err(invalid)?;
        }

        let got = read(&doc.root);
        if !same(&got, policy) {
            return Err(invalid(format!(
                "internal error: the agent rules do not read back as written \
                 (wanted {policy:?}, got {got:?})"
            )));
        }
        self.files
            .insert(COLLECTION_FILE.to_owned(), Source::Yaml(doc));
        self.model.config.agents = got;
        Ok(())
    }
}

/// Removes `extensions.requesttailor.agents` and parents that become empty. `Err` names a
/// parent that is not a block mapping.
fn remove_agents(root: &mut Map) -> Result<(), &'static str> {
    let Some(ext) = root.get_mut("extensions") else {
        return Ok(());
    };
    let ext = ext.block_map_mut().ok_or("extensions")?;
    let Some(rt) = ext.get_mut("requesttailor") else {
        return Ok(());
    };
    let rt = rt.block_map_mut().ok_or("extensions.requesttailor")?;
    rt.remove("agents");
    if rt.is_empty() {
        ext.remove("requesttailor");
    }
    if ext.is_empty() {
        root.remove("extensions");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileSet, read_collection};

    const HEAD: &str = "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n";

    fn doc(collection: &str) -> CollectionDoc {
        let files = FileSet {
            files: [(COLLECTION_FILE.to_owned(), collection.as_bytes().to_vec())]
                .into_iter()
                .collect(),
        };
        read_collection(&files).unwrap()
    }

    fn text(d: &CollectionDoc) -> String {
        String::from_utf8(d.write().files[COLLECTION_FILE].clone()).unwrap()
    }

    fn env(name: &str, access: AgentAccess, production: bool) -> EnvironmentPolicy {
        EnvironmentPolicy {
            environment: name.into(),
            access,
            production,
        }
    }

    fn policy(access: AgentAccess, envs: Vec<EnvironmentPolicy>) -> AgentPolicy {
        AgentPolicy {
            access,
            environments: envs,
        }
    }

    const EXAMPLE: &str = "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: read            # default\n      environments:\n        Local: { access: write }\n        PRD: { access: write, production: true }\n        Vault: { access: none }\n";

    #[test]
    fn reads_the_documented_format() {
        let d = doc(EXAMPLE);
        assert_eq!(
            d.model().config.agents,
            policy(
                AgentAccess::Read,
                vec![
                    env("Local", AgentAccess::Write, false),
                    env("PRD", AgentAccess::Write, true),
                    env("Vault", AgentAccess::None, false),
                ]
            )
        );
        // Unchanged documents write back byte for byte.
        assert_eq!(text(&d), EXAMPLE);
    }

    #[test]
    fn reads_leniently_but_never_more_permissive() {
        assert_eq!(doc(HEAD).model().config.agents, AgentPolicy::default());
        let d = doc(&format!(
            "{HEAD}extensions:\n  requesttailor:\n    agents:\n      access: everything\n      environments:\n        A:\n          production: yes please\n        B: none\n        C: {{ access: write, production: false }}\n        \"D: x\": {{ access: sudo }}\n"
        ));
        assert_eq!(
            d.model().config.agents,
            policy(
                AgentAccess::Read,
                vec![
                    env("A", AgentAccess::Read, true),
                    env("B", AgentAccess::None, false),
                    env("C", AgentAccess::Write, false),
                    env("D: x", AgentAccess::Read, false),
                ]
            )
        );
        // An environment without `access` gets the default.
        let d = doc(&format!(
            "{HEAD}extensions:\n  requesttailor:\n    agents:\n      access: write\n      environments:\n        PRD: {{ production: true }}\n"
        ));
        assert_eq!(
            d.model().config.agents.environments,
            [env("PRD", AgentAccess::Write, true)]
        );
    }

    #[test]
    fn writes_a_new_policy_into_a_file_without_extensions() {
        let mut d = doc(HEAD);
        let p = policy(
            AgentAccess::Read,
            vec![
                env("Local", AgentAccess::Write, false),
                env("PRD", AgentAccess::Write, true),
            ],
        );
        d.set_agent_policy(&p).unwrap();
        assert_eq!(
            text(&d),
            format!(
                "{HEAD}\nextensions:\n  requesttailor:\n    agents:\n      access: read\n      environments:\n        Local: {{ access: write }}\n        PRD: {{ access: write, production: true }}\n"
            )
        );
        assert_eq!(d.model().config.agents, p);
        // Read back from the written files.
        assert_eq!(doc(&text(&d)).model().config.agents, p);
    }

    #[test]
    fn keeps_other_extensions_and_untouched_entries() {
        let original = format!(
            "{HEAD}\nextensions:\n  requesttailor:\n    auth:\n      type: oauth2\n    agents:\n      access: read\n      environments:\n        Local: {{access: write}}   # dev box\n        PRD: {{ access: write, production: true }}\n  other:\n    keep: me\n"
        );
        let mut d = doc(&original);
        let mut p = d.model().config.agents.clone();
        p.environments[1].access = AgentAccess::Read;
        p.environments
            .push(env("QA Box", AgentAccess::Write, false));
        p.environments.push(env("true", AgentAccess::None, false));
        d.set_agent_policy(&p).unwrap();
        assert_eq!(
            text(&d),
            format!(
                "{HEAD}\nextensions:\n  requesttailor:\n    auth:\n      type: oauth2\n    agents:\n      access: read\n      environments:\n        Local: {{access: write}}   # dev box\n        PRD: {{ access: read, production: true }}\n        QA Box: {{ access: write }}\n        \"true\": {{ access: none }}\n  other:\n    keep: me\n"
            )
        );
        assert_eq!(doc(&text(&d)).model().config.agents, p);

        // Removing an environment and changing the default.
        p.environments.retain(|e| e.environment != "QA Box");
        p.access = AgentAccess::None;
        d.set_agent_policy(&p).unwrap();
        let t = text(&d);
        assert!(t.contains("      access: none\n"), "{t}");
        assert!(!t.contains("QA Box"), "{t}");
        assert!(t.contains("Local: {access: write}   # dev box"), "{t}");
        assert_eq!(doc(&t).model().config.agents, p);
    }

    #[test]
    fn the_default_policy_removes_what_it_added() {
        let mut d = doc(HEAD);
        d.set_agent_policy(&policy(AgentAccess::Write, vec![]))
            .unwrap();
        assert_eq!(
            text(&d),
            format!("{HEAD}\nextensions:\n  requesttailor:\n    agents:\n      access: write\n")
        );
        d.set_agent_policy(&AgentPolicy::default()).unwrap();
        assert_eq!(text(&d), HEAD);
        // Other extensions stay.
        let mut d = doc(EXAMPLE
            .replace("  requesttailor:\n", "  other: 1\n  requesttailor:\n")
            .as_str());
        d.set_agent_policy(&AgentPolicy::default()).unwrap();
        assert_eq!(text(&d), format!("{HEAD}\nextensions:\n  other: 1\n"));
    }

    #[test]
    fn unchanged_policy_and_bad_input() {
        let mut d = doc(EXAMPLE);
        let same_policy = d.model().config.agents.clone();
        d.set_agent_policy(&same_policy).unwrap();
        assert_eq!(text(&d), EXAMPLE);
        let dup = policy(
            AgentAccess::Read,
            vec![
                env("A", AgentAccess::Read, false),
                env("A", AgentAccess::Write, false),
            ],
        );
        assert!(d.set_agent_policy(&dup).is_err());
        let unnamed = policy(AgentAccess::Read, vec![env("", AgentAccess::Read, false)]);
        assert!(d.set_agent_policy(&unnamed).is_err());
        assert_eq!(text(&d), EXAMPLE);
        // A flow-style `extensions` is refused, the file stays.
        let flow = format!("{HEAD}extensions: {{ other: 1 }}\n");
        let mut d = doc(&flow);
        assert!(
            d.set_agent_policy(&policy(AgentAccess::Write, vec![]))
                .is_err()
        );
        assert_eq!(text(&d), flow);
        // A flow-style `agents` is written anew.
        let mut d = doc(&format!(
            "{HEAD}extensions:\n  requesttailor:\n    agents: {{ access: none }}\n"
        ));
        let p = policy(
            AgentAccess::Read,
            vec![env("PRD", AgentAccess::Write, true)],
        );
        d.set_agent_policy(&p).unwrap();
        assert_eq!(doc(&text(&d)).model().config.agents, p);
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let crlf = HEAD.replace('\n', "\r\n");
        let mut d = doc(&crlf);
        let p = policy(
            AgentAccess::Read,
            vec![env("PRD", AgentAccess::Write, true)],
        );
        d.set_agent_policy(&p).unwrap();
        let t = text(&d);
        assert!(!t.replace("\r\n", "").contains('\n'), "{t:?}");
        assert_eq!(doc(&t).model().config.agents, p);
    }
}
