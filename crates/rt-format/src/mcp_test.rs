// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved MCP tests (wave 8, strand D): one `<name>.mcp.yaml` companion file per test.
//!
//! The OpenCollection reader reads only `*.yml`, so these files never show up as requests.
//! The app's types live in `rt_app::mcp_client` (`McpTest`); this module is the file format:
//!
//! ```yaml
//! name: Echo says hello
//! server:
//!   transport: http
//!   url: "{{mcpUrl}}"
//!   headers:
//!     - name: Authorization
//!       value: "Bearer {{token}}"
//! tool: echo
//! arguments:
//!   text: hello
//! assertions:
//!   - expression: res.body.text
//!     operator: eq
//!     value: hello
//! ```
//!
//! - A `stdio` server has `command`, `args` (list), `env` (list of `name`/`value`) and an
//!   optional `cwd` instead of `url` and `headers`.
//! - Keys are written in this order; keys of `arguments` sorted, so the file is stable
//!   whatever order a UI produced. Empty lists and `cwd: null` are left out, `enabled: false`
//!   is written only for disabled assertions, `value` only when not empty.
//! - Round trip: [`write`] of what [`read`] returned gives the same bytes when the file is
//!   unchanged (`previous`), and every file this writer produced reads back to the same test.
//! - Secret values never belong here, only `{{name}}` references; the app checks that before
//!   it stages a file (`rt_app::mcp_client::save_test`).

use rt_core::model::Assertion;
use serde_json::{Map, Number, Value};

use crate::FormatError;
use crate::yaml::{self, Node, Resolved, Scalar, Style};

/// File extension of saved MCP tests (as `rt_app::mcp_client::TEST_EXTENSION`).
pub const EXTENSION: &str = "mcp.yaml";

/// A saved MCP test as the file has it.
#[derive(Debug, Clone, PartialEq)]
pub struct TestFile {
    pub name: String,
    pub server: ServerFile,
    pub tool: String,
    pub arguments: Value,
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerFile {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// Whether `path` names a saved MCP test.
pub fn is_test_path(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(EXTENSION))
        .is_some_and(|stem| stem.len() > 1 && stem.ends_with('.'))
}

/// Reads one test file. `path` is only used in error messages.
pub fn read(path: &str, bytes: &[u8]) -> Result<TestFile, FormatError> {
    let invalid = |message: String| FormatError::Invalid {
        path: path.to_owned(),
        message,
    };
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("not UTF-8".into()))?;
    let doc = yaml::parse(text).map_err(|e| invalid(e.to_string()))?;
    let root = &doc.root;
    if !root.is_map() {
        return Err(invalid("expected a mapping".into()));
    }
    let Node::Map(map) = root else {
        unreachable!("checked")
    };
    for e in &map.entries {
        if !["name", "server", "tool", "arguments", "assertions"].contains(&e.key.value.as_str()) {
            return Err(invalid(format!("unknown key {:?}", e.key.value)));
        }
    }
    let text_of = |node: Option<&Node>, what: &str| -> Result<String, FormatError> {
        match node {
            Some(n) => n
                .text()
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("{what} must be text"))),
            None => Err(invalid(format!("missing {what}"))),
        }
    };
    let name = text_of(root.get("name"), "name")?;
    let tool = text_of(root.get("tool"), "tool")?;
    let server = read_server(
        root.get("server")
            .ok_or_else(|| invalid("missing server".into()))?,
    )
    .map_err(invalid)?;
    let arguments = match root.get("arguments") {
        None => Value::Object(Map::new()),
        Some(n) => to_json(n).map_err(|e| invalid(format!("arguments: {e}")))?,
    };
    let mut assertions = Vec::new();
    if let Some(list) = root.get("assertions") {
        if !list.is_seq() {
            return Err(invalid("assertions must be a list".into()));
        }
        for (i, item) in list.items().enumerate() {
            assertions.push(
                read_assertion(item).map_err(|e| invalid(format!("assertion {}: {e}", i + 1)))?,
            );
        }
    }
    Ok(TestFile {
        name,
        server,
        tool,
        arguments,
        assertions,
    })
}

