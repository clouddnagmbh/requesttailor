// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! EDMX (`$metadata`) into [`Model`], for OData V2 (`edmx:Edmx Version="1.0"`, also the V3
//! layout) and V4 (`Version="4.0"`/`"4.01"`).
//!
//! Names are normalized: aliases (`SAP__self.SalesOrderType`, `Common.Label`) are replaced by
//! their namespaces, so every qualified name in the model uses the schema namespace.
//!
//! What goes where:
//! - V2: `Association`s give navigation targets and multiplicity (`*` is to-many); the
//!   `sap:creatable`, `sap:updatable`, `sap:deletable` attributes of entity sets give their
//!   capabilities; property attributes in [`SAP_PROPERTY_ATTRIBUTES`] become annotations of
//!   the same name, `ConcurrencyMode="Fixed"` becomes `etag=true`; `FunctionImport`s are
//!   operations, `m:HttpMethod="GET"` (or none) a function, any other method an action.
//! - V4: `NavigationProperty Type="Collection(...)"` is to-many; `Action`s and `Function`s
//!   are operations: bound ones with `bound_to` (the binding parameter's type, as written,
//!   `Collection(...)` for collection-bound ones) and without the binding parameter, unbound
//!   ones only through an `ActionImport`/`FunctionImport`, named after the import.
//! - Annotations (inline or in `Annotations Target="..."`, V4 vocabularies, also found in V2
//!   documents): `Common.DraftRoot` sets `EntitySet::draft`; `Capabilities.Insert-`,
//!   `Update-` and `DeleteRestrictions` the set's capabilities; `FilterRestrictions/
//!   NonFilterableProperties` and `SortRestrictions/NonSortableProperties` give
//!   `filterable=false`/`sortable=false`; `Core.OptimisticConcurrency` gives `etag=true`;
//!   `Common.Label` gives `label`, `Core.Computed` `computed=true`, `Core.Immutable`
//!   `immutable=true` on properties.
//!
//! [`Operation::name`] is the simple name; a bound operation is called as
//! `<namespace>.<name>` (the namespace of [`Model::namespace`]).

use std::collections::BTreeMap;

use crate::edm::{
    ComplexType, EntitySet, EntityType, Model, Navigation, Operation, OperationKind, Property,
    Version,
};

use super::xml::{self, Element};

/// Namespace of `sap:` attributes (SAP Gateway V2).
pub const SAP_NS: &str = "http://www.sap.com/Protocols/SAPData";
/// Namespace of `m:` attributes (V2 `m:HttpMethod`, `m:HasStream`, ...).
pub const METADATA_NS: &str = "http://schemas.microsoft.com/ado/2007/08/dataservices/metadata";
const ATOM_NS: &str = "http://www.w3.org/2005/Atom";

/// `sap:` attributes of V2 properties kept as annotations (same name, value as written).
pub const SAP_PROPERTY_ATTRIBUTES: [&str; 11] = [
    "label",
    "heading",
    "quickinfo",
    "creatable",
    "updatable",
    "filterable",
    "sortable",
    "required-in-filter",
    "unit",
    "semantics",
    "display-format",
];

const COMMON: &str = "com.sap.vocabularies.Common.v1.";
const CORE: &str = "Org.OData.Core.V1.";
const CAPABILITIES: &str = "Org.OData.Capabilities.V1.";

/// Everything read from a document: the model and what it could not hold.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub model: Model,
    /// Service root of an `atom:link rel="self"` to `$metadata` (SAP Gateway writes one).
    pub self_root: Option<String>,
    /// V4 singletons (not in the model).
    pub singletons: Vec<String>,
    /// V4 unbound actions and functions without an import (not callable by URL).
    pub without_import: Vec<String>,
    /// V2 function imports whose HTTP method is not GET or POST (`name`, method).
    pub other_methods: Vec<(String, String)>,
}

