// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! GraphQL schemas for the app (wave 8, strand B; `GraphQlApi.schema`).
//!
//! The standard introspection query is sent like the request itself (same URL, method,
//! headers, auth and variables; scripts, assertions and actions are left out), the answer
//! is printed as SDL ([`sdl`]) and kept per URL and environment in a [`SchemaCache`] until the
//! user refreshes. The shells send [`introspection_snapshot`] with their usual transport and
//! services, then call [`schema_from_report`].

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rt_core::model::{Body, Item};
use rt_core::send::{ReportBody, SendReport};
use serde_json::Value;

use crate::{AppError, SendInput, SendSnapshot, Workspace};

/// The introspection query of graphql-js (`getIntrospectionQuery` with descriptions,
/// `specifiedByURL`, repeatable directives; no deprecated arguments or input fields, which
/// older servers reject).
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery {
  __schema {
    description
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives {
      name
      description
      isRepeatable
      locations
      args { ...InputValue }
    }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  specifiedByURL
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
    deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name
    description
    isDeprecated
    deprecationReason
  }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType { kind name }
            }
          }
        }
      }
    }
  }
}
";

/// The cache key of the request at `path`: environment and resolved URL (secrets masked).
pub fn cache_key(ws: &Workspace, path: &str, input: &SendInput) -> Result<String, AppError> {
    let preview = ws.preview(path, input.clone())?;
    let url = preview
        .url
        .ok_or_else(|| AppError::Shell(preview.error.unwrap_or_else(|| "no URL".into())))?;
    Ok(format!(
        "{}\n{url}",
        input.environment.as_deref().unwrap_or("")
    ))
}

/// SDL by [`cache_key`]; the shells keep one per open collection.
#[derive(Debug, Clone, Default)]
pub struct SchemaCache {
    schemas: BTreeMap<String, String>,
}

impl SchemaCache {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.schemas.get(key).map(String::as_str)
    }

    pub fn put(&mut self, key: String, sdl: String) {
        self.schemas.insert(key, sdl);
    }

    pub fn clear(&mut self) {
        self.schemas.clear();
    }
}

/// The request at `path` with the introspection query as its body, ready to send.
pub fn introspection_snapshot(
    ws: &Workspace,
    path: &str,
    input: SendInput,
) -> Result<SendSnapshot, AppError> {
    let mut snapshot = ws.snapshot(path, input)?;
    fn find<'a>(items: &'a mut [Item], path: &str) -> Option<&'a mut rt_core::model::RequestSpec> {
        for item in items {
            match item {
                Item::Request(r) if r.path == path => return Some(&mut r.spec),
                Item::Folder(f) => {
                    if let Some(s) = find(&mut f.items, path) {
                        return Some(s);
                    }
                }
                Item::Request(_) => {}
            }
        }
        None
    }
    let spec = find(&mut snapshot.collection.items, path)
        .ok_or_else(|| AppError::RequestNotFound(path.to_owned()))?;
    if !spec.protocol.is_http() {
        return Err(AppError::Unsupported(format!(
            "{path} is not an HTTP or GraphQL request"
        )));
    }
    spec.body = Body::GraphQl {
        query: INTROSPECTION_QUERY.to_owned(),
        variables: String::new(),
    };
    if spec.method.eq_ignore_ascii_case("GET") {
        spec.method = "POST".into();
    }
    spec.scripts.clear();
    spec.assertions.clear();
    spec.actions.clear();
    Ok(snapshot)
}

