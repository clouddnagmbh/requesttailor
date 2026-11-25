// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! XML Schema, as far as sample messages need it: the global elements and types of every
//! schema the WSDL and its files carry, and [`Sampler`], which writes an example instance of an
//! element or type (required and optional parts, the first alternative of a choice, the first
//! enumeration value, a plain value per built-in type).

use std::collections::BTreeMap;

use crate::node::{Child, Node};
use crate::xml::Element;

pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";

/// Nesting of sample content stops here (a type that contains itself stops earlier).
const MAX_DEPTH: usize = 12;

/// A global definition and the schema settings it was found under.
#[derive(Debug, Clone)]
struct Def {
    el: Element,
    /// `targetNamespace` of the schema.
    tns: String,
    /// `elementFormDefault="qualified"`.
    qualified: bool,
}

type Key = (String, String);

#[derive(Debug, Default)]
pub struct Schema {
    elements: BTreeMap<Key, Def>,
    complex: BTreeMap<Key, Def>,
    simple: BTreeMap<Key, Def>,
    groups: BTreeMap<Key, Def>,
    attribute_groups: BTreeMap<Key, Def>,
    /// `schemaLocation`s and `namespace`s of `xs:import`/`xs:include`, for the importer.
    pub references: Vec<String>,
}

impl Schema {
    /// Adds every global definition of an `xs:schema` element.
    pub fn add(&mut self, schema: &Element) {
        let tns = schema.attr("targetNamespace").unwrap_or("").to_owned();
        let qualified = schema.attr("elementFormDefault") == Some("qualified");
        for child in &schema.children {
            if child.ns != XSD_NS {
                continue;
            }
            let table = match child.name.as_str() {
                "element" => &mut self.elements,
                "complexType" => &mut self.complex,
                "simpleType" => &mut self.simple,
                "group" => &mut self.groups,
                "attributeGroup" => &mut self.attribute_groups,
                "import" | "include" | "redefine" => {
                    if let Some(loc) = child.attr("schemaLocation") {
                        self.references.push(loc.to_owned());
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(name) = child.attr("name") {
                table.insert(
                    (tns.clone(), name.to_owned()),
                    Def {
                        el: child.clone(),
                        tns: tns.clone(),
                        qualified,
                    },
                );
            }
        }
    }

    pub fn has_element(&self, ns: &str, name: &str) -> bool {
        self.elements
            .contains_key(&(ns.to_owned(), name.to_owned()))
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.complex.is_empty() && self.simple.is_empty()
    }
}

/// Something the sample could not represent faithfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub detail: String,
}

pub struct Sampler<'a> {
    schema: &'a Schema,
    /// What was left out or guessed, without repeats.
    pub gaps: Vec<Gap>,
}

impl<'a> Sampler<'a> {
    pub fn new(schema: &'a Schema) -> Self {
        Sampler {
            schema,
            gaps: Vec::new(),
        }
    }

    fn gap(&mut self, detail: String) {
        if !self.gaps.iter().any(|g| g.detail == detail) {
            self.gaps.push(Gap { detail });
        }
    }

    /// A sample of the global element `ns:name`. An unknown element is an empty one, noted.
    pub fn element(&mut self, ns: &str, name: &str) -> Node {
        let key = (ns.to_owned(), name.to_owned());
        match self.schema.elements.get(&key) {
            Some(def) => {
                let def = def.clone();
                let mut stack = Vec::new();
                let mut out = Vec::new();
                self.declared(&def.el, &def, true, &mut stack, &mut out);
                match out.into_iter().find_map(|c| match c {
                    Child::Node(n) => Some(n),
                    Child::Comment(_) => None,
                }) {
                    Some(n) => n,
                    None => Node::new(ns, name),
                }
            }
            None => {
                self.gap(format!(
                    "element {{{ns}}}{name} is not defined in any schema"
                ));
                Node::new(ns, name)
            }
        }
    }

    /// A sample element named `name` (no namespace) of the type `ns:local` (an RPC part).
    pub fn typed(&mut self, name: &str, ns: &str, local: &str) -> Node {
        let mut node = Node::new("", name);
        let mut stack = Vec::new();
        let ctx = Def {
            el: Element::default(),
            tns: ns.to_owned(),
            qualified: false,
        };
        self.fill_named_type(&mut node, ns, local, &ctx, &mut stack);
        node
    }

