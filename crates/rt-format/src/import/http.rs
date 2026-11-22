// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for `.http` / `.rest` files: the VS Code REST Client and the JetBrains HTTP
//! Client syntax, with JetBrains' `http-client.env.json` and `http-client.private.env.json`.
//! Owner: wave 4, strand B.
//!
//! Requests are separated by `###` (its text names the request, as in JetBrains) and named
//! by `# @name` (VS Code) or else `METHOD path`. File variables (`@host = example.com`)
//! become collection variables (folder variables when several files are imported, one folder
//! per file). `Authorization: Basic user:password` (which the REST Client encodes itself)
//! becomes Basic auth. Response handlers, pre-request scripts and output redirection are
//! reported; so are system variables (`{{$guid}}`, `{{$processEnv X}}`, ...) and references
//! to other requests' responses, which stay as text.
//!
//! Environments: every top-level object of `http-client.env.json` is an environment; values
//! of the private file are secrets and never written.

use rt_core::model::{Auth, Body, Environment, KeyValue, RawKind, RequestSpec, Settings, Variable};

use super::auth;
use super::common::{Losses, finish, invalid, location, raw_kind, url_params, utf8};
use super::out::{AuthOut, CollectionOut, FolderOut, ItemOut, RequestOut};
use super::value::parse_json;
use super::{Import, ImportError, ImportFormat, LossKind, SourceFile, base_name, stem};

const F: ImportFormat = ImportFormat::Http;

pub(crate) const METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

/// Request methods of other protocols in JetBrains' syntax.
const OTHER_PROTOCOLS: &[&str] = &["WEBSOCKET", "GRPC", "GRAPHQL"];

pub(super) fn import(
    files: &[SourceFile<'_>],
    envs: &[SourceFile<'_>],
) -> Result<Import, ImportError> {
    let mut losses = Losses::default();
    let multi = files.len() > 1;
    let mut collection = CollectionOut::default();
    for f in files {
        let text = utf8(F, f.data)?;
        let text: &str = &text;
        let file_stem = stem(f.name, &[".http", ".rest"]).to_owned();
        let folder: Vec<String> = if multi {
            vec![file_stem.clone()]
        } else {
            Vec::new()
        };
        let parsed = parse_file(text, &folder, &mut losses);
        let var_here = if multi {
            file_stem.clone()
        } else {
            "collection".to_owned()
        };
        let values: Vec<&str> = parsed
            .vars
            .iter()
            .filter_map(|v| v.value.as_deref())
            .collect();
        let refs = response_refs(values.iter().copied());
        if !refs.is_empty() {
            losses.approximated(
                &var_here,
                LossKind::Variable,
                format!(
                    "file variables refer to other requests' responses, kept as text: {}",
                    refs.join(", ")
                ),
            );
        }
        losses.dynamic_vars(&var_here, values);
        if multi {
            collection.items.push(ItemOut::Folder(FolderOut {
                name: file_stem,
                defaults: super::out::DefaultsOut {
                    vars: parsed.vars,
                    ..Default::default()
                },
                items: parsed.items,
                ..FolderOut::default()
            }));
        } else {
            collection.defaults.vars = parsed.vars;
            collection.items = parsed.items;
        }
    }
    let first = files
        .first()
        .map(|f| stem(f.name, &[".http", ".rest"]))
        .unwrap_or("");
    collection.name = if multi || first.is_empty() || first.eq_ignore_ascii_case("pasted") {
        "HTTP requests".into()
    } else {
        first.to_owned()
    };
    if !collection.items.iter().any(|i| match i {
        ItemOut::Request(_) => true,
        ItemOut::Folder(f) => !f.items.is_empty(),
    }) {
        return Err(invalid(F, "no request found"));
    }
    // Public environments first, then the private (secret) values on top.
    let mut sorted: Vec<&SourceFile<'_>> = envs.iter().collect();
    sorted.sort_by_key(|f| base_name(f.name).to_ascii_lowercase().contains("private"));
    for env in sorted {
        environments(env, &mut collection, &mut losses)?;
    }
    finish(F, collection, losses)
}

struct Parsed {
    vars: Vec<Variable>,
    items: Vec<ItemOut>,
}

/// One block between `###` separators.
#[derive(Default)]
struct Block<'a> {
    title: Option<String>,
    lines: Vec<&'a str>,
}

fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out = vec![Block::default()];
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("###") {
            let title = rest.trim();
            out.push(Block {
                title: (!title.is_empty()).then(|| title.to_owned()),
                lines: Vec::new(),
            });
        } else {
            out.last_mut().expect("never empty").lines.push(line);
        }
    }
    out
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('#') || t.starts_with("//")
}

