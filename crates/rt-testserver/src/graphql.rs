// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `POST /graphql` (wave 8, strand B): a tiny GraphQL endpoint for the schema browser and
//! sends. It answers the standard introspection query (any query that asks for `__schema`)
//! and executes simple operations against a fixed library. Test infrastructure only.
//!
//! ```graphql
//! "The root of all queries."
//! type Query {
//!   "Greets `name`."
//!   hello(name: String = "world"): String!
//!   books(genre: Genre): [Book!]!
//!   book(id: ID!): Book
//! }
//! type Mutation { addBook(input: BookInput!): Book! }
//! "A book in the test library."
//! type Book { id: ID!, title: String!, genre: Genre, year: Int @deprecated(reason: "Use published."), published: String }
//! enum Genre { FICTION SCIENCE }
//! input BookInput { title: String!, genre: Genre = FICTION }
//! ```
//!
//! Supported: `query`/`mutation`/anonymous operations, operation names, aliases, arguments
//! (literals and `$variables`), nested selections, `__typename`. Not supported (an error):
//! fragments, directives, subscriptions. A request header `authorization` is echoed by the
//! `hello` field as `Hello, {name} ({authorization})!` so tests can see auth.

use serde_json::{Map, Value, json};

/// The JSON response for a GraphQL request body (`{"query", "variables", "operationName"}`).
pub fn respond(body: &[u8], authorization: Option<&str>) -> Value {
    let Ok(request) = serde_json::from_slice::<Value>(body) else {
        return error("the body is not JSON");
    };
    let Some(query) = request.get("query").and_then(Value::as_str) else {
        return error("no query");
    };
    let variables = request
        .get("variables")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if query.contains("__schema") {
        return json!({ "data": { "__schema": schema() } });
    }
    match parse(query) {
        Ok(op) => match execute(&op, &variables, authorization) {
            Ok(data) => json!({ "data": data }),
            Err(e) => error(&e),
        },
        Err(e) => error(&e),
    }
}

fn error(message: &str) -> Value {
    json!({ "errors": [{ "message": message }] })
}

// ---------------------------------------------------------------------------------------------
// Introspection

fn named(kind: &str, name: &str) -> Value {
    json!({ "kind": kind, "name": name, "ofType": null })
}

fn non_null(of: Value) -> Value {
    json!({ "kind": "NON_NULL", "name": null, "ofType": of })
}

fn list(of: Value) -> Value {
    json!({ "kind": "LIST", "name": null, "ofType": of })
}

fn scalar(name: &str) -> Value {
    named("SCALAR", name)
}

fn arg(name: &str, ty: Value, default: Option<&str>) -> Value {
    json!({
        "name": name, "description": null, "type": ty, "defaultValue": default,
        "isDeprecated": false, "deprecationReason": null
    })
}

fn field(name: &str, description: Option<&str>, args: Vec<Value>, ty: Value) -> Value {
    json!({
        "name": name, "description": description, "args": args, "type": ty,
        "isDeprecated": false, "deprecationReason": null
    })
}

fn object(name: &str, description: Option<&str>, fields: Vec<Value>) -> Value {
    json!({
        "kind": "OBJECT", "name": name, "description": description, "specifiedByURL": null,
        "fields": fields, "inputFields": null, "interfaces": [], "enumValues": null,
        "possibleTypes": null
    })
}

fn builtin_scalar(name: &str) -> Value {
    json!({
        "kind": "SCALAR", "name": name, "description": null, "specifiedByURL": null,
        "fields": null, "inputFields": null, "interfaces": null, "enumValues": null,
        "possibleTypes": null
    })
}