    /// `xs:element` (declaration or `ref`) as sample children (comments and the element,
    /// once per repetition).
    fn declared(
        &mut self,
        decl: &Element,
        ctx: &Def,
        global: bool,
        stack: &mut Vec<Key>,
        out: &mut Vec<Child>,
    ) {
        if stack.len() >= MAX_DEPTH {
            self.gap("content nested deeper than 12 levels was cut".into());
            return;
        }
        let (def, ctx_here): (Element, Def) = match decl.attr("ref") {
            Some(r) if !global => match decl.resolve_qname(r) {
                Some((ns, local)) => {
                    match self.schema.elements.get(&(ns.clone(), local.to_owned())) {
                        Some(d) => (d.el.clone(), d.clone()),
                        None => {
                            self.gap(format!("element reference {{{ns}}}{local} is not defined"));
                            return;
                        }
                    }
                }
                None => return,
            },
            _ => (decl.clone(), ctx.clone()),
        };
        let Some(name) = def.attr("name") else { return };
        let is_global_decl = global || decl.attr("ref").is_some();
        let qualified = is_global_decl
            || match def.attr("form") {
                Some(f) => f == "qualified",
                None => ctx_here.qualified,
            };
        let ns = if qualified {
            ctx_here.tns.clone()
        } else {
            String::new()
        };
        let min = decl.attr("minOccurs").unwrap_or("1");
        let max = decl.attr("maxOccurs").unwrap_or("1");
        if max == "0" {
            return;
        }
        let optional = min == "0";
        let many = max == "unbounded" || max.parse::<u32>().is_ok_and(|n| n > 1);
        if optional {
            out.push(Child::Comment("Optional:".into()));
        }
        if many {
            out.push(Child::Comment(if optional {
                "Zero or more repetitions:".into()
            } else {
                "1 or more repetitions:".into()
            }));
        }
        let mut node = Node::new(&ns, name);
        match def.attr("type") {
            Some(t) => match def.resolve_qname(t) {
                Some((tns, local)) => {
                    self.fill_named_type(&mut node, &tns, local, &ctx_here, stack)
                }
                None => {
                    self.gap(format!("type {t} of element {name}: prefix not declared"));
                    node.text = Some("string".into());
                }
            },
            None => {
                if let Some(ct) = def
                    .children
                    .iter()
                    .find(|c| c.ns == XSD_NS && c.name == "complexType")
                {
                    self.fill_complex(&mut node, ct, &ctx_here, stack);
                } else if let Some(st) = def
                    .children
                    .iter()
                    .find(|c| c.ns == XSD_NS && c.name == "simpleType")
                {
                    node.text = Some(self.simple_value(st, &ctx_here, 0));
                } else if def.attr("fixed").is_some() || def.attr("default").is_some() {
                    node.text = def.attr("fixed").or(def.attr("default")).map(str::to_owned);
                } else {
                    node.text = Some(String::new());
                }
            }
        }
        if let Some(v) = def.attr("fixed").or(def.attr("default")) {
            if node.children.is_empty() {
                node.text = Some(v.to_owned());
            }
        }
        out.push(Child::Node(node));
    }

    fn fill_named_type(
        &mut self,
        node: &mut Node,
        ns: &str,
        local: &str,
        ctx: &Def,
        stack: &mut Vec<Key>,
    ) {
        if ns == XSD_NS {
            node.text = Some(builtin_sample(local).to_owned());
            return;
        }
        let key = (ns.to_owned(), local.to_owned());
        if let Some(def) = self.schema.complex.get(&key).cloned() {
            if stack.contains(&key) {
                self.gap(format!("recursive type {{{ns}}}{local} was cut"));
                node.children
                    .push(Child::Comment(format!("recursive type {local} omitted")));
                return;
            }
            stack.push(key);
            self.fill_complex(node, &def.el, &def, stack);
            stack.pop();
        } else if let Some(def) = self.schema.simple.get(&key).cloned() {
            node.text = Some(self.simple_value(&def.el, &def, 0));
        } else {
            let _ = ctx;
            self.gap(format!("type {{{ns}}}{local} is not defined in any schema"));
            node.text = Some("string".into());
        }
    }