/// `# @name value` / `// @name value`.
fn meta(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let t = t
        .strip_prefix('#')
        .or_else(|| t.strip_prefix("//"))?
        .trim_start();
    let t = t.strip_prefix('@')?;
    let (k, v) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
    let v = v.trim();
    let v = v.strip_prefix('=').map(str::trim).unwrap_or(v);
    Some((k, v))
}

/// `@name = value` file variable.
fn file_var(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    Some((name.to_owned(), value.trim().to_owned()))
}

fn request_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    let mut words = t.split_whitespace();
    let first = words.next()?;
    let upper = first.to_ascii_uppercase();
    if METHODS.contains(&upper.as_str()) || OTHER_PROTOCOLS.contains(&upper.as_str()) {
        let rest = t[first.len()..].trim();
        let url = rest.strip_suffix_http_version().trim().to_owned();
        return Some((upper, url));
    }
    if first.starts_with("http://") || first.starts_with("https://") || first.starts_with("{{") {
        return Some((
            "GET".into(),
            t.strip_suffix_http_version().trim().to_owned(),
        ));
    }
    None
}

trait HttpVersion {
    fn strip_suffix_http_version(&self) -> &str;
}

impl HttpVersion for str {
    /// The request target without a trailing `HTTP/1.1` (or `HTTP/2`).
    fn strip_suffix_http_version(&self) -> &str {
        match self.rsplit_once(char::is_whitespace) {
            Some((head, v)) if v.starts_with("HTTP/") => head,
            _ => self,
        }
    }
}