/// The SDL of an introspection answer, or why there is none.
pub fn schema_from_report(report: &SendReport) -> Result<String, AppError> {
    let fail = |m: String| AppError::Shell(format!("GraphQL introspection failed: {m}"));
    let response = match (&report.response, &report.error) {
        (Some(r), _) => r,
        (None, Some(e)) => return Err(fail(error_text(e))),
        (None, None) => return Err(fail("no response".into())),
    };
    let ReportBody::Utf8(text) = &response.body else {
        return Err(fail(format!("HTTP {}, not JSON", response.status)));
    };
    let json: Value = serde_json::from_str(text)
        .map_err(|_| fail(format!("HTTP {}, the answer is not JSON", response.status)))?;
    if let Some(schema) = json.pointer("/data/__schema").filter(|s| s.is_object()) {
        return sdl(schema).map_err(fail);
    }
    let message = json
        .pointer("/errors/0/message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("HTTP {}, no __schema in the answer", response.status));
    Err(fail(message))
}

fn error_text(e: &rt_core::send::SendError) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| {
            v.get("message")
                .map(|m| m.to_string().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| format!("{e:?}"))
}

// ---------------------------------------------------------------------------------------------
// SDL printer

const BUILTIN_SCALARS: [&str; 5] = ["String", "Int", "Float", "Boolean", "ID"];
const BUILTIN_DIRECTIVES: [&str; 5] = ["skip", "include", "deprecated", "specifiedBy", "oneOf"];

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn list_of<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn type_ref(t: &Value) -> Result<String, String> {
    match str_of(t, "kind") {
        Some("NON_NULL") => Ok(format!(
            "{}!",
            type_ref(t.get("ofType").unwrap_or(&Value::Null))?
        )),
        Some("LIST") => Ok(format!(
            "[{}]",
            type_ref(t.get("ofType").unwrap_or(&Value::Null))?
        )),
        Some(_) => str_of(t, "name")
            .map(str::to_owned)
            .ok_or_else(|| "a type reference without a name".to_owned()),
        None => Err("a type reference without a kind".into()),
    }
}

fn description(out: &mut String, v: &Value, indent: &str) {
    let Some(d) = str_of(v, "description").filter(|d| !d.is_empty()) else {
        return;
    };
    if !d.contains('\n') && !d.contains('"') && !d.contains('\\') {
        let _ = writeln!(out, "{indent}\"{d}\"");
        return;
    }
    let _ = writeln!(out, "{indent}\"\"\"");
    for line in d.replace("\"\"\"", "\\\"\"\"").lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{indent}{line}");
        }
    }
    let _ = writeln!(out, "{indent}\"\"\"");
}

fn deprecated(v: &Value) -> String {
    if v.get("isDeprecated").and_then(Value::as_bool) != Some(true) {
        return String::new();
    }
    match str_of(v, "deprecationReason") {
        None | Some("No longer supported") => " @deprecated".into(),
        Some(r) => format!(
            " @deprecated(reason: {})",
            serde_json::to_string(r).unwrap_or_default()
        ),
    }
}

fn input_value(v: &Value) -> Result<String, String> {
    let mut s = format!(
        "{}: {}",
        str_of(v, "name").unwrap_or_default(),
        type_ref(v.get("type").unwrap_or(&Value::Null))?
    );
    if let Some(d) = str_of(v, "defaultValue") {
        let _ = write!(s, " = {d}");
    }
    s.push_str(&deprecated(v));
    Ok(s)
}

fn args(out: &mut String, list: &[Value], indent: &str) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    let described = list
        .iter()
        .any(|a| str_of(a, "description").is_some_and(|d| !d.is_empty()));
    if !described {
        let parts = list
            .iter()
            .map(input_value)
            .collect::<Result<Vec<_>, _>>()?;
        let _ = write!(out, "({})", parts.join(", "));
        return Ok(());
    }
    out.push_str("(\n");
    let inner = format!("{indent}  ");
    for a in list {
        description(out, a, &inner);
        let _ = writeln!(out, "{inner}{}", input_value(a)?);
    }
    let _ = write!(out, "{indent})");
    Ok(())
}

fn implements(t: &Value) -> Result<String, String> {
    let names = list_of(t, "interfaces")
        .iter()
        .map(type_ref)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if names.is_empty() {
        String::new()
    } else {
        format!(" implements {}", names.join(" & "))
    })
}

fn named_type(out: &mut String, t: &Value) -> Result<(), String> {
    let name = str_of(t, "name").ok_or("a type without a name")?;
    description(out, t, "");
    match str_of(t, "kind").unwrap_or_default() {
        "SCALAR" => {
            let _ = write!(out, "scalar {name}");
            if let Some(url) = str_of(t, "specifiedByURL") {
                let _ = write!(
                    out,
                    " @specifiedBy(url: {})",
                    serde_json::to_string(url).unwrap_or_default()
                );
            }
            out.push('\n');
        }
        kind @ ("OBJECT" | "INTERFACE") => {
            let keyword = if kind == "OBJECT" {
                "type"
            } else {
                "interface"
            };
            let _ = write!(out, "{keyword} {name}{}", implements(t)?);
            let fields = list_of(t, "fields");
            if fields.is_empty() {
                out.push('\n');
                return Ok(());
            }
            out.push_str(" {\n");
            for f in fields {
                description(out, f, "  ");
                let _ = write!(out, "  {}", str_of(f, "name").unwrap_or_default());
                args(out, list_of(f, "args"), "  ")?;
                let _ = writeln!(
                    out,
                    ": {}{}",
                    type_ref(f.get("type").unwrap_or(&Value::Null))?,
                    deprecated(f)
                );
            }
            out.push_str("}\n");
        }
        "UNION" => {
            let members = list_of(t, "possibleTypes")
                .iter()
                .map(type_ref)
                .collect::<Result<Vec<_>, _>>()?;
            let _ = write!(out, "union {name}");
            if !members.is_empty() {
                let _ = write!(out, " = {}", members.join(" | "));
            }
            out.push('\n');
        }
        "ENUM" => {
            let _ = writeln!(out, "enum {name} {{");
            for v in list_of(t, "enumValues") {
                description(out, v, "  ");
                let _ = writeln!(
                    out,
                    "  {}{}",
                    str_of(v, "name").unwrap_or_default(),
                    deprecated(v)
                );
            }
            out.push_str("}\n");
        }
        "INPUT_OBJECT" => {
            let _ = writeln!(out, "input {name} {{");
            for f in list_of(t, "inputFields") {
                description(out, f, "  ");
                let _ = writeln!(out, "  {}", input_value(f)?);
            }
            out.push_str("}\n");
        }
        other => return Err(format!("type {name} of unknown kind {other}")),
    }
    Ok(())
}

