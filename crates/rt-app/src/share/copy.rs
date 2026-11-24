// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! "Copy as": a request as a curl command, a JavaScript `fetch` call or Python with
//! `requests`. Variables stay variables of the target tool (`$BASE_URL`,
//! `process.env.BASE_URL`, `os.environ["BASE_URL"]`); with "Resolve variables" the values of
//! non-secret variables of the chosen environment are put in. Secret values never are: the
//! model holds none, and a literal credential in the request is replaced by a variable of
//! the target tool like in a link ([`super::sanitize`]).

use std::collections::BTreeMap;

use rt_core::model::{
    ApiKeyPlacement, Auth, Body, Environment, KeyValue, ParamKind, PartKind, Protocol, RawKind,
    RequestSpec,
};

use super::sanitize::{is_sensitive_name, sanitize as sanitize_spec};
use super::{CopiedAs, CopyKind, CopyOptions, ShareError};

/// One piece of a text: literal, or an environment variable of the target tool.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Lit(String),
    Var(String),
}

type Tpl = Vec<Seg>;

fn lit(s: &str) -> Tpl {
    vec![Seg::Lit(s.to_owned())]
}

fn cat(mut a: Tpl, b: Tpl) -> Tpl {
    a.extend(b);
    a
}

fn has_var(t: &Tpl) -> bool {
    t.iter().any(|s| matches!(s, Seg::Var(_)))
}

/// `baseUrl` -> `BASE_URL`, `secret:api key` -> `API_KEY`.
fn env_name(name: &str) -> String {
    let name = name.trim().trim_start_matches('$');
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase()
                && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
            {
                out.push('_');
            }
            out.push(c.to_ascii_uppercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
        prev = Some(c);
    }
    let out = out.trim_end_matches('_').to_owned();
    if out.is_empty() {
        "VALUE".to_owned()
    } else if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{out}")
    } else {
        out
    }
}

struct Ctx {
    resolve: BTreeMap<String, String>,
    vars: Vec<String>,
}

impl Ctx {
    fn tpl(&mut self, text: &str) -> Tpl {
        let mut out: Tpl = Vec::new();
        let push_lit = |out: &mut Tpl, s: &str| {
            if s.is_empty() {
                return;
            }
            if let Some(Seg::Lit(last)) = out.last_mut() {
                last.push_str(s);
            } else {
                out.push(Seg::Lit(s.to_owned()));
            }
        };
        let mut rest = text;
        while let Some(start) = rest.find("{{") {
            let Some(end) = rest[start + 2..].find("}}") else {
                break;
            };
            push_lit(&mut out, &rest[..start]);
            let raw = rest[start + 2..start + 2 + end].trim();
            let (name, secret) = match raw.strip_prefix("secret:") {
                Some(n) => (n.trim(), true),
                None => (raw, false),
            };
            match self.resolve.get(name) {
                Some(value) if !secret => push_lit(&mut out, value),
                _ => {
                    let env = env_name(name);
                    if !self.vars.contains(&env) {
                        self.vars.push(env.clone());
                    }
                    out.push(Seg::Var(env));
                }
            }
            rest = &rest[start + 2 + end + 2..];
        }
        push_lit(&mut out, rest);
        out
    }
}

/// How a body is sent.
enum Payload {
    None,
    Text {
        data: Tpl,
        content_type: Option<&'static str>,
    },
    Form(Vec<(Tpl, Tpl)>),
    Multipart(Vec<Part>),
    File(String),
}

struct Part {
    name: Tpl,
    value: PartValue,
}

enum PartValue {
    Text(Tpl),
    File(String),
}

struct Plan {
    method: String,
    url: Tpl,
    headers: Vec<(String, Tpl)>,
    basic: Option<(Tpl, Tpl)>,
    payload: Payload,
    notes: Vec<String>,
}

fn path_params(spec: &RequestSpec) -> String {
    let mut url = spec.url.clone();
    for p in spec
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Path && p.enabled && !p.value.is_empty())
    {
        let needle = format!(":{}", p.name);
        let mut out = String::new();
        let mut rest = url.as_str();
        while let Some(i) = rest.find(&needle) {
            let after = &rest[i + needle.len()..];
            let at_segment = i > 0 && rest.as_bytes()[i - 1] == b'/';
            let ends = after.is_empty() || after.starts_with(['/', '?', '#']);
            out.push_str(&rest[..i]);
            out.push_str(if at_segment && ends {
                &p.value
            } else {
                &needle
            });
            rest = after;
        }
        out.push_str(rest);
        url = out;
    }
    url
}