fn parse_file(text: &str, folder: &[String], losses: &mut Losses) -> Parsed {
    let mut vars: Vec<Variable> = Vec::new();
    let mut items = Vec::new();
    for block in blocks(text) {
        let mut name: Option<String> = block.title.clone();
        let mut no_redirect = false;
        let mut timeout_ms = None;
        let mut pre_script = String::new();
        let mut prompts = Vec::new();
        let mut i = 0;
        let lines = &block.lines;
        // Variables, comments and metadata before the request line.
        let mut start = None;
        while i < lines.len() {
            let line = lines[i];
            if line.trim().is_empty() {
                i += 1;
                continue;
            }
            if let Some((k, v)) = meta(line) {
                match k {
                    "name" if !v.is_empty() => name = Some(v.to_owned()),
                    "no-redirect" => no_redirect = true,
                    "prompt" => prompts.push(v.split_whitespace().next().unwrap_or("").to_owned()),
                    "timeout" if parse_timeout(v).is_some() => timeout_ms = parse_timeout(v),
                    "note" | "title" | "no-log" | "no-cookie-jar" | "no-auto-encoding"
                    | "timeout" | "connection-timeout" => {
                        let here = location(folder, name.as_deref().unwrap_or("file"));
                        if k == "note" || k == "title" || k == "no-log" {
                            // Display hints only.
                        } else {
                            losses.dropped(
                                &here,
                                LossKind::Setting,
                                format!("`@{k}` not imported"),
                            );
                        }
                    }
                    _ => {}
                }
                i += 1;
                continue;
            }
            if is_comment(line) {
                i += 1;
                continue;
            }
            // JetBrains pre-request script: `< {% ... %}` before the request line.
            if let Some(rest) = line.trim_start().strip_prefix('<')
                && rest.trim_start().starts_with("{%")
            {
                let mut code = rest.trim_start()[2..].to_owned();
                while !code.contains("%}") && i + 1 < lines.len() {
                    i += 1;
                    code.push('\n');
                    code.push_str(lines[i]);
                }
                if let Some(end) = code.find("%}") {
                    code.truncate(end);
                }
                pre_script.push_str(&code);
                pre_script.push('\n');
                i += 1;
                continue;
            }
            if let Some((n, v)) = file_var(line) {
                vars.retain(|x| x.name != n);
                vars.push(Variable::new(n, v));
                i += 1;
                continue;
            }
            if request_line(line).is_some() {
                start = Some(i);
            }
            break;
        }
        let Some(start) = start else {
            // A block with only variables or comments, or text that is no request.
            if let Some(line) = lines[i.min(lines.len())..]
                .iter()
                .find(|l| !l.trim().is_empty())
            {
                let here = location(folder, name.as_deref().unwrap_or("file"));
                losses.dropped(
                    &here,
                    LossKind::Other,
                    format!(
                        "text that is not a request not imported: `{}`",
                        line.trim().chars().take(60).collect::<String>()
                    ),
                );
            }
            continue;
        };
        let (method, mut url) = request_line(lines[start]).expect("checked above");
        i = start + 1;
        // Multi-line query: lines starting with `?` or `&`.
        while i < lines.len() {
            let t = lines[i].trim();
            if t.starts_with('?') || t.starts_with('&') {
                url.push_str(t);
                i += 1;
            } else {
                break;
            }
        }
        let mut headers = Vec::new();
        while i < lines.len() && !lines[i].trim().is_empty() {
            let line = lines[i];
            i += 1;
            if is_comment(line) {
                continue;
            }
            if let Some((n, v)) = line.split_once(':') {
                headers.push(KeyValue::new(n.trim(), v.trim()));
            }
        }
        let body_lines: Vec<&str> = lines.get(i..).unwrap_or_default().to_vec();
        let req_name = name.unwrap_or_else(|| default_name(&method, &url));
        let here = location(folder, &req_name);
        if OTHER_PROTOCOLS.contains(&method.as_str()) {
            losses.dropped(
                &here,
                LossKind::Protocol,
                format!("{method} request not imported"),
            );
            continue;
        }
        losses.script(&here, "pre-request script", &pre_script);
        let mut r = request(
            &method,
            url,
            headers,
            &body_lines,
            &here,
            no_redirect,
            &prompts,
            losses,
            req_name,
        );
        r.spec.settings.timeout_ms = timeout_ms;
        items.push(ItemOut::Request(r));
    }
    Parsed { vars, items }
}

/// JetBrains `@timeout`: seconds, or a number with `ms`, `s` or `m`.
fn parse_timeout(v: &str) -> Option<u64> {
    let v = v.trim();
    let digits = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    let n: u64 = v[..digits].parse().ok()?;
    let factor = match v[digits..].trim() {
        "" | "s" => 1000,
        "ms" => 1,
        "m" => 60_000,
        _ => return None,
    };
    n.checked_mul(factor)
}

fn default_name(method: &str, url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or(rest)
        .trim_end_matches('/');
    format!("{method} {}", rest.chars().take(120).collect::<String>())
}