/// The SDL of an introspection `__schema` object: a `schema` block only if the root types are
/// not named `Query`/`Mutation`/`Subscription`, then directives and types in the server's
/// order, without built-in scalars, built-in directives and introspection types.
pub fn sdl(schema: &Value) -> Result<String, String> {
    let mut blocks: Vec<String> = Vec::new();
    let roots = [
        ("query", "queryType", "Query"),
        ("mutation", "mutationType", "Mutation"),
        ("subscription", "subscriptionType", "Subscription"),
    ];
    let named: Vec<(&str, &str, &str)> = roots
        .iter()
        .filter_map(|(op, key, default)| {
            schema
                .get(*key)
                .and_then(|t| str_of(t, "name"))
                .map(|n| (*op, n, *default))
        })
        .collect();
    if named.is_empty() {
        return Err("the schema has no query type".into());
    }
    let description_set = str_of(schema, "description").is_some_and(|d| !d.is_empty());
    if description_set || named.iter().any(|(_, n, d)| n != d) {
        let mut b = String::new();
        description(&mut b, schema, "");
        b.push_str("schema {\n");
        for (op, n, _) in &named {
            let _ = writeln!(b, "  {op}: {n}");
        }
        b.push_str("}\n");
        blocks.push(b);
    }
    for d in list_of(schema, "directives") {
        let name = str_of(d, "name").unwrap_or_default();
        if BUILTIN_DIRECTIVES.contains(&name) {
            continue;
        }
        let mut b = String::new();
        description(&mut b, d, "");
        let _ = write!(b, "directive @{name}");
        args(&mut b, list_of(d, "args"), "")?;
        if d.get("isRepeatable").and_then(Value::as_bool) == Some(true) {
            b.push_str(" repeatable");
        }
        let locations: Vec<&str> = list_of(d, "locations")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let _ = writeln!(b, " on {}", locations.join(" | "));
        blocks.push(b);
    }
    for t in list_of(schema, "types") {
        let name = str_of(t, "name").unwrap_or_default();
        if name.starts_with("__")
            || (str_of(t, "kind") == Some("SCALAR") && BUILTIN_SCALARS.contains(&name))
        {
            continue;
        }
        let mut b = String::new();
        named_type(&mut b, t)?;
        blocks.push(b);
    }
    Ok(blocks.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prints_sdl() {
        let schema = json!({
            "queryType": { "name": "Root" },
            "mutationType": null,
            "subscriptionType": null,
            "directives": [
                { "name": "deprecated", "args": [], "locations": ["FIELD_DEFINITION"] },
                { "name": "auth", "description": "Needs a role.", "isRepeatable": true,
                  "args": [{ "name": "role", "type": { "kind": "SCALAR", "name": "String" }, "defaultValue": "\"admin\"" }],
                  "locations": ["FIELD_DEFINITION", "OBJECT"] }
            ],
            "types": [
                { "kind": "OBJECT", "name": "Root", "description": "Line one\nline two",
                  "interfaces": [{ "kind": "INTERFACE", "name": "Node", "ofType": null }],
                  "fields": [
                    { "name": "id", "args": [], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "isDeprecated": false },
                    { "name": "search", "description": "Find things.",
                      "args": [
                        { "name": "q", "description": "Text", "type": { "kind": "SCALAR", "name": "String" }, "defaultValue": null },
                        { "name": "first", "type": { "kind": "SCALAR", "name": "Int" }, "defaultValue": "10" }
                      ],
                      "type": { "kind": "LIST", "name": null, "ofType": { "kind": "UNION", "name": "Hit" } },
                      "isDeprecated": true, "deprecationReason": "Use find." }
                  ] },
                { "kind": "INTERFACE", "name": "Node", "fields": [
                    { "name": "id", "args": [], "type": { "kind": "NON_NULL", "ofType": { "kind": "SCALAR", "name": "ID" } } }
                  ], "interfaces": [] },
                { "kind": "UNION", "name": "Hit", "possibleTypes": [{ "kind": "OBJECT", "name": "Root" }] },
                { "kind": "ENUM", "name": "Color", "enumValues": [
                    { "name": "RED", "isDeprecated": false },
                    { "name": "OLD", "isDeprecated": true, "deprecationReason": "No longer supported" }
                  ] },
                { "kind": "INPUT_OBJECT", "name": "Filter", "inputFields": [
                    { "name": "color", "type": { "kind": "ENUM", "name": "Color" }, "defaultValue": "RED" }
                  ] },
                { "kind": "SCALAR", "name": "Date", "specifiedByURL": "https://example.com/date" },
                { "kind": "SCALAR", "name": "String" },
                { "kind": "OBJECT", "name": "__Type", "fields": [] }
            ]
        });
        assert_eq!(
            sdl(&schema).unwrap(),
            r#"schema {
  query: Root
}

"Needs a role."
directive @auth(role: String = "admin") repeatable on FIELD_DEFINITION | OBJECT

"""
Line one
line two
"""
type Root implements Node {
  id: ID!
  "Find things."
  search(
    "Text"
    q: String
    first: Int = 10
  ): [Hit] @deprecated(reason: "Use find.")
}

interface Node {
  id: ID!
}

union Hit = Root

enum Color {
  RED
  OLD @deprecated
}

input Filter {
  color: Color = RED
}

scalar Date @specifiedBy(url: "https://example.com/date")
"#
        );
        assert!(sdl(&json!({ "types": [] })).is_err());
    }

    #[test]
    fn the_test_server_schema_prints() {
        let printed = sdl(&rt_testserver_schema()).unwrap();
        assert!(
            printed.starts_with("\"The root of all queries.\"\ntype Query {\n"),
            "{printed}"
        );
        assert!(
            printed.contains("  hello(name: String = \"world\"): String!\n"),
            "{printed}"
        );
        assert!(printed.contains("  year: Int @deprecated(reason: \"Use published.\")\n"));
        assert!(
            printed.contains("input BookInput {\n  title: String!\n  genre: Genre = FICTION\n}\n")
        );
        assert!(!printed.contains("scalar String"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn rt_testserver_schema() -> Value {
        rt_testserver::graphql::schema()
    }

    #[cfg(target_arch = "wasm32")]
    fn rt_testserver_schema() -> Value {
        json!({ "queryType": { "name": "Query" }, "types": [] })
    }

    #[test]
    fn introspection_replaces_the_body_only() {
        let mut files = rt_format::FileSet::default();
        for (p, t) in [
            (
                "opencollection.yml",
                "opencollection: 1.0.0\ninfo:\n  name: g\n",
            ),
            (
                "q.yml",
                "info:\n  name: q\n  type: graphql\n\ngraphql:\n  method: POST\n  url: \"{{api}}/graphql\"\n  headers:\n    - name: x-a\n      value: \"1\"\n  body:\n    query: \"{ a }\"\n    variables: \"{}\"\n  auth:\n    type: bearer\n    token: t\n\nruntime:\n  scripts:\n    - type: before-request\n      code: \"x\"\n",
            ),
        ] {
            files.files.insert(p.into(), t.as_bytes().to_vec());
        }
        let ws = Workspace::open(files).unwrap();
        let input = SendInput {
            vars: vec![("api".into(), "http://h".into())],
            ..SendInput::default()
        };
        let snap = introspection_snapshot(&ws, "q.yml", input.clone()).unwrap();
        let spec = &snap.collection.request("q.yml").unwrap().request.spec;
        assert!(matches!(&spec.body, Body::GraphQl { query, .. } if query == INTROSPECTION_QUERY));
        assert!(spec.scripts.is_empty());
        assert_eq!(spec.headers.len(), 1);
        assert_eq!(
            cache_key(&ws, "q.yml", &input).unwrap(),
            "\nhttp://h/graphql"
        );
        // The workspace itself is unchanged.
        assert!(ws.dirty().is_empty());
    }
}