/// The `__schema` of the test library, as the standard introspection query returns it.
pub fn schema() -> Value {
    let book = named("OBJECT", "Book");
    let mut year = field("year", None, vec![], scalar("Int"));
    year["isDeprecated"] = json!(true);
    year["deprecationReason"] = json!("Use published.");
    let types = vec![
        object(
            "Query",
            Some("The root of all queries."),
            vec![
                field(
                    "hello",
                    Some("Greets `name`."),
                    vec![arg("name", scalar("String"), Some("\"world\""))],
                    non_null(scalar("String")),
                ),
                field(
                    "books",
                    None,
                    vec![arg("genre", named("ENUM", "Genre"), None)],
                    non_null(list(non_null(book.clone()))),
                ),
                field(
                    "book",
                    None,
                    vec![arg("id", non_null(scalar("ID")), None)],
                    book.clone(),
                ),
            ],
        ),
        object(
            "Mutation",
            None,
            vec![field(
                "addBook",
                None,
                vec![arg(
                    "input",
                    non_null(named("INPUT_OBJECT", "BookInput")),
                    None,
                )],
                non_null(book.clone()),
            )],
        ),
        object(
            "Book",
            Some("A book in the test library."),
            vec![
                field("id", None, vec![], non_null(scalar("ID"))),
                field("title", None, vec![], non_null(scalar("String"))),
                field("genre", None, vec![], named("ENUM", "Genre")),
                year,
                field("published", None, vec![], scalar("String")),
            ],
        ),
        json!({
            "kind": "ENUM", "name": "Genre", "description": null, "specifiedByURL": null,
            "fields": null, "inputFields": null, "interfaces": null, "possibleTypes": null,
            "enumValues": [
                { "name": "FICTION", "description": null, "isDeprecated": false, "deprecationReason": null },
                { "name": "SCIENCE", "description": null, "isDeprecated": false, "deprecationReason": null }
            ]
        }),
        json!({
            "kind": "INPUT_OBJECT", "name": "BookInput", "description": null,
            "specifiedByURL": null, "fields": null, "interfaces": null, "enumValues": null,
            "possibleTypes": null,
            "inputFields": [
                arg("title", non_null(scalar("String")), None),
                arg("genre", named("ENUM", "Genre"), Some("FICTION")),
            ]
        }),
        builtin_scalar("String"),
        builtin_scalar("ID"),
        builtin_scalar("Int"),
        builtin_scalar("Boolean"),
    ];
    json!({
        "description": null,
        "queryType": { "name": "Query" },
        "mutationType": { "name": "Mutation" },
        "subscriptionType": null,
        "types": types,
        "directives": [
            {
                "name": "deprecated", "description": null, "isRepeatable": false,
                "locations": ["FIELD_DEFINITION", "ENUM_VALUE"],
                "args": [arg("reason", scalar("String"), Some("\"No longer supported\""))]
            }
        ]
    })
}

// ---------------------------------------------------------------------------------------------
// A small parser and executor

#[derive(Debug)]
struct Operation {
    mutation: bool,
    selections: Vec<Selection>,
}

#[derive(Debug)]
struct Selection {
    alias: Option<String>,
    name: String,
    args: Vec<(String, Arg)>,
    selections: Vec<Selection>,
}

#[derive(Debug, Clone)]
enum Arg {
    Var(String),
    Value(Value),
}

