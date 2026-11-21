// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Variables and environments.

use serde::{Deserialize, Serialize};

/// A variable as declared in a file. Secret variables carry no value in the file
/// (product principle 3); their value comes from the keychain, a vault or the command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    /// `None` for secrets and for variables declared without a value.
    pub value: Option<String>,
    pub enabled: bool,
    pub secret: bool,
}

impl Variable {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
            enabled: true,
            secret: false,
        }
    }
}

/// A named set of variables, e.g. `Local`, `DEV`, `PRD`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub variables: Vec<Variable>,
}