/// Renders `test`. With `previous` (the file's current bytes), an unchanged test keeps them
/// byte for byte (comments, layout), so saving never rewrites a file nobody changed.
pub fn write(test: &TestFile, previous: Option<&[u8]>) -> String {
    if let Some(bytes) = previous
        && let Ok(text) = std::str::from_utf8(bytes)
        && read("", bytes).is_ok_and(|t| t == *test)
    {
        return text.to_owned();
    }
    let mut out = String::new();
    out.push_str(&format!("name: {}\n", scalar(&test.name)));
    out.push_str("server:\n");
    match &test.server {
        ServerFile::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            out.push_str("  transport: stdio\n");
            out.push_str(&format!("  command: {}\n", scalar(command)));
            if !args.is_empty() {
                out.push_str("  args:\n");
                for a in args {
                    out.push_str(&format!("    - {}\n", scalar(a)));
                }
            }
            pairs(&mut out, "env", env);
            if let Some(cwd) = cwd {
                out.push_str(&format!("  cwd: {}\n", scalar(cwd)));
            }
        }
        ServerFile::Http { url, headers } => {
            out.push_str("  transport: http\n");
            out.push_str(&format!("  url: {}\n", scalar(url)));
            pairs(&mut out, "headers", headers);
        }
    }
    out.push_str(&format!("tool: {}\n", scalar(&test.tool)));
    out.push_str("arguments:");
    value(&mut out, &test.arguments, 0);
    if !test.assertions.is_empty() {
        out.push_str("assertions:\n");
        for a in &test.assertions {
            out.push_str(&format!("  - expression: {}\n", scalar(&a.expression)));
            out.push_str(&format!("    operator: {}\n", scalar(&a.operator)));
            if !a.value.is_empty() {
                out.push_str(&format!("    value: {}\n", scalar(&a.value)));
            }
            if let Some(d) = &a.description {
                out.push_str(&format!("    description: {}\n", scalar(d)));
            }
            if !a.enabled {
                out.push_str("    enabled: false\n");
            }
        }
    }
    out
}

fn pairs(out: &mut String, key: &str, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    out.push_str(&format!("  {key}:\n"));
    for (name, value) in pairs {
        out.push_str(&format!("    - name: {}\n", scalar(name)));
        out.push_str(&format!("      value: {}\n", scalar(value)));
    }
}

/// Writes `v` as the value of a key at column `col` whose line is already started (`key:`),
/// ending with a line break. Layout as Bruno writes YAML (two columns per level; the first
/// entry of a mapping in a sequence follows its `- `).
fn value(out: &mut String, v: &Value, col: usize) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            entries(out, m, col + 2, false);
        }
        Value::Array(items) if !items.is_empty() => {
            out.push('\n');
            for v in items {
                out.push_str(&" ".repeat(col + 2));
                item(out, v, col + 2);
            }
        }
        other => {
            out.push(' ');
            out.push_str(&leaf(other));
            out.push('\n');
        }
    }
}

/// The entries of a mapping with keys at column `col`, sorted; with `inline`, the first key
/// goes right where the line is (after a `- `).
fn entries(out: &mut String, m: &Map<String, Value>, col: usize, inline: bool) {
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    for (n, k) in keys.into_iter().enumerate() {
        if n > 0 || !inline {
            out.push_str(&" ".repeat(col));
        }
        out.push_str(&scalar(k));
        out.push(':');
        value(out, &m[k], col);
    }
}

/// A sequence item whose `-` is at column `col` (the line is indented already).
fn item(out: &mut String, v: &Value, col: usize) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            out.push_str("- ");
            entries(out, m, col + 2, true);
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("-\n");
            for v in items {
                out.push_str(&" ".repeat(col + 2));
                item(out, v, col + 2);
            }
        }
        other => {
            out.push_str("- ");
            out.push_str(&leaf(other));
            out.push('\n');
        }
    }
}

/// An empty collection or a scalar, on one line.
fn leaf(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => scalar(s),
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
    }
}

/// A string as a YAML scalar: plain where that reads back as the same string, else
/// double-quoted with JSON escapes (a subset of YAML's).
fn scalar(s: &str) -> String {
    let safe_char = |c: char| c.is_ascii_alphanumeric() || " _./-+@()".contains(c);
    let plain = !s.is_empty()
        && s.chars().all(safe_char)
        && (s.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '/' || c == '(')
            // `-x` is plain, `- x` would be a sequence item.
            || (s.starts_with('-') && s.len() > 1 && !s[1..].starts_with(' ')))
        && !s.ends_with(' ')
        && yaml::resolve_plain(s) == Resolved::Str;
    if plain {
        s.to_owned()
    } else {
        serde_json::to_string(s).expect("strings serialize")
    }
}