    /// Attributes and content of an `xs:complexType`.
    fn fill_complex(&mut self, node: &mut Node, ct: &Element, ctx: &Def, stack: &mut Vec<Key>) {
        for child in &ct.children {
            if child.ns != XSD_NS {
                continue;
            }
            match child.name.as_str() {
                "sequence" | "all" | "choice" | "group" => {
                    self.particle(child, ctx, stack, &mut node.children)
                }
                "attribute" | "attributeGroup" => self.attribute(child, ctx, node),
                "complexContent" | "simpleContent" => {
                    let simple = child.name == "simpleContent";
                    for derivation in child.children.iter().filter(|d| {
                        d.ns == XSD_NS && matches!(d.name.as_str(), "extension" | "restriction")
                    }) {
                        let extension = derivation.name == "extension";
                        if let Some(base) = derivation.attr("base") {
                            if let Some((ns, local)) = derivation.resolve_qname(base) {
                                if simple {
                                    node.text = Some(self.base_text(&ns, local, ctx));
                                } else if extension {
                                    self.fill_named_type(node, &ns, local, ctx, stack);
                                }
                            }
                        }
                        for part in &derivation.children {
                            if part.ns != XSD_NS {
                                continue;
                            }
                            match part.name.as_str() {
                                "sequence" | "all" | "choice" | "group" => {
                                    self.particle(part, ctx, stack, &mut node.children)
                                }
                                "attribute" | "attributeGroup" => self.attribute(part, ctx, node),
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if node.children.is_empty() && node.text.is_none() {
            node.text = None;
        }
    }

    fn base_text(&mut self, ns: &str, local: &str, ctx: &Def) -> String {
        if ns == XSD_NS {
            return builtin_sample(local).to_owned();
        }
        match self
            .schema
            .simple
            .get(&(ns.to_owned(), local.to_owned()))
            .cloned()
        {
            Some(def) => self.simple_value(&def.el, &def, 0),
            None => {
                let _ = ctx;
                "string".into()
            }
        }
    }

    fn particle(&mut self, p: &Element, ctx: &Def, stack: &mut Vec<Key>, out: &mut Vec<Child>) {
        match p.name.as_str() {
            "sequence" | "all" => {
                for child in p.children.iter().filter(|c| c.ns == XSD_NS) {
                    self.particle(child, ctx, stack, out);
                }
            }
            "choice" => {
                let options: Vec<&Element> = p
                    .children
                    .iter()
                    .filter(|c| c.ns == XSD_NS && c.name != "annotation")
                    .collect();
                if options.len() > 1 {
                    out.push(Child::Comment(format!(
                        "You have a CHOICE of the next {} items at this level",
                        options.len()
                    )));
                }
                if let Some(first) = options.first() {
                    self.particle(first, ctx, stack, out);
                }
            }
            "element" => self.declared(p, ctx, false, stack, out),
            "group" => {
                if let Some(r) = p.attr("ref") {
                    if let Some((ns, local)) = p.resolve_qname(r) {
                        if let Some(def) = self
                            .schema
                            .groups
                            .get(&(ns.clone(), local.to_owned()))
                            .cloned()
                        {
                            for child in def.el.children.iter().filter(|c| c.ns == XSD_NS) {
                                self.particle(child, &def, stack, out);
                            }
                        } else {
                            self.gap(format!("group {{{ns}}}{local} is not defined"));
                        }
                    }
                }
            }
            "any" => {
                out.push(Child::Comment("any element is allowed here".into()));
                self.gap("xs:any: no sample content for wildcard elements".into());
            }
            _ => {}
        }
    }

    /// Required attributes only (optional ones are left out of a sample).
    fn attribute(&mut self, a: &Element, ctx: &Def, node: &mut Node) {
        if a.name == "attributeGroup" {
            if let Some(r) = a.attr("ref") {
                if let Some((ns, local)) = a.resolve_qname(r) {
                    if let Some(def) = self
                        .schema
                        .attribute_groups
                        .get(&(ns, local.to_owned()))
                        .cloned()
                    {
                        for child in def
                            .el
                            .children
                            .iter()
                            .filter(|c| c.ns == XSD_NS && c.name == "attribute")
                        {
                            self.attribute(child, &def, node);
                        }
                    }
                }
            }
            return;
        }
        if a.attr("use") != Some("required") {
            return;
        }
        let Some(name) = a.attr("name") else { return };
        let value = if let Some(v) = a.attr("fixed").or(a.attr("default")) {
            v.to_owned()
        } else if let Some(t) = a.attr("type") {
            match a.resolve_qname(t) {
                Some((ns, local)) => self.base_text(&ns, local, ctx),
                None => "string".into(),
            }
        } else if let Some(st) = a
            .children
            .iter()
            .find(|c| c.ns == XSD_NS && c.name == "simpleType")
        {
            self.simple_value(st, ctx, 0)
        } else {
            "string".into()
        };
        node.attrs.push((String::new(), name.to_owned(), value));
    }

    /// A value of an `xs:simpleType`: the first enumeration value, a bound, or the base's.
    #[allow(clippy::only_used_in_recursion)] // `ctx` is kept for facets that need the schema's settings
    fn simple_value(&mut self, st: &Element, ctx: &Def, depth: usize) -> String {
        if depth > 8 {
            return "string".into();
        }
        for child in st.children.iter().filter(|c| c.ns == XSD_NS) {
            match child.name.as_str() {
                "restriction" => {
                    if let Some(e) = child.children.iter().find(|c| c.name == "enumeration") {
                        return e.attr("value").unwrap_or("").to_owned();
                    }
                    let bound = |name: &str| {
                        child
                            .children
                            .iter()
                            .find(|c| c.name == name)
                            .and_then(|c| c.attr("value"))
                            .and_then(|v| v.parse::<i64>().ok())
                    };
                    if let Some(v) = bound("minInclusive") {
                        return v.to_string();
                    }
                    if let Some(v) = bound("minExclusive") {
                        return (v + 1).to_string();
                    }
                    if let Some(base) = child.attr("base") {
                        if let Some((ns, local)) = child.resolve_qname(base) {
                            if ns == XSD_NS {
                                return builtin_sample(local).to_owned();
                            }
                            if let Some(def) =
                                self.schema.simple.get(&(ns, local.to_owned())).cloned()
                            {
                                return self.simple_value(&def.el, &def, depth + 1);
                            }
                        }
                    }
                    if let Some(inner) = child.children.iter().find(|c| c.name == "simpleType") {
                        return self.simple_value(inner, ctx, depth + 1);
                    }
                }
                "list" | "union" => return "string".into(),
                _ => {}
            }
        }
        "string".into()
    }
}

/// A plain value for a built-in type (by local name).
pub fn builtin_sample(local: &str) -> &'static str {
    match local {
        "int" | "integer" | "long" | "short" | "byte" | "nonNegativeInteger" | "unsignedInt"
        | "unsignedLong" | "unsignedShort" | "unsignedByte" | "nonPositiveInteger"
        | "negativeInteger" => "0",
        "positiveInteger" => "1",
        "decimal" | "float" | "double" => "0.0",
        "boolean" => "false",
        "date" => "2026-01-01",
        "dateTime" => "2026-01-01T00:00:00Z",
        "time" => "00:00:00",
        "duration" => "P1D",
        "gYear" => "2026",
        "gYearMonth" => "2026-01",
        "base64Binary" => "AA==",
        "hexBinary" => "00",
        "anyURI" => "https://example.com/",
        "anyType" | "anySimpleType" => "",
        _ => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn schema(text: &str) -> Schema {
        let mut s = Schema::default();
        s.add(&xml::parse(text).unwrap());
        s
    }

    #[test]
    fn builtin_values() {
        assert_eq!(builtin_sample("int"), "0");
        assert_eq!(builtin_sample("dateTime"), "2026-01-01T00:00:00Z");
        assert_eq!(builtin_sample("whatever"), "string");
    }

    #[test]
    fn recursive_types_are_cut_and_reported() {
        let s = schema(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t"
                targetNamespace="urn:t" elementFormDefault="qualified">
              <xs:element name="Tree" type="t:Node"/>
              <xs:complexType name="Node"><xs:sequence>
                <xs:element name="label" type="xs:string"/>
                <xs:element name="child" type="t:Node" minOccurs="0"/>
              </xs:sequence></xs:complexType></xs:schema>"#,
        );
        let mut sampler = Sampler::new(&s);
        let node = sampler.element("urn:t", "Tree");
        assert_eq!(node.name, "Tree");
        assert!(sampler.gaps.iter().any(|g| g.detail.contains("recursive")));
    }
}