pub fn parse(text: &str, service_root: &str) -> Result<Parsed, String> {
    let root = xml::parse(text)?;
    if root.name != "Edmx" {
        return Err(format!(
            "not an EDMX document (root element <{}>)",
            root.name
        ));
    }
    let version = match root.attr("Version") {
        Some(v) if v.starts_with('4') => Version::V4,
        Some(v) if v.starts_with('1') || v.starts_with('2') || v.starts_with('3') => Version::V2,
        Some(v) => return Err(format!("unsupported EDMX version {v}")),
        None => return Err("EDMX without a Version".into()),
    };
    let services = root
        .child("DataServices")
        .ok_or("EDMX without DataServices")?;
    let schemas: Vec<&Element> = services.children_named("Schema").collect();
    if schemas.is_empty() {
        return Err("EDMX without a Schema".into());
    }

    let mut names = Names::default();
    for include in root
        .children_named("Reference")
        .flat_map(|r| r.children_named("Include"))
    {
        if let (Some(ns), Some(alias)) = (include.attr("Namespace"), include.attr("Alias")) {
            names.alias(alias, ns);
        }
    }
    for schema in &schemas {
        if let (Some(ns), Some(alias)) = (schema.attr("Namespace"), schema.attr("Alias")) {
            names.alias(alias, ns);
        }
    }

    let mut b = Builder {
        version: Some(version),
        names,
        ..Builder::default()
    };
    for schema in &schemas {
        b.schema(schema);
    }
    let (namespace, container) = b
        .container
        .take()
        .ok_or("EDMX without an EntityContainer")?;
    let mut model = Model {
        version,
        service_root: with_slash(service_root),
        namespace: namespace.clone(),
        entity_types: Vec::new(),
        complex_types: Vec::new(),
        entity_sets: Vec::new(),
        operations: Vec::new(),
    };
    b.types(&mut model);
    let mut parsed = Parsed {
        model,
        self_root: self_root(&root),
        singletons: Vec::new(),
        without_import: Vec::new(),
        other_methods: Vec::new(),
    };
    b.entity_container(&namespace, container, &mut parsed);
    b.annotations(&namespace, container, &mut parsed.model);
    Ok(parsed)
}

fn with_slash(root: &str) -> String {
    if root.is_empty() || root.ends_with('/') {
        root.to_owned()
    } else {
        format!("{root}/")
    }
}

/// The service root of `<atom:link rel="self" href=".../$metadata"/>`, if any.
fn self_root(root: &Element) -> Option<String> {
    root.descendants().into_iter().find_map(|e| {
        if e.ns != ATOM_NS || e.name != "link" || e.attr("rel") != Some("self") {
            return None;
        }
        let href = e.attr("href")?;
        let base = href.split(['?', '#']).next()?;
        let root = base.strip_suffix("$metadata")?;
        (root.starts_with("http://") || root.starts_with("https://")).then(|| with_slash(root))
    })
}

/// Alias → namespace.
#[derive(Debug, Default)]
struct Names {
    aliases: BTreeMap<String, String>,
}

impl Names {
    fn alias(&mut self, alias: &str, namespace: &str) {
        self.aliases.insert(alias.to_owned(), namespace.to_owned());
    }

    /// `Alias.Name` → `Namespace.Name`, also inside `Collection(...)`.
    fn qualify(&self, name: &str) -> String {
        let name = name.trim();
        if let Some(inner) = name
            .strip_prefix("Collection(")
            .and_then(|r| r.strip_suffix(')'))
        {
            return format!("Collection({})", self.qualify(inner));
        }
        match name.rsplit_once('.') {
            Some((prefix, local)) => match self.aliases.get(prefix) {
                Some(ns) => format!("{ns}.{local}"),
                None => name.to_owned(),
            },
            None => name.to_owned(),
        }
    }
}

/// An entity or complex type as declared (before base types are applied).
#[derive(Debug, Clone, Default)]
struct RawType {
    base: Option<String>,
    key: Vec<String>,
    properties: Vec<Property>,
    navigation: Vec<RawNavigation>,
    media: bool,
}

#[derive(Debug, Clone)]
enum RawNavigation {
    /// V2: `Relationship` and `ToRole`.
    V2 {
        name: String,
        relationship: String,
        to_role: String,
    },
    V4(Navigation),
}