fn read_server(node: &Node) -> Result<ServerFile, String> {
    if !node.is_map() {
        return Err("server must be a mapping".into());
    }
    let text = |key: &str| node.get(key).and_then(Node::text).map(str::to_owned);
    let list = |key: &str| -> Result<Vec<(String, String)>, String> {
        let Some(n) = node.get(key) else {
            return Ok(Vec::new());
        };
        if !n.is_seq() {
            return Err(format!("server.{key} must be a list"));
        }
        n.items()
            .map(|item| {
                let name = item.get("name").and_then(Node::text);
                let value = item.get("value").and_then(Node::text).unwrap_or("");
                match name {
                    Some(name) => Ok((name.to_owned(), value.to_owned())),
                    None => Err(format!("server.{key}: every entry needs a name")),
                }
            })
            .collect()
    };
    let allowed: &[&str] = match text("transport").as_deref() {
        Some("stdio") => &["transport", "command", "args", "env", "cwd"],
        Some("http") => &["transport", "url", "headers"],
        Some(other) => return Err(format!("unknown server.transport {other:?}")),
        None => return Err("missing server.transport (stdio or http)".into()),
    };
    if let Node::Map(m) = node
        && let Some(e) = m
            .entries
            .iter()
            .find(|e| !allowed.contains(&e.key.value.as_str()))
    {
        return Err(format!("unknown key server.{}", e.key.value));
    }
    if allowed.contains(&"url") {
        Ok(ServerFile::Http {
            url: text("url").ok_or("missing server.url")?,
            headers: list("headers")?,
        })
    } else {
        let args = match node.get("args") {
            None => Vec::new(),
            Some(n) if n.is_seq() => n
                .items()
                .map(|i| {
                    i.text()
                        .map(str::to_owned)
                        .ok_or("server.args must be text")
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("server.args must be a list".into()),
        };
        Ok(ServerFile::Stdio {
            command: text("command").ok_or("missing server.command")?,
            args,
            env: list("env")?,
            cwd: text("cwd"),
        })
    }
}

fn read_assertion(node: &Node) -> Result<Assertion, String> {
    if !node.is_map() {
        return Err("expected a mapping".into());
    }
    if let Node::Map(m) = node
        && let Some(e) = m.entries.iter().find(|e| {
            !["expression", "operator", "value", "description", "enabled"]
                .contains(&e.key.value.as_str())
        })
    {
        return Err(format!("unknown key {:?}", e.key.value));
    }
    let text = |key: &str| node.get(key).and_then(Node::text).map(str::to_owned);
    Ok(Assertion {
        expression: text("expression").ok_or("missing expression")?,
        operator: text("operator").unwrap_or_default(),
        value: text("value").unwrap_or_default(),
        description: text("description"),
        enabled: match node.get("enabled") {
            None => true,
            Some(n) => n.as_bool().ok_or("enabled must be true or false")?,
        },
    })
}

fn to_json(node: &Node) -> Result<Value, String> {
    Ok(match node {
        Node::Scalar(s) => scalar_json(s)?,
        Node::Map(m) => {
            let mut out = Map::new();
            for e in &m.entries {
                out.insert(e.key.value.clone(), to_json(&e.value)?);
            }
            Value::Object(out)
        }
        Node::Seq(s) => Value::Array(
            s.items
                .iter()
                .map(|i| to_json(&i.value))
                .collect::<Result<_, _>>()?,
        ),
    })
}

fn scalar_json(s: &Scalar) -> Result<Value, String> {
    if s.style != Style::Plain {
        return Ok(Value::String(s.value.clone()));
    }
    Ok(match s.resolve() {
        Resolved::Null => Value::Null,
        Resolved::Bool(b) => Value::Bool(b),
        Resolved::Int(i) => Value::Number(i.into()),
        Resolved::Float(f) => Value::Number(
            Number::from_f64(f).ok_or_else(|| format!("{} is not a JSON number", s.value))?,
        ),
        Resolved::Str => Value::String(s.value.clone()),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn assertion(expression: &str, operator: &str, value: &str) -> Assertion {
        Assertion {
            expression: expression.into(),
            operator: operator.into(),
            value: value.into(),
            description: None,
            enabled: true,
        }
    }

    fn http_test() -> TestFile {
        TestFile {
            name: "Echo says hello".into(),
            server: ServerFile::Http {
                url: "{{mcpUrl}}".into(),
                headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
            },
            tool: "echo".into(),
            arguments: json!({"text": "hello"}),
            assertions: vec![assertion("res.body.text", "eq", "hello")],
        }
    }

    const HTTP_FILE: &str = "name: Echo says hello
server:
  transport: http
  url: \"{{mcpUrl}}\"
  headers:
    - name: Authorization
      value: \"Bearer {{token}}\"
tool: echo
arguments:
  text: hello
assertions:
  - expression: res.body.text
    operator: eq
    value: hello
";

    #[test]
    fn writes_the_documented_layout() {
        assert_eq!(write(&http_test(), None), HTTP_FILE);
        assert_eq!(
            read("t.mcp.yaml", HTTP_FILE.as_bytes()).unwrap(),
            http_test()
        );
    }

    #[test]
    fn stdio_servers_and_nested_arguments_round_trip() {
        let test = TestFile {
            name: "add: 2 + 3".into(),
            server: ServerFile::Stdio {
                command: "rt-testserver".into(),
                args: vec!["--mcp-stdio".into(), "--flag=\"x\"".into()],
                env: vec![("API_KEY".into(), "{{apiKey}}".into())],
                cwd: Some("tools".into()),
            },
            tool: "add".into(),
            arguments: json!({
                "b": 3, "a": 2.5, "neg": -1, "big": 1e100,
                "s": "true", "n": "12", "empty": "", "nul": null, "t": true,
                "multi": "line 1\nline 2\t\u{1}é", "obj": {}, "arr": [],
                "list": [1, "two", {"x": [null, {"y": "z"}]}, [[]]],
                "key with: colon": "#hash", "-dash": "- item", "quote": "\"'",
            }),
            assertions: vec![
                Assertion {
                    description: Some("sum".into()),
                    enabled: false,
                    ..assertion("res.body.sum", "eq", "5.5")
                },
                assertion("res.status", "isNumber", ""),
            ],
        };
        let text = write(&test, None);
        let back = read("x.mcp.yaml", text.as_bytes()).unwrap();
        assert_eq!(back, test, "{text}");
        // Stable: writing what was read gives the same bytes.
        assert_eq!(write(&back, None), text);
        assert!(text.contains("  args:\n    - --mcp-stdio\n"), "{text}");
        assert!(text.contains("  s: \"true\"\n"), "{text}");
        assert!(text.contains("  cwd: tools\n"), "{text}");
        assert!(text.contains("    enabled: false\n"), "{text}");
    }

    #[test]
    fn unchanged_files_keep_their_bytes() {
        let hand = "# written by hand\nname: 'Echo says hello'\nserver: {transport: http, url: \"{{mcpUrl}}\", headers: [{name: Authorization, value: \"Bearer {{token}}\"}]}\ntool: echo   # the tool\narguments: {text: hello}\nassertions:\n- expression: res.body.text\n  operator: eq\n  value: hello\n";
        let test = read("t.mcp.yaml", hand.as_bytes()).unwrap();
        assert_eq!(test, http_test());
        assert_eq!(write(&test, Some(hand.as_bytes())), hand);
        // A changed test is written in the canonical layout.
        let mut changed = test;
        changed.tool = "echo2".into();
        assert_eq!(
            write(&changed, Some(hand.as_bytes())),
            HTTP_FILE.replace("tool: echo\n", "tool: echo2\n")
        );
    }

    #[test]
    fn rejects_what_it_does_not_know() {
        for (text, needle) in [
            ("- a\n", "expected a mapping"),
            ("name: x\ntool: t\n", "missing server"),
            ("name: x\nserver:\n  url: u\ntool: t\n", "transport"),
            (
                "name: x\nserver:\n  transport: ws\ntool: t\n",
                "unknown server.transport",
            ),
            (
                "name: x\nserver:\n  transport: http\n  url: u\n  command: c\ntool: t\n",
                "unknown key server.command",
            ),
            (
                "name: x\nserver:\n  transport: http\n  url: u\ntool: t\nextra: 1\n",
                "unknown key",
            ),
            (
                "name: x\nserver:\n  transport: http\n  url: u\n",
                "missing tool",
            ),
            (
                "name: x\nserver:\n  transport: http\n  url: u\ntool: t\nassertions:\n  - operator: eq\n",
                "missing expression",
            ),
            ("name: [\n", "line"),
        ] {
            let err = read("bad.mcp.yaml", text.as_bytes())
                .unwrap_err()
                .to_string();
            assert!(err.contains(needle), "{text:?}: {err}");
            assert!(err.starts_with("bad.mcp.yaml: "), "{err}");
        }
        // Arguments default to an empty object.
        let t = read(
            "t.mcp.yaml",
            b"name: x\nserver:\n  transport: http\n  url: u\ntool: t\n",
        )
        .unwrap();
        assert_eq!(t.arguments, json!({}));
        assert!(t.assertions.is_empty());
    }

    #[test]
    fn test_paths() {
        assert!(is_test_path("mcp/echo.mcp.yaml"));
        assert!(is_test_path("echo.mcp.yaml"));
        assert!(!is_test_path(".mcp.yaml"));
        assert!(!is_test_path("echo.yml"));
        assert!(!is_test_path("echomcp.yaml"));
        assert!(!is_test_path("mcp.yaml/x.yml"));
    }
}
