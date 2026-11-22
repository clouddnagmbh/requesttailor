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