struct Parser<'a> {
    src: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.at < self.src.len() {
            match self.src[self.at] {
                b' ' | b'\t' | b'\n' | b'\r' | b',' => self.at += 1,
                b'#' => {
                    while self.at < self.src.len() && self.src[self.at] != b'\n' {
                        self.at += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.src.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected `{}` at offset {}", c as char, self.at))
        }
    }

    fn name(&mut self) -> Result<String, String> {
        self.skip();
        let start = self.at;
        while self
            .src
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.at += 1;
        }
        if start == self.at {
            return Err(format!("expected a name at offset {start}"));
        }
        Ok(String::from_utf8_lossy(&self.src[start..self.at]).into_owned())
    }

    fn operation(&mut self) -> Result<Operation, String> {
        let mut mutation = false;
        if self.peek() != Some(b'{') {
            match self.name()?.as_str() {
                "query" => {}
                "mutation" => mutation = true,
                "fragment" => return Err("fragments are not supported by the test server".into()),
                other => return Err(format!("unsupported operation `{other}`")),
            }
            if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                self.name()?;
            }
            if self.peek() == Some(b'(') {
                // Variable definitions: skipped, values come from `variables`.
                let mut depth = 0;
                while let Some(c) = self.src.get(self.at) {
                    self.at += 1;
                    match c {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let selections = self.selection_set()?;
        if self.peek().is_some() {
            return Err("only one operation per document is supported by the test server".into());
        }
        Ok(Operation {
            mutation,
            selections,
        })
    }

    fn selection_set(&mut self) -> Result<Vec<Selection>, String> {
        self.eat(b'{')?;
        let mut out = Vec::new();
        while self.peek() != Some(b'}') {
            if self.peek() == Some(b'.') {
                return Err("fragments are not supported by the test server".into());
            }
            if self.peek().is_none() {
                return Err("unexpected end of the query".into());
            }
            let first = self.name()?;
            let (alias, name) = if self.peek() == Some(b':') {
                self.at += 1;
                (Some(first), self.name()?)
            } else {
                (None, first)
            };
            let mut args = Vec::new();
            if self.peek() == Some(b'(') {
                self.at += 1;
                while self.peek() != Some(b')') {
                    let arg = self.name()?;
                    self.eat(b':')?;
                    args.push((arg, self.value()?));
                }
                self.at += 1;
            }
            if self.peek() == Some(b'@') {
                return Err("directives are not supported by the test server".into());
            }
            let selections = if self.peek() == Some(b'{') {
                self.selection_set()?
            } else {
                Vec::new()
            };
            out.push(Selection {
                alias,
                name,
                args,
                selections,
            });
        }
        self.at += 1;
        Ok(out)
    }

    fn value(&mut self) -> Result<Arg, String> {
        match self.peek() {
            Some(b'$') => {
                self.at += 1;
                Ok(Arg::Var(self.name()?))
            }
            Some(b'"') => {
                self.at += 1;
                let start = self.at;
                while self.src.get(self.at).is_some_and(|c| *c != b'"') {
                    if self.src[self.at] == b'\\' {
                        self.at += 1;
                    }
                    self.at += 1;
                }
                let raw = String::from_utf8_lossy(&self.src[start..self.at]).into_owned();
                self.at += 1;
                Ok(Arg::Value(Value::String(raw.replace("\\\"", "\""))))
            }
            Some(b'{') => {
                self.at += 1;
                let mut map = Map::new();
                while self.peek() != Some(b'}') {
                    let key = self.name()?;
                    self.eat(b':')?;
                    let Arg::Value(v) = self.value()? else {
                        return Err("variables inside objects are not supported".into());
                    };
                    map.insert(key, v);
                }
                self.at += 1;
                Ok(Arg::Value(Value::Object(map)))
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let start = self.at;
                while self
                    .src
                    .get(self.at)
                    .is_some_and(|c| *c == b'-' || *c == b'.' || c.is_ascii_digit())
                {
                    self.at += 1;
                }
                let text = String::from_utf8_lossy(&self.src[start..self.at]).into_owned();
                serde_json::from_str(&text)
                    .map(Arg::Value)
                    .map_err(|_| format!("bad number {text}"))
            }
            Some(_) => {
                let word = self.name()?;
                Ok(Arg::Value(match word.as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "null" => Value::Null,
                    // Enum values are strings in the response.
                    _ => Value::String(word),
                }))
            }
            None => Err("unexpected end of the query".into()),
        }
    }
}

fn parse(query: &str) -> Result<Operation, String> {
    Parser {
        src: query.as_bytes(),
        at: 0,
    }
    .operation()
}

fn books() -> Vec<Value> {
    vec![
        json!({ "id": "1", "title": "The Test Pilot", "genre": "FICTION", "year": 1999, "published": "1999-05-01" }),
        json!({ "id": "2", "title": "Protocols at Rest", "genre": "SCIENCE", "year": 2021, "published": "2021-11-02" }),
    ]
}

fn arg_value(args: &[(String, Arg)], name: &str, variables: &Map<String, Value>) -> Option<Value> {
    let (_, arg) = args.iter().find(|(n, _)| n == name)?;
    match arg {
        Arg::Var(v) => variables.get(v).cloned(),
        Arg::Value(v) => Some(v.clone()),
    }
}

fn select(type_name: &str, value: &Value, selections: &[Selection]) -> Result<Value, String> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    if let Value::Array(items) = value {
        return items
            .iter()
            .map(|v| select(type_name, v, selections))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    if selections.is_empty() {
        return Err(format!("field of type {type_name} needs a selection"));
    }
    let mut out = Map::new();
    for s in selections {
        let key = s.alias.clone().unwrap_or_else(|| s.name.clone());
        let v = match s.name.as_str() {
            "__typename" => Value::String(type_name.into()),
            name => match value.get(name) {
                Some(v) => v.clone(),
                None => {
                    return Err(format!(
                        "Cannot query field \"{name}\" on type \"{type_name}\"."
                    ));
                }
            },
        };
        out.insert(key, v);
    }
    Ok(Value::Object(out))
}

fn execute(
    op: &Operation,
    variables: &Map<String, Value>,
    authorization: Option<&str>,
) -> Result<Value, String> {
    let root = if op.mutation { "Mutation" } else { "Query" };
    let mut out = Map::new();
    for s in &op.selections {
        let key = s.alias.clone().unwrap_or_else(|| s.name.clone());
        let get = |n: &str| arg_value(&s.args, n, variables);
        let value = match (op.mutation, s.name.as_str()) {
            (_, "__typename") => Value::String(root.into()),
            (false, "hello") => {
                let name = get("name")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "world".into());
                Value::String(match authorization {
                    Some(a) => format!("Hello, {name} ({a})!"),
                    None => format!("Hello, {name}!"),
                })
            }
            (false, "books") => {
                let genre = get("genre");
                let list: Vec<Value> = books()
                    .into_iter()
                    .filter(|b| {
                        genre
                            .as_ref()
                            .is_none_or(|g| g.is_null() || b["genre"] == *g)
                    })
                    .collect();
                select("Book", &Value::Array(list), &s.selections)?
            }
            (false, "book") => {
                let id = get("id").ok_or("book needs an id")?;
                let id = id
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| id.to_string());
                let found = books()
                    .into_iter()
                    .find(|b| b["id"] == id.as_str())
                    .unwrap_or(Value::Null);
                select("Book", &found, &s.selections)?
            }
            (true, "addBook") => {
                let input = get("input").ok_or("addBook needs an input")?;
                let title = input
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or("BookInput.title is required")?;
                let genre = input.get("genre").cloned().unwrap_or(json!("FICTION"));
                let book = json!({ "id": "3", "title": title, "genre": genre, "year": null, "published": null });
                select("Book", &book, &s.selections)?
            }
            (_, name) => return Err(format!("Cannot query field \"{name}\" on type \"{root}\".")),
        };
        out.insert(key, value);
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::respond;
    use serde_json::json;

    fn ask(query: &str, variables: serde_json::Value) -> serde_json::Value {
        let body = json!({ "query": query, "variables": variables }).to_string();
        respond(body.as_bytes(), None)
    }

    #[test]
    fn queries_mutations_and_errors() {
        assert_eq!(
            ask("{ hello }", json!(null)),
            json!({ "data": { "hello": "Hello, world!" } })
        );
        assert_eq!(
            ask(
                "query Q($g: Genre) { list: books(genre: $g) { title __typename } }",
                json!({ "g": "SCIENCE" })
            ),
            json!({ "data": { "list": [{ "title": "Protocols at Rest", "__typename": "Book" }] } })
        );
        assert_eq!(
            ask(
                "mutation { addBook(input: { title: \"New\" }) { id title genre } }",
                json!({})
            ),
            json!({ "data": { "addBook": { "id": "3", "title": "New", "genre": "FICTION" } } })
        );
        assert_eq!(
            ask("{ book(id: 9) { title } }", json!({})),
            json!({ "data": { "book": null } })
        );
        assert!(
            ask("{ nope }", json!({}))["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("nope")
        );
        assert!(ask("{ ...F }", json!({}))["errors"].is_array());
        let schema = ask(
            "query IntrospectionQuery { __schema { types { name } } }",
            json!({}),
        );
        assert_eq!(schema["data"]["__schema"]["queryType"]["name"], "Query");
    }
}
