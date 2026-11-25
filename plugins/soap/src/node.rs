// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A tiny XML tree for what the plugin writes (sample messages), and its serializer.
//! Namespaces get prefixes, all declared on the root element of the message.

/// A child of a [`Node`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Node(Node),
    Comment(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    /// Namespace URI, `""` for none.
    pub ns: String,
    pub name: String,
    /// `(namespace, name, value)`.
    pub attrs: Vec<(String, String, String)>,
    pub children: Vec<Child>,
    pub text: Option<String>,
}

impl Node {
    pub fn new(ns: &str, name: &str) -> Self {
        Node {
            ns: ns.to_owned(),
            name: name.to_owned(),
            ..Node::default()
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

/// Prefixes for namespaces, in the order they were first seen.
#[derive(Debug, Default)]
pub struct Prefixes {
    bound: Vec<(String, String)>,
}

impl Prefixes {
    /// Binds `ns` to `preferred` if that prefix is free, else to `nsN`.
    pub fn bind(&mut self, ns: &str, preferred: &str) -> String {
        if let Some((_, p)) = self.bound.iter().find(|(n, _)| n == ns) {
            return p.clone();
        }
        let mut prefix = preferred.to_owned();
        let mut n = 1;
        while self.bound.iter().any(|(_, p)| *p == prefix) {
            prefix = format!("ns{n}");
            n += 1;
        }
        self.bound.push((ns.to_owned(), prefix.clone()));
        prefix
    }

    /// Binds every namespace used in `nodes` (`first` gets the preferred prefix `tns`).
    pub fn collect(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.collect_node(node);
        }
    }

    fn collect_node(&mut self, node: &Node) {
        if !node.ns.is_empty() {
            self.bind(&node.ns, "tns");
        }
        for (ns, _, _) in &node.attrs {
            if !ns.is_empty() {
                self.bind(ns, "ns");
            }
        }
        for child in &node.children {
            if let Child::Node(n) = child {
                self.collect_node(n);
            }
        }
    }

    pub fn get(&self, ns: &str) -> Option<&str> {
        self.bound
            .iter()
            .find(|(n, _)| n == ns)
            .map(|(_, p)| p.as_str())
    }

    pub fn declarations(&self) -> impl Iterator<Item = (&str, &str)> {
        self.bound.iter().map(|(n, p)| (p.as_str(), n.as_str()))
    }
}

/// Escapes text content.
pub fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escapes an attribute value (double-quoted).
pub fn escape_attr(text: &str) -> String {
    escape_text(text).replace('"', "&quot;")
}

fn qname(prefixes: &Prefixes, ns: &str, name: &str) -> String {
    match prefixes.get(ns) {
        Some(p) if !ns.is_empty() => format!("{p}:{name}"),
        _ => name.to_owned(),
    }
}

/// Writes `node` with `depth` levels of indentation (two spaces each).
pub fn render(node: &Node, depth: usize, prefixes: &Prefixes, out: &mut String) {
    let pad = "  ".repeat(depth);
    out.push_str(&pad);
    out.push('<');
    let name = qname(prefixes, &node.ns, &node.name);
    out.push_str(&name);
    for (ns, attr, value) in &node.attrs {
        out.push(' ');
        out.push_str(&qname(prefixes, ns, attr));
        out.push_str("=\"");
        out.push_str(&escape_attr(value));
        out.push('"');
    }
    if node.children.is_empty() {
        match &node.text {
            Some(t) if !t.is_empty() => {
                out.push('>');
                out.push_str(&escape_text(t));
                out.push_str("</");
                out.push_str(&name);
                out.push_str(">\n");
            }
            _ => out.push_str("/>\n"),
        }
        return;
    }
    out.push_str(">\n");
    for child in &node.children {
        match child {
            Child::Node(n) => render(n, depth + 1, prefixes, out),
            Child::Comment(c) => {
                out.push_str(&pad);
                out.push_str("  <!--");
                out.push_str(&c.replace("--", "- -"));
                out.push_str("-->\n");
            }
        }
    }
    out.push_str(&pad);
    out.push_str("</");
    out.push_str(&name);
    out.push_str(">\n");
}
