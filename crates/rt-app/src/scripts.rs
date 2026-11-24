// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts in the app (wave 7): trust and session variables, shared by the desktop and web
//! shells (`ScriptsApi` in `apps/ui/src/commands/backend.ts`).
//!
//! - **Trust** (strand A): scripts run only in a collection the user trusts. The shells keep
//!   the trusted collection ids (desktop: `trusted-scripts.json` in the data folder; web:
//!   browser storage). `rt send` and `rt run` run scripts (the user started them on purpose);
//!   `--no-scripts` switches them off.
//! - **Session variables** (strand C): what scripts, actions and "set as variable" set lives
//!   in memory per environment and goes into the runtime layer of every later send.

use rt_core::model::{Collection, Item};
use rt_core::script::VarChange;
use rt_core::send::SendReport;
use rt_core::services::{Note, NoteLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptTrust {
    pub has_scripts: bool,
    pub trusted: bool,
}

/// The note a send carries when the collection has scripts but is not trusted.
pub const UNTRUSTED_NOTE: &str = "Scripts were not run: this collection is not trusted yet.";

/// Whether any collection, folder or request of `collection` has a script.
pub fn has_scripts(collection: &Collection) -> bool {
    fn any(list: &[Item]) -> bool {
        list.iter().any(|item| match item {
            Item::Folder(f) => !f.defaults.scripts.is_empty() || any(&f.items),
            Item::Request(r) => !r.spec.scripts.is_empty(),
        })
    }
    !collection.defaults.scripts.is_empty() || any(&collection.items)
}

/// Whether sending `request_path` would run a script (collection, folders or request, any
/// phase).
pub fn request_has_scripts(collection: &Collection, request_path: &str) -> bool {
    use rt_core::model::ScriptPhase;
    collection.request(request_path).is_some_and(|context| {
        [
            ScriptPhase::BeforeRequest,
            ScriptPhase::AfterResponse,
            ScriptPhase::Tests,
        ]
        .into_iter()
        .any(|phase| !rt_core::script::scripts_for(&context, phase).is_empty())
    })
}

/// For a send that ran without the script runner because the collection is not trusted:
/// adds the [`UNTRUSTED_NOTE`] warning (source `script`) if the request has scripts.
pub fn note_untrusted(report: &mut SendReport, collection: &Collection) {
    if request_has_scripts(collection, &report.request_path)
        && !report.notes.iter().any(|n| n.text == UNTRUSTED_NOTE)
    {
        report.notes.push(Note {
            source: "script".into(),
            level: NoteLevel::Warning,
            text: UNTRUSTED_NOTE.into(),
        });
    }
}

/// Variables of the session, per environment (`""` for none), in the order set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionVars {
    by_environment: BTreeMap<String, Vec<VarChange>>,
}

impl SessionVars {
    pub fn list(&self, environment: &str) -> Vec<VarChange> {
        self.by_environment
            .get(environment)
            .cloned()
            .unwrap_or_default()
    }

    /// Applies a change: a value replaces the variable (moved to the end), `None` removes it.
    pub fn apply(&mut self, environment: &str, change: &VarChange) {
        let list = self
            .by_environment
            .entry(environment.to_owned())
            .or_default();
        list.retain(|c| c.name != change.name);
        if change.value.is_some() {
            list.push(change.clone());
        }
    }

    pub fn clear(&mut self, environment: &str) {
        self.by_environment.remove(environment);
    }

    /// `(name, value)` pairs for the runtime layer of a send, before the user's own `vars`.
    pub fn runtime(&self, environment: &str) -> Vec<(String, String)> {
        self.list(environment)
            .into_iter()
            .filter_map(|c| Some((c.name, c.value?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::model::VarScope;

    fn change(name: &str, value: Option<&str>) -> VarChange {
        VarChange {
            name: name.into(),
            value: value.map(str::to_owned),
            scope: VarScope::Runtime,
        }
    }

    fn report_for(path: &str) -> SendReport {
        SendReport {
            version: 1,
            request_path: path.into(),
            environment: None,
            request: None,
            response: None,
            variables: Vec::new(),
            unresolved: Vec::new(),
            error: None,
            notes: Vec::new(),
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            checks: Vec::new(),
            rate_limit: None,
            segments: Vec::new(),
        }
    }

    #[test]
    fn untrusted_note_only_for_requests_with_scripts() {
        use rt_core::model::{Script, ScriptPhase};
        let files = rt_format::FileSet {
            files: [
                ("opencollection.yml", "opencollection: \"1.0.0\"\ninfo:\n  name: c\n"),
                (
                    "a.yml",
                    "info:\n  name: a\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://h/\n",
                ),
                (
                    "b.yml",
                    "info:\n  name: b\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: http://h/\n",
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.as_bytes().to_vec()))
            .collect(),
        };
        let mut collection = rt_format::read_collection(&files).unwrap().model().clone();
        assert!(!has_scripts(&collection));
        let Some(Item::Request(b)) = collection.items.iter_mut().find(|i| match i {
            Item::Request(r) => r.path == "b.yml",
            _ => false,
        }) else {
            panic!("no b.yml");
        };
        b.spec.scripts.push(Script {
            phase: ScriptPhase::Tests,
            code: "test('x', () => {})".into(),
        });
        assert!(has_scripts(&collection));
        assert!(!request_has_scripts(&collection, "a.yml"));
        assert!(request_has_scripts(&collection, "b.yml"));

        let mut report = report_for("a.yml");
        note_untrusted(&mut report, &collection);
        assert!(report.notes.is_empty());
        let mut report = report_for("b.yml");
        note_untrusted(&mut report, &collection);
        note_untrusted(&mut report, &collection);
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].source, "script");
        assert_eq!(report.notes[0].level, NoteLevel::Warning);
        assert_eq!(report.notes[0].text, UNTRUSTED_NOTE);
    }

    #[test]
    fn session_vars_replace_unset_and_clear_per_environment() {
        let mut s = SessionVars::default();
        s.apply("Local", &change("a", Some("1")));
        s.apply("Local", &change("b", Some("2")));
        s.apply("Local", &change("a", Some("3")));
        s.apply("Prod", &change("a", Some("9")));
        assert_eq!(
            s.runtime("Local"),
            vec![("b".into(), "2".into()), ("a".into(), "3".into())]
        );
        s.apply("Local", &change("b", None));
        assert_eq!(s.runtime("Local"), vec![("a".into(), "3".into())]);
        s.clear("Local");
        assert!(s.list("Local").is_empty());
        assert_eq!(s.list("Prod").len(), 1);
    }
}