#[allow(clippy::too_many_arguments)]
fn request(
    method: &str,
    url: String,
    mut headers: Vec<KeyValue>,
    body_lines: &[&str],
    here: &str,
    no_redirect: bool,
    prompts: &[String],
    losses: &mut Losses,
    name: String,
) -> RequestOut {
    // Body until a response handler (`> {% %}`, `> file.js`) or redirection (`>> file`).
    let mut body = Vec::new();
    let mut k = 0;
    while k < body_lines.len() {
        let line = body_lines[k];
        let t = line.trim_start();
        if t.starts_with(">>") {
            losses.dropped(
                here,
                LossKind::Setting,
                format!("response redirection `{}` not imported", t.trim()),
            );
            k += 1;
            continue;
        }
        if t.starts_with("> ") || t == ">" || t.starts_with(">{%") {
            let (code, next) = script_block(body_lines, k, '>');
            losses.script(here, "response handler script", &code);
            k = next;
            continue;
        }
        if t.starts_with("< {%") || t.starts_with("<{%") {
            let (code, next) = script_block(body_lines, k, '<');
            losses.script(here, "pre-request script", &code);
            k = next;
            continue;
        }
        body.push(line);
        k += 1;
    }
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    while body.first().is_some_and(|l| l.trim().is_empty()) {
        body.remove(0);
    }
    let content_type = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.clone())
        .unwrap_or_default();
    let body = if body.is_empty() {
        Body::None
    } else if body.len() == 1 && body[0].trim_start().starts_with('<') {
        let file = body[0].trim_start()[1..]
            .trim_start_matches('@')
            .trim()
            .to_owned();
        losses.approximated(
            here,
            LossKind::Body,
            format!("body file `{file}` is read relative to the collection folder; copy it there"),
        );
        Body::File {
            path: file,
            content_type: (!content_type.is_empty()).then_some(content_type.clone()),
        }
    } else {
        let text = body.join("\n");
        if content_type.to_ascii_lowercase().contains("multipart/") {
            losses.approximated(
                here,
                LossKind::Body,
                "multipart body kept as text; file parts (`< file`) are not read",
            );
        }
        if body.iter().any(|l| l.trim_start().starts_with("< "))
            && !content_type.contains("multipart/")
        {
            losses.approximated(
                here,
                LossKind::Body,
                "file references inside the body are kept as text",
            );
        }
        Body::Raw {
            kind: if content_type.is_empty() {
                RawKind::Text
            } else {
                raw_kind(&content_type)
            },
            data: text,
        }
    };
    // `Authorization: Basic user:password` / `Basic user password` / `Digest user password`.
    let mut auth = AuthOut::Model(Auth::None);
    if let Some(pos) = headers
        .iter()
        .position(|h| h.name.eq_ignore_ascii_case("authorization"))
    {
        let value = headers[pos].value.clone();
        let mut words = value.split_whitespace();
        let scheme = words.next().unwrap_or("").to_ascii_lowercase();
        let rest: Vec<&str> = words.collect();
        let pair = match rest.as_slice() {
            [user, password] => Some(((*user).to_owned(), (*password).to_owned())),
            [one] if one.contains(':') && !one.contains("{{") => {
                let (u, p) = one.split_once(':').expect("checked");
                Some((u.to_owned(), p.to_owned()))
            }
            _ => None,
        };
        if let Some((username, password)) = pair {
            if scheme == "basic" {
                auth = AuthOut::Model(Auth::Basic { username, password });
                headers.remove(pos);
            } else if scheme == "digest" {
                auth = auth::user_password("digest", &username, &password);
                headers.remove(pos);
                auth::report(losses, here, &auth, "Digest auth");
            }
        }
    }
    let mut texts: Vec<&str> = vec![&url];
    texts.extend(headers.iter().map(|h| h.value.as_str()));
    if let Body::Raw { data, .. } = &body {
        texts.push(data);
    }
    losses.dynamic_vars(here, texts.iter().copied());
    let refs = response_refs(texts.iter().copied());
    if !refs.is_empty() {
        losses.approximated(
            here,
            LossKind::Variable,
            format!(
                "references to other requests' responses kept as text: {}",
                refs.join(", ")
            ),
        );
    }
    if !prompts.is_empty() {
        losses.approximated(
            here,
            LossKind::Variable,
            format!(
                "prompt {} {} not asked for; set {} as variables before sending",
                if prompts.len() == 1 {
                    "variable"
                } else {
                    "variables"
                },
                prompts
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if prompts.len() == 1 { "it" } else { "them" }
            ),
        );
    }
    let model_auth = match &auth {
        AuthOut::Model(a) => a.clone(),
        AuthOut::Other { .. } => Auth::None,
    };
    losses.literal_credentials(here, &model_auth, &headers);
    let spec = RequestSpec {
        name,
        seq: None,
        method: method.to_owned(),
        params: url_params(&url, &[]),
        url,
        headers,
        body,
        auth: Auth::None,
        vars: Vec::new(),
        settings: Settings {
            follow_redirects: no_redirect.then_some(false),
            ..Settings::default()
        },
    };
    let mut r = RequestOut::new(spec);
    r.auth = auth;
    r
}