#[derive(Debug, Default)]
struct Builder<'a> {
    version: Option<Version>,
    names: Names,
    entity_types: Vec<(String, RawType)>,
    complex_types: Vec<(String, RawType)>,
    /// Qualified association → role → (type, multiplicity).
    associations: BTreeMap<String, Vec<(String, String, String)>>,
    /// V4 actions and functions: qualified name, element.
    operations: Vec<(String, OperationKind, &'a Element)>,
    annotations: Vec<(String, &'a Element)>,
    /// Schema namespace and the container: the default one (`m:IsDefaultEntityContainer`) or
    /// the first.
    container: Option<(String, &'a Element)>,
}

impl<'a> Builder<'a> {
    fn version(&self) -> Version {
        self.version.unwrap_or(Version::V4)
    }

    fn schema(&mut self, schema: &'a Element) {
        let ns = schema.attr("Namespace").unwrap_or_default().to_owned();
        let qualified = |name: &str| format!("{ns}.{name}");
        for e in &schema.children {
            let Some(name) = e.attr("Name") else {
                if e.name == "Annotations"
                    && let Some(target) = e.attr("Target")
                {
                    self.annotations.push((target.to_owned(), e));
                }
                continue;
            };
            match e.name.as_str() {
                "EntityType" => {
                    let t = self.raw_type(e, &qualified(name));
                    self.entity_types.push((qualified(name), t));
                }
                "ComplexType" => {
                    let t = self.raw_type(e, &qualified(name));
                    self.complex_types.push((qualified(name), t));
                }
                "Association" => {
                    let ends = e
                        .children_named("End")
                        .map(|end| {
                            (
                                end.attr("Role").unwrap_or_default().to_owned(),
                                self.names.qualify(end.attr("Type").unwrap_or_default()),
                                end.attr("Multiplicity").unwrap_or_default().to_owned(),
                            )
                        })
                        .collect();
                    self.associations.insert(qualified(name), ends);
                }
                "Action" => self
                    .operations
                    .push((qualified(name), OperationKind::Action, e)),
                "Function" => self
                    .operations
                    .push((qualified(name), OperationKind::Function, e)),
                "EntityContainer" => {
                    let default =
                        e.attr_ns(METADATA_NS, "IsDefaultEntityContainer") == Some("true");
                    if self.container.is_none() || default {
                        self.container = Some((ns.clone(), e));
                    }
                }
                _ => {}
            }
        }
    }

    fn raw_type(&mut self, e: &'a Element, qualified: &str) -> RawType {
        let mut t = RawType {
            base: e.attr("BaseType").map(|b| self.names.qualify(b)),
            key: e
                .child("Key")
                .map(|k| {
                    k.children_named("PropertyRef")
                        .filter_map(|r| r.attr("Name").map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            media: e.attr("HasStream") == Some("true")
                || e.attr_ns(METADATA_NS, "HasStream") == Some("true"),
            ..RawType::default()
        };
        for p in e.children_named("Property") {
            t.properties.push(self.property(p, qualified));
        }
        for n in e.children_named("NavigationProperty") {
            let name = n.attr("Name").unwrap_or_default().to_owned();
            t.navigation
                .push(match (n.attr("Relationship"), n.attr("Type")) {
                    (Some(relationship), _) => RawNavigation::V2 {
                        name,
                        relationship: self.names.qualify(relationship),
                        to_role: n.attr("ToRole").unwrap_or_default().to_owned(),
                    },
                    (None, ty) => {
                        let ty = self.names.qualify(ty.unwrap_or_default());
                        let (target, many) = match ty
                            .strip_prefix("Collection(")
                            .and_then(|r| r.strip_suffix(')'))
                        {
                            Some(inner) => (inner.to_owned(), true),
                            None => (ty, false),
                        };
                        RawNavigation::V4(Navigation { name, target, many })
                    }
                });
        }
        t
    }

    /// A property or parameter; inline annotations are kept for [`Self::annotations`].
    fn property(&mut self, p: &'a Element, parent: &str) -> Property {
        let name = p.attr("Name").unwrap_or_default().to_owned();
        let number = |a: &str| p.attr(a).and_then(|v| v.parse::<u32>().ok());
        let mut annotations = Vec::new();
        for attr in &p.attrs {
            if attr.ns == SAP_NS && SAP_PROPERTY_ATTRIBUTES.contains(&attr.name.as_str()) {
                annotations.push((attr.name.clone(), attr.value.clone()));
            }
        }
        if p.attr("ConcurrencyMode") == Some("Fixed") {
            annotations.push(("etag".to_owned(), "true".to_owned()));
        }
        for a in p.children_named("Annotation") {
            self.annotations.push((format!("{parent}/{name}"), a));
        }
        Property {
            name,
            r#type: self.names.qualify(p.attr("Type").unwrap_or("Edm.String")),
            nullable: p.attr("Nullable") != Some("false"),
            max_length: number("MaxLength"),
            precision: number("Precision"),
            scale: p.attr("Scale").map(str::to_owned),
            annotations,
        }
    }

    /// Entity and complex types with their base types applied, V2 navigation resolved.
    fn types(&self, model: &mut Model) {
        for (name, _) in &self.entity_types {
            let t = self.flatten(name, &self.entity_types, 0);
            model.entity_types.push(EntityType {
                name: name.clone(),
                key: t.key,
                properties: t.properties,
                navigation: t
                    .navigation
                    .into_iter()
                    .map(|n| self.navigation(n))
                    .collect(),
                media: t.media,
            });
        }
        for (name, _) in &self.complex_types {
            let t = self.flatten(name, &self.complex_types, 0);
            model.complex_types.push(ComplexType {
                name: name.clone(),
                properties: t.properties,
            });
        }
    }

    /// Whether type `ty` is `base` or derives from it.
    fn derives(&self, ty: &str, base: &str) -> bool {
        let mut current = ty.to_owned();
        for _ in 0..16 {
            if current == base {
                return true;
            }
            let next = self
                .entity_types
                .iter()
                .chain(&self.complex_types)
                .find(|(n, _)| *n == current)
                .and_then(|(_, t)| t.base.clone());
            match next {
                Some(b) => current = b,
                None => return false,
            }
        }
        false
    }

    /// `name` with everything inherited from its base types (base first).
    fn flatten(&self, name: &str, all: &[(String, RawType)], depth: usize) -> RawType {
        let Some((_, t)) = all.iter().find(|(n, _)| n == name) else {
            return RawType::default();
        };
        let mut t = t.clone();
        if let Some(base) = t.base.clone().filter(|_| depth < 16) {
            let mut b = self.flatten(&base, all, depth + 1);
            if t.key.is_empty() {
                t.key = std::mem::take(&mut b.key);
            }
            b.properties.append(&mut t.properties);
            t.properties = b.properties;
            b.navigation.append(&mut t.navigation);
            t.navigation = b.navigation;
            t.media |= b.media;
        }
        t
    }

    fn navigation(&self, n: RawNavigation) -> Navigation {
        match n {
            RawNavigation::V4(n) => n,
            RawNavigation::V2 {
                name,
                relationship,
                to_role,
            } => {
                let end = self
                    .associations
                    .get(&relationship)
                    .and_then(|ends| ends.iter().find(|(role, _, _)| *role == to_role));
                Navigation {
                    name,
                    target: end.map(|(_, t, _)| t.clone()).unwrap_or_default(),
                    many: end.is_some_and(|(_, _, m)| m == "*"),
                }
            }
        }
    }

    fn entity_container(&mut self, namespace: &str, container: &'a Element, out: &mut Parsed) {
        let qualified_container =
            format!("{namespace}.{}", container.attr("Name").unwrap_or_default());
        let mut imported: Vec<(String, String)> = Vec::new(); // operation → import name
        for e in &container.children {
            let Some(name) = e.attr("Name") else { continue };
            for a in e.children_named("Annotation") {
                self.annotations
                    .push((format!("{qualified_container}/{name}"), a));
            }
            match e.name.as_str() {
                "EntitySet" => {
                    let flag = |a: &str| e.attr_ns(SAP_NS, a) != Some("false");
                    out.model.entity_sets.push(EntitySet {
                        name: name.to_owned(),
                        entity_type: self.names.qualify(e.attr("EntityType").unwrap_or_default()),
                        insertable: flag("creatable"),
                        updatable: flag("updatable"),
                        deletable: flag("deletable"),
                        draft: false,
                    });
                }
                "Singleton" => out.singletons.push(name.to_owned()),
                "FunctionImport" if self.version() == Version::V2 => {
                    let method = e
                        .attr_ns(METADATA_NS, "HttpMethod")
                        .unwrap_or("GET")
                        .to_ascii_uppercase();
                    let kind = if method == "GET" {
                        OperationKind::Function
                    } else {
                        OperationKind::Action
                    };
                    if method != "GET" && method != "POST" {
                        out.other_methods.push((name.to_owned(), method));
                    }
                    let parameters = e
                        .children_named("Parameter")
                        .map(|p| self.property(p, &format!("{qualified_container}/{name}")))
                        .collect();
                    out.model.operations.push(Operation {
                        name: name.to_owned(),
                        kind,
                        bound_to: None,
                        parameters,
                        returns: e.attr("ReturnType").map(|r| self.names.qualify(r)),
                    });
                }
                "FunctionImport" => {
                    if let Some(f) = e.attr("Function") {
                        imported.push((self.names.qualify(f), name.to_owned()));
                    }
                }
                "ActionImport" => {
                    if let Some(a) = e.attr("Action") {
                        imported.push((self.names.qualify(a), name.to_owned()));
                    }
                }
                _ => {}
            }
        }

        let operations = std::mem::take(&mut self.operations);
        let mut unbound_done: Vec<String> = Vec::new();
        for (qualified, kind, e) in &operations {
            let simple = qualified.rsplit('.').next().unwrap_or(qualified).to_owned();
            let bound = e.attr("IsBound") == Some("true");
            let mut params: Vec<Property> = e
                .children_named("Parameter")
                .map(|p| self.property(p, qualified))
                .collect();
            let returns = e
                .child("ReturnType")
                .and_then(|r| r.attr("Type"))
                .map(|t| self.names.qualify(t));
            let (name, bound_to) = if bound {
                if params.is_empty() {
                    continue;
                }
                let binding = params.remove(0);
                (simple, Some(binding.r#type))
            } else {
                let Some((_, import)) = imported.iter().find(|(op, _)| op == qualified) else {
                    if !out.without_import.contains(&simple) {
                        out.without_import.push(simple);
                    }
                    continue;
                };
                // One request per import, also for overloaded functions.
                if unbound_done.contains(import) {
                    continue;
                }
                unbound_done.push(import.clone());
                (import.clone(), None)
            };
            out.model.operations.push(Operation {
                name,
                kind: *kind,
                bound_to,
                parameters: params,
                returns,
            });
        }
        self.operations = operations;
    }

    fn annotations(&self, namespace: &str, container: &Element, model: &mut Model) {
        let container_name = format!("{namespace}.{}", container.attr("Name").unwrap_or_default());
        for (target, group) in &self.annotations {
            let list: Vec<&Element> = if group.name == "Annotations" {
                group.children_named("Annotation").collect()
            } else {
                vec![group]
            };
            let (head, member) = match target.split_once('/') {
                Some((h, m)) => (self.names.qualify(h), Some(m)),
                None => (self.names.qualify(target), None),
            };
            for a in list {
                let Some(term) = a.attr("Term").map(|t| self.term(t)) else {
                    continue;
                };
                // An annotation with a qualifier is for one context only (e.g. one UI).
                if a.attr("Qualifier").is_some() {
                    continue;
                }
                match member {
                    Some(set) if head == container_name => {
                        set_annotation(model, set, &term, a);
                    }
                    Some(property) => {
                        let property = property.split('/').next().unwrap_or(property);
                        // Also on the types that inherit the property.
                        let types: Vec<&String> = self
                            .entity_types
                            .iter()
                            .chain(&self.complex_types)
                            .map(|(n, _)| n)
                            .filter(|n| self.derives(n, &head))
                            .collect();
                        for ty in types {
                            if let Some(p) = find_property(model, ty, property) {
                                property_annotation(p, &term, a);
                            }
                        }
                    }
                    None => {}
                }
            }
        }
    }

    /// A term with its vocabulary alias replaced (`Common.Label` →
    /// `com.sap.vocabularies.Common.v1.Label`); the usual aliases also when undeclared.
    fn term(&self, term: &str) -> String {
        let q = self.names.qualify(term);
        if q != term {
            return q;
        }
        match term.split_once('.') {
            Some(("Common", t)) => format!("{COMMON}{t}"),
            Some(("Core", t)) => format!("{CORE}{t}"),
            Some(("Capabilities", t)) => format!("{CAPABILITIES}{t}"),
            _ => q,
        }
    }
}

fn find_property<'m>(model: &'m mut Model, ty: &str, name: &str) -> Option<&'m mut Property> {
    if let Some(t) = model.entity_types.iter_mut().find(|t| t.name == ty) {
        return t.properties.iter_mut().find(|p| p.name == name);
    }
    model
        .complex_types
        .iter_mut()
        .find(|t| t.name == ty)?
        .properties
        .iter_mut()
        .find(|p| p.name == name)
}

/// Sets (replaces) an annotation of a property.
fn annotate(p: &mut Property, name: &str, value: &str) {
    match p.annotations.iter_mut().find(|(n, _)| n == name) {
        Some(a) => a.1 = value.to_owned(),
        None => p.annotations.push((name.to_owned(), value.to_owned())),
    }
}

fn property_annotation(p: &mut Property, term: &str, a: &Element) {
    if term == format!("{COMMON}Label") {
        if let Some(label) = string_value(a) {
            annotate(p, "label", &label);
        }
        return;
    }
    let name = match term.strip_prefix(CORE) {
        Some("Computed") => "computed",
        Some("Immutable") => "immutable",
        Some("OptimisticConcurrency") => "etag",
        _ => return,
    };
    if bool_value(a).unwrap_or(true) {
        annotate(p, name, "true");
    }
}

fn set_annotation(model: &mut Model, set: &str, term: &str, a: &Element) {
    let Some(index) = model.entity_sets.iter().position(|s| s.name == set) else {
        return;
    };
    let entity_type = model.entity_sets[index].entity_type.clone();
    let mark = |model: &mut Model, paths: Vec<String>, name: &str, value: &str| {
        for path in paths {
            if let Some(p) = find_property(model, &entity_type, &path) {
                annotate(p, name, value);
            }
        }
    };
    let s = &mut model.entity_sets[index];
    if term == format!("{COMMON}DraftRoot") {
        s.draft = true;
        return;
    }
    let Some(capability) = term.strip_prefix(CAPABILITIES) else {
        if term == format!("{CORE}OptimisticConcurrency") {
            mark(model, paths(a), "etag", "true");
        }
        return;
    };
    let record = a.child("Record");
    let value = |property: &str| record.and_then(|r| record_value(r, property));
    match capability {
        "InsertRestrictions" => {
            if let Some(v) = value("Insertable").and_then(bool_value) {
                s.insertable = v;
            }
        }
        "UpdateRestrictions" => {
            if let Some(v) = value("Updatable").and_then(bool_value) {
                s.updatable = v;
            }
        }
        "DeleteRestrictions" => {
            if let Some(v) = value("Deletable").and_then(bool_value) {
                s.deletable = v;
            }
        }
        "FilterRestrictions" => {
            if let Some(v) = value("NonFilterableProperties") {
                mark(model, paths(v), "filterable", "false");
            }
        }
        "SortRestrictions" => {
            if let Some(v) = value("NonSortableProperties") {
                mark(model, paths(v), "sortable", "false");
            }
        }
        _ => {}
    }
}

/// The `PropertyValue` of a record.
fn record_value<'e>(record: &'e Element, property: &str) -> Option<&'e Element> {
    record
        .children_named("PropertyValue")
        .find(|v| v.attr("Property") == Some(property))
}

/// `Bool="true"` or `<Bool>true</Bool>`; `None` when the element states no boolean.
fn bool_value(e: &Element) -> Option<bool> {
    let text = e
        .attr("Bool")
        .map(str::to_owned)
        .or_else(|| e.child("Bool").map(|b| b.text.trim().to_owned()))?;
    Some(text == "true")
}

/// `String="..."` or `<String>...</String>`.
fn string_value(e: &Element) -> Option<String> {
    e.attr("String")
        .map(str::to_owned)
        .or_else(|| e.child("String").map(|s| s.text.clone()))
}

/// Property paths of `<Collection><PropertyPath>A</PropertyPath>...` (also `PropertyPath="A"`
/// for a single one).
fn paths(e: &Element) -> Vec<String> {
    if let Some(p) = e.attr("PropertyPath") {
        return vec![p.to_owned()];
    }
    e.child("Collection")
        .map(|c| {
            c.children_named("PropertyPath")
                .map(|p| p.text.trim().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0"?>
<edmx:Edmx Version="4.0" xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx">
  <edmx:DataServices>
    <Schema Namespace="org.example" Alias="Ex" xmlns="http://docs.oasis-open.org/odata/ns/edm">
      <EntityType Name="Base" Abstract="true">
        <Key><PropertyRef Name="ID"/></Key>
        <Property Name="ID" Type="Edm.Int64" Nullable="false"/>
        <Property Name="Name" Type="Edm.String" MaxLength="max">
          <Annotation Term="Core.Immutable"/>
        </Property>
      </EntityType>
      <EntityType Name="Doc" BaseType="Ex.Base" HasStream="true">
        <Property Name="Size" Type="Edm.Int32"/>
        <Property Name="Tags" Type="Collection(Edm.String)"/>
        <NavigationProperty Name="parts" Type="Collection(Ex.Doc)"/>
      </EntityType>
      <Function Name="find"><Parameter Name="q" Type="Edm.String"/><ReturnType Type="Collection(Ex.Doc)"/></Function>
      <Function Name="find"><ReturnType Type="Collection(Ex.Doc)"/></Function>
      <Action Name="orphan"/>
      <Action Name="purge" IsBound="true"><Parameter Name="in" Type="Collection(Ex.Doc)"/></Action>
      <EntityContainer Name="C">
        <EntitySet Name="Docs" EntityType="Ex.Doc">
          <Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions">
            <Record><PropertyValue Property="Insertable"><Bool>false</Bool></PropertyValue></Record>
          </Annotation>
        </EntitySet>
        <Singleton Name="Me" Type="Ex.Doc"/>
        <FunctionImport Name="find" Function="Ex.find"/>
      </EntityContainer>
      <Annotations Target="Ex.C/Docs">
        <Annotation Term="Capabilities.FilterRestrictions">
          <Record>
            <PropertyValue Property="NonFilterableProperties">
              <Collection><PropertyPath>Size</PropertyPath></Collection>
            </PropertyValue>
          </Record>
        </Annotation>
        <Annotation Term="Capabilities.UpdateRestrictions" Qualifier="UI">
          <Record><PropertyValue Property="Updatable" Bool="false"/></Record>
        </Annotation>
      </Annotations>
      <Annotations Target="org.example.Doc/Size">
        <Annotation Term="Common.Label"><String>Size &amp; weight</String></Annotation>
      </Annotations>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;

    #[test]
    fn base_types_aliases_imports_and_inline_annotations() {
        let parsed = parse(DOC, "https://h/svc").unwrap();
        let m = &parsed.model;
        assert_eq!(m.service_root, "https://h/svc/");
        let doc = m.entity_type("org.example.Doc").unwrap();
        assert_eq!(doc.key, vec!["ID"]);
        assert!(doc.media);
        let names: Vec<_> = doc.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["ID", "Name", "Size", "Tags"]);
        assert_eq!(doc.properties[1].max_length, None);
        assert_eq!(
            doc.properties[1].annotations,
            vec![("immutable".into(), "true".into())]
        );
        assert_eq!(
            doc.properties[2].annotations,
            vec![
                ("filterable".into(), "false".into()),
                ("label".into(), "Size & weight".into())
            ]
        );
        assert_eq!(doc.properties[3].r#type, "Collection(Edm.String)");
        assert_eq!(doc.navigation[0].target, "org.example.Doc");
        assert!(doc.navigation[0].many);
        let set = m.entity_set("Docs").unwrap();
        assert!(!set.insertable);
        assert!(set.updatable, "qualified annotations are ignored");
        assert_eq!(parsed.singletons, vec!["Me"]);
        assert_eq!(parsed.without_import, vec!["orphan"]);
        let ops: Vec<_> = m
            .operations
            .iter()
            .map(|o| (o.name.as_str(), o.bound_to.as_deref(), o.parameters.len()))
            .collect();
        assert_eq!(
            ops,
            vec![
                ("find", None, 1),
                ("purge", Some("Collection(org.example.Doc)"), 0)
            ]
        );
    }

    #[test]
    fn not_edmx() {
        assert!(parse("<html/>", "").unwrap_err().contains("not an EDMX"));
        assert!(
            parse("<edmx:Edmx xmlns:edmx='x' Version='9.0'/>", "")
                .unwrap_err()
                .contains("unsupported")
        );
        assert!(
            parse(
                "<Edmx Version='4.0'><DataServices><Schema/></DataServices></Edmx>",
                ""
            )
            .unwrap_err()
            .contains("EntityContainer")
        );
    }

    #[test]
    fn sap_self_link_gives_the_root() {
        let doc = r#"<edmx:Edmx Version="1.0" xmlns:edmx="http://schemas.microsoft.com/ado/2007/06/edmx">
  <edmx:DataServices><Schema Namespace="S" xmlns="http://schemas.microsoft.com/ado/2008/09/edm">
    <EntityContainer Name="C"/>
    <atom:link rel="self" href="https://gw.example.com/sap/opu/odata/sap/S/$metadata" xmlns:atom="http://www.w3.org/2005/Atom"/>
  </Schema></edmx:DataServices></edmx:Edmx>"#;
        let parsed = parse(doc, "").unwrap();
        assert_eq!(
            parsed.self_root.as_deref(),
            Some("https://gw.example.com/sap/opu/odata/sap/S/")
        );
        assert_eq!(parsed.model.version, Version::V2);
    }
}
