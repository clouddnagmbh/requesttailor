// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The service model read from `$metadata` (EDMX), for V2 and V4 alike. Strand A fills it
//! (`metadata::parse`); the query builder (B), writes (C) and landscapes (D) read it. It is
//! stored as JSON in the plugin's storage (`metadata::cache`), so changes here are interface
//! changes of the plugin.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Version {
    V2,
    V4,
}

/// One service: its root URL and what `$metadata` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub version: Version,
    /// Service root as requested, without query, with a trailing `/`
    /// (e.g. `https://host/sap/opu/odata/sap/ZRT_SALES_SRV/`).
    pub service_root: String,
    /// Schema namespace of the entity container.
    pub namespace: String,
    pub entity_types: Vec<EntityType>,
    pub complex_types: Vec<ComplexType>,
    pub entity_sets: Vec<EntitySet>,
    /// V2 function imports and V4 action/function imports.
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityType {
    /// Qualified: `Namespace.Name`.
    pub name: String,
    pub key: Vec<String>,
    pub properties: Vec<Property>,
    pub navigation: Vec<Navigation>,
    /// V4 `HasStream` / V2 `m:HasStream`.
    #[serde(default)]
    pub media: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplexType {
    pub name: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    /// EDM type as written: `Edm.String`, `Edm.Decimal`, `Collection(Edm.String)`, or a
    /// qualified complex/enum type.
    pub r#type: String,
    pub nullable: bool,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub precision: Option<u32>,
    #[serde(default)]
    pub scale: Option<String>,
    /// SAP V2 annotations and V4 vocabulary terms that the UI uses, e.g. `label`,
    /// `filterable=false`, `creatable=false`, `updatable=false` (name → value).
    #[serde(default)]
    pub annotations: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Navigation {
    pub name: String,
    /// Qualified target entity type.
    pub target: String,
    /// `true` for to-many (V2 multiplicity `*`, V4 `Collection(...)`).
    pub many: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySet {
    pub name: String,
    /// Qualified entity type.
    pub entity_type: String,
    /// SAP V2 `sap:creatable` etc. and V4 capabilities; `true` when not stated.
    pub insertable: bool,
    pub updatable: bool,
    pub deletable: bool,
    /// Draft-enabled (V4 `Common.DraftRoot` / RAP and CAP drafts).
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    /// `function` (GET) or `action` (POST); V2 function imports by their `m:HttpMethod`.
    pub kind: OperationKind,
    /// Bound to this qualified entity type (V4 bound operations), else unbound.
    #[serde(default)]
    pub bound_to: Option<String>,
    pub parameters: Vec<Property>,
    #[serde(default)]
    pub returns: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationKind {
    Function,
    Action,
}

impl Model {
    pub fn entity_type(&self, qualified: &str) -> Option<&EntityType> {
        self.entity_types.iter().find(|t| t.name == qualified)
    }

    pub fn entity_set(&self, name: &str) -> Option<&EntitySet> {
        self.entity_sets.iter().find(|s| s.name == name)
    }
}
