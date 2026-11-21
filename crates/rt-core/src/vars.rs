// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Layered variables with provenance: every resolved value knows where it came from.
//!
//! Owner: wave 2, strand C.
//!
//! # Interpolation rules
//!
//! They follow Bruno (`packages/bruno-common/src/interpolate/index.ts`) so imported collections
//! resolve the same way:
//!
//! - A placeholder is `{{name}}`, where `name` is one or more characters up to the first `}`
//!   (Bruno's regular expression `\{\{([^}]+)\}\}`). `{{}}` is not a placeholder.
//! - Whitespace is part of the name: `{{ name }}` looks up `" name "`, which normally does not
//!   exist and stays unresolved (Bruno: "should strictly match the keys (whitespace matters)").
//! - Unknown names stay in the output verbatim and are reported in `unresolved`.
//! - Values may contain placeholders. They are resolved recursively against the whole scope,
//!   not just the layer the value came from. A placeholder whose expansion runs into a cycle
//!   stays verbatim and is reported as unresolved. (Bruno leaves a partly expanded placeholder
//!   in that case; we keep the one the user wrote.)
//! - Not resolved in wave 2, so they stay verbatim and unresolved: `{{process.env.NAME}}`
//!   (there is no process environment on wasm32, and exposing it is a policy decision),
//!   Bruno's dynamic variables such as `{{$randomInt}}`, and prompt variables `{{?Label}}`.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// Where a variable's value came from. Order of the variants is lowest to highest precedence,
/// matching Bruno so imported collections resolve the same way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "layer", rename_all = "kebab-case")]
pub enum VarSource {
    Global,
    Collection,
    Environment {
        name: String,
    },
    /// `path` as in `Folder::path`.
    Folder {
        path: String,
    },
    Request,
    /// Command line (`--var`), the UI, or a script at run time.
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarLayer {
    pub source: VarSource,
    /// Enabled variables with a value only. `(name, value, secret)`.
    pub vars: Vec<(String, String, bool)>,
}

/// All layers that apply to one request, lowest precedence first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scope {
    pub layers: Vec<VarLayer>,
}

/// One variable that was substituted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarUse {
    pub name: String,
    pub source: VarSource,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpolated {
    pub value: String,
    pub used: Vec<VarUse>,
    /// Names of `{{placeholders}}` with no value in any layer. They stay in `value` verbatim.
    pub unresolved: Vec<String>,
}

impl Scope {
    /// Looks a name up, highest precedence first. Within one layer the last definition wins.
    pub fn get(&self, name: &str) -> Option<(&str, &VarSource, bool)> {
        self.entry(name)
            .map(|(_, value, source, secret)| (value, source, secret))
    }

    /// Replaces every `{{name}}` in `template`. Variable values may themselves contain
    /// placeholders; resolution is recursive with cycle detection.
    pub fn interpolate(&self, template: &str) -> Interpolated {
        let mut resolver = Resolver {
            scope: self,
            cache: HashMap::new(),
            stack: Vec::new(),
        };
        let mut out = Expansion::default();
        // At the top level a cycle keeps the placeholder; below it, it fails the whole value.
        // So this call cannot fail.
        let _ = resolver.expand_into(template, &mut out, |out, name, placeholder| {
            out.value.push_str(placeholder);
            out.unresolved.push(name.to_owned());
            Ok(())
        });
        dedup_by_key(&mut out.used, |u| u.name.clone());
        dedup_by_key(&mut out.unresolved, Clone::clone);
        Interpolated {
            value: out.value,
            used: out.used,
            unresolved: out.unresolved,
        }
    }

    /// Like `get`, but also borrows the stored name.
    fn entry(&self, name: &str) -> Option<(&str, &str, &VarSource, bool)> {
        self.layers.iter().rev().find_map(|layer| {
            layer
                .vars
                .iter()
                .rev()
                .find(|(n, _, _)| n == name)
                .map(|(n, value, secret)| (n.as_str(), value.as_str(), &layer.source, *secret))
        })
    }
}

/// Finds the next placeholder at or after byte `from`: `(start, end, name)`, where
/// `template[start..end]` is the whole `{{name}}`. Same matches as Bruno's
/// `\{\{([^}]+)\}\}`.
fn next_placeholder(template: &str, from: usize) -> Option<(usize, usize, &str)> {
    let bytes = template.as_bytes();
    let mut at = from;
    while let Some(offset) = template[at..].find("{{") {
        let start = at + offset;
        let name_start = start + 2;
        // `[^}]+` runs to the first `}`; the match needs `}}` right there.
        let close = name_start + template[name_start..].find('}')?;
        if close > name_start && bytes.get(close + 1) == Some(&b'}') {
            return Some((start, close + 2, &template[name_start..close]));
        }
        at = start + 1;
    }
    None
}

/// Keeps the first occurrence of every key, in order.
pub(crate) fn dedup_by_key<T, K: Hash + Eq>(items: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(key(item)));
}

/// The expansion of a placeholder ran into a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cycle;

/// An expanded text. For a variable, `used` starts with the variable itself.
#[derive(Debug, Clone, Default)]
struct Expansion {
    value: String,
    used: Vec<VarUse>,
    unresolved: Vec<String>,
}

type Expanded = Result<Rc<Expansion>, Cycle>;

struct Resolver<'a> {
    scope: &'a Scope,
    /// Expansion results by name. They do not depend on the expansion stack: a name runs into
    /// a cycle exactly when a cycle is reachable from it.
    cache: HashMap<&'a str, Expanded>,
    /// Names being expanded right now, outermost first.
    stack: Vec<&'a str>,
}

impl<'a> Resolver<'a> {
    /// `None`: the name is not defined in any layer.
    fn expand_var(&mut self, name: &str) -> Option<Expanded> {
        let (name, value, source, secret) = self.scope.entry(name)?;
        if let Some(hit) = self.cache.get(name) {
            return Some(hit.clone());
        }
        if self.stack.contains(&name) {
            return Some(Err(Cycle));
        }
        self.stack.push(name);
        let mut out = Expansion::default();
        out.used.push(VarUse {
            name: name.to_owned(),
            source: source.clone(),
            secret,
        });
        let result = self
            .expand_into(value, &mut out, |_, _, _| Err(Cycle))
            .map(|()| Rc::new(out));
        self.stack.pop();
        self.cache.insert(name, result.clone());
        Some(result)
    }

    /// Appends the expansion of `template` to `out`. Unknown names stay verbatim;
    /// `on_cycle(out, name, placeholder)` decides what a cyclic placeholder does.
    fn expand_into(
        &mut self,
        template: &str,
        out: &mut Expansion,
        on_cycle: impl Fn(&mut Expansion, &str, &str) -> Result<(), Cycle>,
    ) -> Result<(), Cycle> {
        let mut rest = 0;
        while let Some((start, end, name)) = next_placeholder(template, rest) {
            out.value.push_str(&template[rest..start]);
            match self.expand_var(name) {
                Some(Ok(e)) => {
                    out.value.push_str(&e.value);
                    out.used.extend(e.used.iter().cloned());
                    out.unresolved.extend(e.unresolved.iter().cloned());
                }
                Some(Err(Cycle)) => on_cycle(out, name, &template[start..end])?,
                None => {
                    out.value.push_str(&template[start..end]);
                    out.unresolved.push(name.to_owned());
                }
            }
            rest = end;
        }
        out.value.push_str(&template[rest..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