fn has_header(headers: &[(String, Tpl)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

fn plan(spec: &RequestSpec, ctx: &mut Ctx) -> Plan {
    let mut notes = Vec::new();
    let mut url_text = path_params(spec);
    let mut headers: Vec<(String, Tpl)> = spec
        .headers
        .iter()
        .filter(|h| h.enabled && !h.name.trim().is_empty())
        .map(|KeyValue { name, value, .. }| (name.trim().to_owned(), ctx.tpl(value)))
        .collect();
    let mut basic = None;
    match &spec.auth {
        Auth::Basic { username, password } => basic = Some((ctx.tpl(username), ctx.tpl(password))),
        Auth::Bearer { token } => {
            headers.push(("Authorization".into(), cat(lit("Bearer "), ctx.tpl(token))));
        }
        Auth::ApiKey { key, value, placement } => match placement {
            ApiKeyPlacement::Header => headers.push((key.clone(), ctx.tpl(value))),
            ApiKeyPlacement::Query => {
                let sep = if url_text.contains('?') { '&' } else { '?' };
                url_text = format!("{url_text}{sep}{key}={value}");
            }
        },
        Auth::Inherit => notes.push(
            "Authentication is inherited from the folder or the collection; it is not part of this snippet."
                .into(),
        ),
        Auth::OAuth2(_) => notes.push(
            "OAuth 2.0 is not part of this snippet: add the access token as a header.".into(),
        ),
        Auth::AwsSigV4(_) => notes.push("AWS Signature V4 is not part of this snippet.".into()),
        Auth::Unsupported { kind } => notes.push(format!("{kind} authentication is not part of this snippet.")),
        Auth::None => {}
    }
    let payload = match &spec.body {
        Body::None => Payload::None,
        Body::Raw { kind, data } => Payload::Text {
            data: ctx.tpl(data),
            content_type: Some(match kind {
                RawKind::Json => "application/json",
                RawKind::Xml => "application/xml",
                RawKind::Text => "text/plain",
                RawKind::Sparql => "application/sparql-query",
            }),
        },
        Body::FormUrlEncoded { fields } => Payload::Form(
            fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| (ctx.tpl(&f.name), ctx.tpl(&f.value)))
                .collect(),
        ),
        Body::Multipart { parts } => {
            let mut out = Vec::new();
            for p in parts.iter().filter(|p| p.enabled) {
                match p.kind {
                    PartKind::Text => out.push(Part {
                        name: ctx.tpl(&p.name),
                        value: PartValue::Text(
                            ctx.tpl(p.values.first().map_or("", String::as_str)),
                        ),
                    }),
                    PartKind::File => {
                        for v in &p.values {
                            out.push(Part {
                                name: ctx.tpl(&p.name),
                                value: PartValue::File(v.clone()),
                            });
                        }
                    }
                }
            }
            Payload::Multipart(out)
        }
        Body::File { path, .. } => Payload::File(path.clone()),
        Body::GraphQl { query, variables } => {
            let vars: serde_json::Value = if variables.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(variables).unwrap_or_else(|_| {
                    notes.push("The GraphQL variables are not valid JSON and are left out.".into());
                    serde_json::json!({})
                })
            };
            let body = serde_json::json!({ "query": query, "variables": vars });
            Payload::Text {
                data: ctx.tpl(&body.to_string()),
                content_type: Some("application/json"),
            }
        }
    };
    if let Payload::Text {
        content_type: Some(ct),
        ..
    } = &payload
        && !has_header(&headers, "content-type")
    {
        headers.push(("Content-Type".into(), lit(ct)));
    }
    Plan {
        method: if spec.method.trim().is_empty() {
            "GET".into()
        } else {
            spec.method.trim().to_ascii_uppercase()
        },
        url: ctx.tpl(&url_text),
        headers,
        basic,
        payload,
        notes,
    }
}

// ---- curl ----

