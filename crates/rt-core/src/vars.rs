// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Layered variables with provenance: every resolved value knows where it came from.
//!
//! Owner: wave 2, strand C.

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
    /// Looks a name up, highest precedence first.
    pub fn get(&self, name: &str) -> Option<(&str, &VarSource, bool)> {
        let _ = name;
        todo!("wave 2, strand C")
    }

    /// Replaces every `{{name}}` in `template`. Variable values may themselves contain
    /// placeholders; resolution is recursive with cycle detection.
    pub fn interpolate(&self, template: &str) -> Interpolated {
        let _ = template;
        todo!("wave 2, strand C")
    }
}

#[cfg(test)]
mod tests;