/// The text of a `> {% … %}` / `< {% … %}` script (or a script file reference) starting at
/// line `k`, and the line after it.
fn script_block(lines: &[&str], k: usize, marker: char) -> (String, usize) {
    let first = lines[k]
        .trim_start()
        .trim_start_matches(marker)
        .trim_start();
    if !first.starts_with("{%") {
        // `> ./handler.js`: a file reference.
        return (format!("{first}\n"), k + 1);
    }
    let mut code = Vec::new();
    let after = &first[2..];
    if let Some(end) = after.find("%}") {
        code.push(&after[..end]);
        return (code.join("\n"), k + 1);
    }
    code.push(after);
    let mut j = k + 1;
    while j < lines.len() {
        if let Some(end) = lines[j].find("%}") {
            code.push(&lines[j][..end]);
            return (code.join("\n"), j + 1);
        }
        code.push(lines[j]);
        j += 1;
    }
    (code.join("\n"), j)
}

/// `{{name.response...}}` / `{{name.request...}}` placeholders.
fn response_refs<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in texts {
        let mut rest = text;
        while let Some(s) = rest.find("{{") {
            rest = &rest[s + 2..];
            let Some(e) = rest.find("}}") else { break };
            let inner = rest[..e].trim();
            if (inner.contains(".response.") || inner.contains(".request."))
                && !out.iter().any(|o| o == inner)
            {
                out.push(format!("{{{{{inner}}}}}"));
            }
            rest = &rest[e..];
        }
    }
    out
}

fn environments(
    f: &SourceFile<'_>,
    c: &mut CollectionOut,
    losses: &mut Losses,
) -> Result<(), ImportError> {
    let private = base_name(f.name).to_ascii_lowercase().contains("private");
    let v = parse_json(&utf8(F, f.data)?)
        .map_err(|e| invalid(F, format!("{}: {e}", base_name(f.name))))?;
    if !v.is_obj() {
        return Err(invalid(
            F,
            format!("{}: expected a JSON object", base_name(f.name)),
        ));
    }
    for (env_name, vars) in v.entries() {
        if env_name.trim().is_empty() || env_name.chars().any(char::is_control) {
            continue;
        }
        if env_name == "$shared" {
            // JetBrains: available in every environment, which may override them.
            for (name, value) in vars.entries() {
                if value.is_obj() || value.is_arr() {
                    continue;
                }
                c.defaults.vars.retain(|x| x.name != *name);
                c.defaults.vars.push(Variable::new(
                    name.clone(),
                    value.text().unwrap_or_default(),
                ));
            }
            losses.approximated(
                "environment $shared",
                LossKind::Variable,
                "shared environment values imported as collection variables",
            );
            continue;
        }
        let here = format!("environment {env_name}");
        let index = match c.environments.iter().position(|e| e.name == *env_name) {
            Some(i) => i,
            None => {
                c.environments.push(Environment {
                    client_certificates: Vec::new(),
                    external_secrets: None,
                    name: env_name.clone(),
                    variables: Vec::new(),
                });
                c.environments.len() - 1
            }
        };
        for (name, value) in vars.entries() {
            if value.is_obj() || value.is_arr() {
                losses.dropped(
                    &here,
                    LossKind::Setting,
                    format!("`{name}` (an object, e.g. auth or SSL configuration) not imported"),
                );
                continue;
            }
            let text = value.text().unwrap_or_default();
            let var = if private {
                if !text.is_empty() {
                    losses.secret_value(&here, name);
                }
                Variable {
                    name: name.clone(),
                    value: None,
                    enabled: true,
                    secret: true,
                }
            } else {
                Variable::new(name.clone(), text)
            };
            let env = &mut c.environments[index];
            env.variables.retain(|x| x.name != *name);
            env.variables.push(var);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_lines() {
        assert_eq!(
            request_line("GET https://x/y HTTP/1.1"),
            Some(("GET".into(), "https://x/y".into()))
        );
        assert_eq!(
            request_line("https://x"),
            Some(("GET".into(), "https://x".into()))
        );
        assert_eq!(
            request_line("post {{host}}/a"),
            Some(("POST".into(), "{{host}}/a".into()))
        );
        assert_eq!(request_line("Content-Type: x"), None);
    }

    #[test]
    fn metadata() {
        assert_eq!(meta("# @name login"), Some(("name", "login")));
        assert_eq!(meta("// @name = x"), Some(("name", "x")));
        assert_eq!(
            file_var("@host = example.com"),
            Some(("host".into(), "example.com".into()))
        );
    }
}