fn sh(t: &Tpl) -> String {
    if !has_var(t) {
        let text: String = t
            .iter()
            .map(|s| if let Seg::Lit(l) = s { l.as_str() } else { "" })
            .collect();
        return format!("'{}'", text.replace('\'', "'\\''"));
    }
    let mut out = String::from("\"");
    for (i, seg) in t.iter().enumerate() {
        match seg {
            Seg::Lit(l) => {
                for c in l.chars() {
                    if matches!(c, '\\' | '"' | '$' | '`') {
                        out.push('\\');
                    }
                    out.push(c);
                }
            }
            Seg::Var(v) => {
                let glued = matches!(t.get(i + 1), Some(Seg::Lit(n)) if n.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'));
                if glued {
                    out.push_str(&format!("${{{v}}}"));
                } else {
                    out.push_str(&format!("${v}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn curl(p: &Plan, grpc: Option<&str>) -> String {
    let mut args: Vec<String> = Vec::new();
    let mut head = String::from("curl");
    if grpc.is_some() {
        head.push_str(" --http2");
    }
    if p.method == "HEAD" {
        head.push_str(" --head");
    } else if grpc.is_some() {
        head.push_str(" -X POST");
    } else if p.method != "GET" {
        head.push_str(&format!(" -X {}", p.method));
    }
    head.push(' ');
    head.push_str(&sh(&p.url));
    args.push(head);
    for (name, value) in &p.headers {
        args.push(format!(
            "-H {}",
            sh(&cat(lit(&format!("{name}: ")), value.clone()))
        ));
    }
    if let Some((u, pw)) = &p.basic {
        args.push(format!(
            "-u {}",
            sh(&cat(cat(u.clone(), lit(":")), pw.clone()))
        ));
    }
    match &p.payload {
        Payload::None => {}
        Payload::Text { data, .. } => args.push(format!("--data-raw {}", sh(data))),
        Payload::Form(fields) => {
            for (k, v) in fields {
                args.push(format!(
                    "--data-urlencode {}",
                    sh(&cat(cat(k.clone(), lit("=")), v.clone()))
                ));
            }
        }
        Payload::Multipart(parts) => {
            for part in parts {
                let value = match &part.value {
                    PartValue::Text(v) => v.clone(),
                    PartValue::File(f) => lit(&format!("@{f}")),
                };
                args.push(format!(
                    "-F {}",
                    sh(&cat(cat(part.name.clone(), lit("=")), value))
                ));
            }
        }
        Payload::File(path) => {
            args.push(format!("--data-binary {}", sh(&lit(&format!("@{path}")))))
        }
    }
    args.join(" \\\n  ")
}

// ---- JavaScript ----

fn js_str(t: &Tpl) -> String {
    if !has_var(t) {
        let text: String = t
            .iter()
            .map(|s| if let Seg::Lit(l) = s { l.as_str() } else { "" })
            .collect();
        return serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".into());
    }
    let mut out = String::from("`");
    for seg in t {
        match seg {
            Seg::Lit(l) => {
                let mut chars = l.chars().peekable();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' | '`' => {
                            out.push('\\');
                            out.push(c);
                        }
                        '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
                        _ => out.push(c),
                    }
                }
            }
            Seg::Var(v) => out.push_str(&format!("${{{}}}", js_env(v))),
        }
    }
    out.push('`');
    out
}

fn js_env(v: &str) -> String {
    if v.starts_with(|c: char| c.is_ascii_digit()) {
        format!("process.env[\"{v}\"]")
    } else {
        format!("process.env.{v}")
    }
}

fn fetch(p: &Plan) -> String {
    let mut pre = String::new();
    let mut lines: Vec<String> = vec![format!(
        "  method: {},",
        serde_json::to_string(&p.method).unwrap_or_default()
    )];
    let mut headers: Vec<(String, String)> = p
        .headers
        .iter()
        .map(|(n, v)| (serde_json::to_string(n).unwrap_or_default(), js_str(v)))
        .collect();
    if let Some((u, pw)) = &p.basic {
        let creds = js_str(&cat(cat(u.clone(), lit(":")), pw.clone()));
        headers.push((
            "\"Authorization\"".into(),
            format!("`Basic ${{Buffer.from({creds}).toString(\"base64\")}}`"),
        ));
    }
    if !headers.is_empty() {
        lines.push("  headers: {".into());
        for (n, v) in headers {
            lines.push(format!("    {n}: {v},"));
        }
        lines.push("  },".into());
    }
    match &p.payload {
        Payload::None => {}
        Payload::Text { data, .. } => lines.push(format!("  body: {},", js_str(data))),
        Payload::Form(fields) => {
            let pairs: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("[{}, {}]", js_str(k), js_str(v)))
                .collect();
            lines.push(format!(
                "  body: new URLSearchParams([{}]),",
                pairs.join(", ")
            ));
        }
        Payload::Multipart(parts) => {
            pre.push_str("const form = new FormData();\n");
            for part in parts {
                match &part.value {
                    PartValue::Text(v) => pre.push_str(&format!(
                        "form.append({}, {});\n",
                        js_str(&part.name),
                        js_str(v)
                    )),
                    PartValue::File(f) => pre.push_str(&format!(
                        "form.append({}, new Blob([await readFile({})]), {});\n",
                        js_str(&part.name),
                        serde_json::to_string(f).unwrap_or_default(),
                        serde_json::to_string(f.rsplit('/').next().unwrap_or(f))
                            .unwrap_or_default(),
                    )),
                }
            }
            pre.push('\n');
            lines.push("  body: form,".into());
        }
        Payload::File(path) => {
            lines.push(format!(
                "  body: await readFile({}),",
                serde_json::to_string(path).unwrap_or_default()
            ));
        }
    }
    let needs_fs = matches!(&p.payload, Payload::File(_))
        || matches!(&p.payload, Payload::Multipart(parts) if parts.iter().any(|x| matches!(x.value, PartValue::File(_))));
    let mut out = String::new();
    if needs_fs {
        out.push_str("import { readFile } from \"node:fs/promises\";\n\n");
    }
    out.push_str(&pre);
    out.push_str(&format!(
        "const response = await fetch({}, {{\n",
        js_str(&p.url)
    ));
    out.push_str(&lines.join("\n"));
    out.push_str("\n});\nconsole.log(response.status);\nconsole.log(await response.text());\n");
    out
}

// ---- Python ----

fn py_str(t: &Tpl) -> String {
    let part = |s: &Seg| match s {
        Seg::Lit(l) => serde_json::to_string(l).unwrap_or_else(|_| "\"\"".into()),
        Seg::Var(v) => format!("os.environ[\"{v}\"]"),
    };
    if t.is_empty() {
        return "\"\"".into();
    }
    t.iter().map(part).collect::<Vec<_>>().join(" + ")
}

fn python(p: &Plan) -> String {
    let uses_env = [&p.url]
        .into_iter()
        .chain(p.headers.iter().map(|(_, v)| v))
        .any(has_var)
        || p.basic.iter().any(|(u, w)| has_var(u) || has_var(w))
        || match &p.payload {
            Payload::Text { data, .. } => has_var(data),
            Payload::Form(f) => f.iter().any(|(k, v)| has_var(k) || has_var(v)),
            Payload::Multipart(parts) => parts
                .iter()
                .any(|x| has_var(&x.name) || matches!(&x.value, PartValue::Text(t) if has_var(t))),
            _ => false,
        };
    let mut out = String::new();
    if uses_env {
        out.push_str("import os\n");
    }
    out.push_str("import requests\n\n");
    let call = match p.method.as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => {
            format!("requests.{}(", p.method.to_ascii_lowercase())
        }
        other => format!(
            "requests.request({}, ",
            serde_json::to_string(other).unwrap_or_default()
        ),
    };
    let mut args: Vec<String> = vec![format!("    {}", py_str(&p.url))];
    if !p.headers.is_empty() {
        let items: Vec<String> = p
            .headers
            .iter()
            .map(|(n, v)| {
                format!(
                    "        {}: {},",
                    serde_json::to_string(n).unwrap_or_default(),
                    py_str(v)
                )
            })
            .collect();
        args.push(format!("    headers={{\n{}\n    }}", items.join("\n")));
    }
    if let Some((u, w)) = &p.basic {
        args.push(format!("    auth=({}, {})", py_str(u), py_str(w)));
    }
    match &p.payload {
        Payload::None => {}
        Payload::Text { data, .. } => args.push(format!("    data={}", py_str(data))),
        Payload::Form(fields) => {
            let items: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("({}, {})", py_str(k), py_str(v)))
                .collect();
            args.push(format!("    data=[{}]", items.join(", ")));
        }
        Payload::Multipart(parts) => {
            let items: Vec<String> = parts
                .iter()
                .map(|x| match &x.value {
                    PartValue::Text(v) => format!("({}, (None, {}))", py_str(&x.name), py_str(v)),
                    PartValue::File(f) => format!(
                        "({}, open({}, \"rb\"))",
                        py_str(&x.name),
                        serde_json::to_string(f).unwrap_or_default()
                    ),
                })
                .collect();
            args.push(format!("    files=[{}]", items.join(", ")));
        }
        Payload::File(path) => args.push(format!(
            "    data=open({}, \"rb\")",
            serde_json::to_string(path).unwrap_or_default()
        )),
    }
    out.push_str(&format!("response = {call}\n{},\n)\n", args.join(",\n")));
    out.push_str("print(response.status_code)\nprint(response.text)\n");
    out
}

/// `spec` as `kind`. `environment` supplies the values "Resolve variables" puts in (never
/// those of secret variables or of names that look like credentials).
pub fn copy_spec(
    spec: &RequestSpec,
    kind: CopyKind,
    environment: Option<&Environment>,
    options: &CopyOptions,
) -> Result<CopiedAs, ShareError> {
    let unavailable = || ShareError::Unavailable("not available for this request".to_owned());
    let grpc = match &spec.protocol {
        Protocol::WebSocket(_) => return Err(unavailable()),
        Protocol::Grpc(g) => {
            if kind != CopyKind::Curl {
                return Err(unavailable());
            }
            Some(g.method.clone())
        }
        Protocol::Http => None,
    };
    let clean = sanitize_spec(spec);
    let mut resolve = BTreeMap::new();
    if options.resolve_variables {
        let own = spec.vars.iter().filter(|v| v.enabled);
        let env = environment
            .into_iter()
            .flat_map(|e| e.variables.iter())
            .filter(|v| v.enabled);
        // The request's own variables win over the environment's.
        for v in env.chain(own) {
            if let Some(value) = v
                .value
                .as_ref()
                .filter(|_| !v.secret && !is_sensitive_name(&v.name))
            {
                resolve.insert(v.name.clone(), value.clone());
            }
        }
    }
    let mut ctx = Ctx {
        resolve,
        vars: Vec::new(),
    };
    let mut p = plan(&clean.spec, &mut ctx);
    let text = match kind {
        CopyKind::Curl => match &grpc {
            Some(method) => {
                p.method = "POST".into();
                p.url = grpc_url(&p.url, method);
                p.headers
                    .push(("Content-Type".into(), lit("application/grpc")));
                p.headers.push(("TE".into(), lit("trailers")));
                p.payload = Payload::File("message.bin".into());
                p.notes.push(
                    "gRPC needs a framed protobuf message: put it in message.bin (rt-grpc can encode the JSON message)."
                        .into(),
                );
                curl(&p, grpc.as_deref())
            }
            None => curl(&p, None),
        },
        CopyKind::Fetch => fetch(&p),
        CopyKind::Python => python(&p),
    };
    let mut notes = p.notes;
    for slug in &clean.placeholders {
        notes.push(format!(
            "A literal value for {slug} was replaced by the variable {}.",
            env_name(slug)
        ));
    }
    if !ctx.vars.is_empty() {
        notes.push(format!(
            "Set these environment variables before you run it: {}.",
            ctx.vars.join(", ")
        ));
    }
    // The second net, like in a link: the scanner reads what the snippet is made from (a
    // snippet itself is code, which the scanner is not made for).
    let source = serde_json::to_string_pretty(&clean.spec).unwrap_or_default();
    let findings = crate::secret_scan::scan(&[("request.json", &source)]);
    if !findings.is_empty() {
        return Err(ShareError::SecretsFound(crate::secret_scan::refusal(
            &findings,
        )));
    }
    Ok(CopiedAs { kind, text, notes })
}

/// The URL of a gRPC call over HTTP/2: `grpc://` is `http://`, `grpcs://` and a bare host are
/// `https://`, and the method is the path.
fn grpc_url(url: &Tpl, method: &str) -> Tpl {
    let mut out = url.clone();
    if let Some(Seg::Lit(first)) = out.first_mut() {
        if let Some(rest) = first.strip_prefix("grpcs://") {
            *first = format!("https://{rest}");
        } else if let Some(rest) = first.strip_prefix("grpc://") {
            *first = format!("http://{rest}");
        } else if !first.contains("://") {
            *first = format!("https://{first}");
        }
    }
    let path = if method.starts_with('/') {
        method.to_owned()
    } else {
        format!("/{method}")
    };
    out.push(Seg::Lit(path));
    out
}
