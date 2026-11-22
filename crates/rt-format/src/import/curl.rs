// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Importer for `curl` command lines (one or more per file; bash, zsh and Windows `cmd`
//! quoting as browsers' "Copy as cURL" writes them). Owner: wave 4, strand B.
//!
//! Every command becomes one request (several files: one folder per file). The request does
//! what curl would do: without `-L` redirects are not followed, `-d` sends a form body unless
//! a `Content-Type` says otherwise, `-G` moves the data into the query string. Options that
//! only shape curl's output (`-s`, `-v`, `-o`, ...) are ignored; options that change the
//! request but have no counterpart (proxy, certificates, `-k`, ...) are reported. Shell
//! variables (`$TOKEN`) become `{{TOKEN}}` placeholders.

use rt_core::model::{
    Auth, Body, KeyValue, MultipartPart, PartKind, RawKind, RequestSpec, Settings,
};

use super::auth;
use super::common::{Losses, finish, invalid, location, plural, raw_kind, url_params, utf8};
use super::out::{AuthOut, CollectionOut, FolderOut, ItemOut, RequestOut};
use super::{Import, ImportError, ImportFormat, LossKind, SourceFile, base_name, stem};

const F: ImportFormat = ImportFormat::Curl;

/// Most commands accepted in one import.
const MAX_COMMANDS: usize = 10_000;

pub(super) fn import(files: &[SourceFile<'_>]) -> Result<Import, ImportError> {
    let mut losses = Losses::default();
    let mut collection = CollectionOut::default();
    let multi = files.len() > 1;
    let mut total = 0;
    let mut requests = 0;
    for f in files {
        let text = utf8(F, f.data)?;
        let text: &str = &text;
        let commands =
            split_commands(text).map_err(|e| invalid(F, format!("{}: {e}", base_name(f.name))))?;
        let curls: Vec<&Vec<String>> = commands
            .iter()
            .filter(|c| c.first().is_some_and(|w| is_curl(w)))
            .collect();
        total += curls.len();
        if total > MAX_COMMANDS {
            return Err(invalid(F, format!("more than {MAX_COMMANDS} commands")));
        }
        let folder = if multi {
            vec![stem(f.name, &[".curl", ".txt", ".sh", ".bat", ".cmd"]).to_owned()]
        } else {
            Vec::new()
        };
        for c in &commands {
            if let Some(w) = c.first().filter(|w| !is_curl(w)) {
                losses.dropped(
                    &location(&folder, base_name(f.name)),
                    LossKind::Other,
                    format!("`{w}` is not a curl command; not imported"),
                );
            }
        }
        let mut items = Vec::new();
        for words in curls {
            let r = command(&words[1..], &folder, &mut losses);
            if r.spec.url.is_empty() {
                // Losses of a request that is not imported would only confuse.
                losses
                    .0
                    .retain(|l| l.location != location(&folder, &r.spec.name));
                losses.dropped(
                    &location(&folder, base_name(f.name)),
                    LossKind::Other,
                    "a curl command without URL not imported",
                );
                continue;
            }
            requests += 1;
            items.push(ItemOut::Request(r));
        }
        if multi {
            collection.items.push(ItemOut::Folder(FolderOut {
                name: folder[0].clone(),
                items,
                ..FolderOut::default()
            }));
        } else {
            collection.items = items;
        }
    }
    if total == 0 {
        return Err(invalid(F, "no curl command found"));
    }
    if requests == 0 {
        return Err(invalid(F, "no curl command with a URL found"));
    }
    let first_name = files
        .first()
        .map(|f| stem(f.name, &[".curl", ".txt", ".sh", ".bat", ".cmd"]))
        .unwrap_or("");
    collection.name = if multi || first_name.is_empty() || first_name.eq_ignore_ascii_case("pasted")
    {
        first_host(&collection.items).unwrap_or_else(|| "cURL import".into())
    } else {
        first_name.to_owned()
    };
    finish(F, collection, losses)
}

fn is_curl(word: &str) -> bool {
    let w = word.rsplit(['/', '\\']).next().unwrap_or(word);
    w == "curl" || w.eq_ignore_ascii_case("curl.exe")
}

fn first_host(items: &[ItemOut]) -> Option<String> {
    for item in items {
        match item {
            ItemOut::Request(r) => {
                let url = &r.spec.url;
                let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
                let host = rest.split(['/', '?', '#']).next().unwrap_or("");
                if !host.is_empty() {
                    return Some(host.to_owned());
                }
            }
            ItemOut::Folder(f) => {
                if let Some(h) = first_host(&f.items) {
                    return Some(h);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Shell words

/// Splits text into commands of words, with POSIX shell quoting (`'…'`, `"…"`, `$'…'`,
/// backslash escapes and line continuations) or, if the text uses `^` continuations, the
/// quoting of Windows `cmd`. Commands end at unquoted line breaks, `;`, `&&`, `||` and `|`.
pub(crate) fn split_commands(text: &str) -> Result<Vec<Vec<String>>, String> {
    if text.contains("^\n") || text.contains("^\r\n") || text.contains("^\"") {
        return split_cmd(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut commands = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut i = 0;
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => {
                end_word(&mut word, &mut in_word, &mut words);
                i += 1;
            }
            '\n' | ';' => {
                end_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
                i += 1;
            }
            '&' | '|' => {
                end_word(&mut word, &mut in_word, &mut words);
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
                i += 1;
                if chars.get(i) == Some(&c) {
                    i += 1;
                }
            }
            '#' if !in_word => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\\' => {
                i += 1;
                match chars.get(i) {
                    Some('\r') if chars.get(i + 1) == Some(&'\n') => i += 2,
                    Some('\n') => i += 1,
                    Some(&n) => {
                        word.push(n);
                        in_word = true;
                        i += 1;
                    }
                    None => {}
                }
            }
            '\'' => {
                in_word = true;
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated single quote".into()),
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            word.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '$' if chars.get(i + 1) == Some(&'\'') => {
                in_word = true;
                i += 2;
                i = ansi_c(&chars, i, &mut word)?;
            }
            '$' if is_var_start(chars.get(i + 1)) => {
                in_word = true;
                i = shell_var(&chars, i, &mut word);
            }
            '"' => {
                in_word = true;
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated double quote".into()),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => match chars.get(i + 1) {
                            Some('\n') => i += 2,
                            Some(&e @ ('"' | '\\' | '$' | '`')) => {
                                word.push(e);
                                i += 2;
                            }
                            _ => {
                                word.push('\\');
                                i += 1;
                            }
                        },
                        Some('$') if is_var_start(chars.get(i + 1)) => {
                            i = shell_var(&chars, i, &mut word);
                        }
                        Some(&ch) => {
                            word.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            c => {
                word.push(c);
                in_word = true;
                i += 1;
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    if !words.is_empty() {
        commands.push(words);
    }
    Ok(commands)
}

/// Windows `cmd` quoting ("Copy as cURL (cmd)"): `^` escapes the next character and continues
/// lines; the program then splits its command line by the C runtime's rules (`"` groups,
/// `\"` is a literal quote).
fn split_cmd(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            unescaped.push(c);
            continue;
        }
        match chars.next() {
            Some('\r') if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            Some('\n') | None => {}
            Some(n) => unescaped.push(n),
        }
    }
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut chars = unescaped.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                word.push('"');
                in_word = true;
            }
            ' ' | '\t' | '\r' | '\n' | '&' | '|' if !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                if !matches!(c, ' ' | '\t' | '\r') && !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if quoted {
        return Err("unterminated double quote".into());
    }
    if in_word {
        words.push(word);
    }
    if !words.is_empty() {
        commands.push(words);
    }
    Ok(commands)
}

fn is_var_start(c: Option<&char>) -> bool {
    matches!(c, Some(c) if c.is_ascii_alphabetic() || *c == '_' || *c == '{')
}

/// `$NAME` or `${NAME}` at `i` → `{{NAME}}`. Returns the position after it.
fn shell_var(chars: &[char], mut i: usize, out: &mut String) -> usize {
    i += 1;
    let braced = chars.get(i) == Some(&'{');
    if braced {
        i += 1;
    }
    let start = i;
    while chars
        .get(i)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
    {
        i += 1;
    }
    let name: String = chars[start..i].iter().collect();
    if braced {
        if chars.get(i) == Some(&'}') && !name.is_empty() {
            i += 1;
        } else {
            out.push_str("${");
            out.push_str(&name);
            return i;
        }
    }
    out.push_str("{{");
    out.push_str(&name);
    out.push_str("}}");
    i
}

/// The body of `$'…'` from `i` (after the quote). Returns the position after the closing quote.
fn ansi_c(chars: &[char], mut i: usize, out: &mut String) -> Result<usize, String> {
    loop {
        match chars.get(i) {
            None => return Err("unterminated $'…' string".into()),
            Some('\'') => return Ok(i + 1),
            Some('\\') => {
                i += 1;
                let Some(&e) = chars.get(i) else {
                    return Err("unterminated $'…' string".into());
                };
                i += 1;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'a' => out.push('\u{7}'),
                    'b' => out.push('\u{8}'),
                    'e' | 'E' => out.push('\u{1b}'),
                    'f' => out.push('\u{c}'),
                    'v' => out.push('\u{b}'),
                    '\\' | '\'' | '"' | '?' => out.push(e),
                    'x' | 'u' | 'U' => {
                        let max = match e {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let start = i;
                        while i - start < max && chars.get(i).is_some_and(char::is_ascii_hexdigit) {
                            i += 1;
                        }
                        let hex: String = chars[start..i].iter().collect();
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(c) => out.push(c),
                            None => {
                                out.push('\\');
                                out.push(e);
                                out.push_str(&hex);
                            }
                        }
                    }
                    '0'..='7' => {
                        let start = i - 1;
                        while i - start < 3 && chars.get(i).is_some_and(|c| ('0'..='7').contains(c))
                        {
                            i += 1;
                        }
                        let oct: String = chars[start..i].iter().collect();
                        if let Some(c) = u32::from_str_radix(&oct, 8).ok().and_then(char::from_u32)
                        {
                            out.push(c);
                        }
                    }
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            Some(&c) => {
                out.push(c);
                i += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Options

/// Options that take an argument and only shape curl's own output or behaviour.
const IGNORED_WITH_ARG: &[&str] = &[
    "-o",
    "--output",
    "-w",
    "--write-out",
    "-D",
    "--dump-header",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--retry",
    "--retry-delay",
    "--retry-max-time",
    "--limit-rate",
    "-Y",
    "--speed-limit",
    "-y",
    "--speed-time",
    "-C",
    "--continue-at",
    "--output-dir",
    "-c",
    "--cookie-jar",
    "--parallel-max",
    "-z",
    "--time-cond",
];

const IGNORED: &[&str] = &[
    "-s",
    "--silent",
    "-S",
    "--show-error",
    "-v",
    "--verbose",
    "-i",
    "--include",
    "-f",
    "--fail",
    "--fail-with-body",
    "-O",
    "--remote-name",
    "-J",
    "--remote-header-name",
    "-#",
    "--progress-bar",
    "--no-progress-meter",
    "-N",
    "--no-buffer",
    "--compressed",
    "-g",
    "--globoff",
    "--create-dirs",
    "-n",
    "--netrc",
    "-q",
    "--disable",
    "-Z",
    "--parallel",
    "--retry-all-errors",
    "--retry-connrefused",
    "--fail-early",
    "-R",
    "--remote-time",
    "--styled-output",
    "--no-keepalive",
    "--tr-encoding",
    "--raw",
    "--path-as-is",
    "--http0.9",
];

/// Options that change the request but have no counterpart; with an argument.
const SETTINGS_WITH_ARG: &[&str] = &[
    "-x",
    "--proxy",
    "-U",
    "--proxy-user",
    "--cacert",
    "--capath",
    "-E",
    "--cert",
    "--key",
    "--cert-type",
    "--key-type",
    "--pass",
    "--resolve",
    "--connect-to",
    "--interface",
    "--connect-timeout",
    "--dns-servers",
    "--ciphers",
    "--tls-max",
    "--noproxy",
    "--unix-socket",
    "--abstract-unix-socket",
    "--local-port",
    "--proxy-header",
    "--preproxy",
    "--socks5",
    "--socks5-hostname",
    "--socks4",
    "--socks4a",
    "--max-filesize",
    "--keepalive-time",
    "--happy-eyeballs-timeout-ms",
    "--expect100-timeout",
    "--pinnedpubkey",
    "--crlfile",
];

const SETTINGS: &[&str] = &[
    "-k",
    "--insecure",
    "--http1.0",
    "--http1.1",
    "--http2",
    "--http2-prior-knowledge",
    "--http3",
    "--http3-only",
    "-4",
    "--ipv4",
    "-6",
    "--ipv6",
    "--tlsv1",
    "--tlsv1.0",
    "--tlsv1.1",
    "--tlsv1.2",
    "--tlsv1.3",
    "-1",
    "--ssl",
    "--ssl-reqd",
    "--proxy-insecure",
    "--location-trusted",
    "--post301",
    "--post302",
    "--post303",
    "-j",
    "--junk-session-cookies",
    "--negotiate",
    "--anyauth",
    "--basic",
    "--tcp-nodelay",
    "--tcp-fastopen",
    "--no-alpn",
    "--no-npn",
    "--no-sessionid",
];

/// Long options that take an argument (and which `--opt=value` may join).
const WITH_ARG: &[&str] = &[
    "-X",
    "--request",
    "-H",
    "--header",
    "-d",
    "--data",
    "--data-ascii",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "--json",
    "-F",
    "--form",
    "--form-string",
    "-u",
    "--user",
    "-A",
    "--user-agent",
    "-e",
    "--referer",
    "-b",
    "--cookie",
    "--url",
    "-m",
    "--max-time",
    "--max-redirs",
    "--oauth2-bearer",
    "--aws-sigv4",
    "--url-query",
    "-T",
    "--upload-file",
];

#[derive(Default)]
struct Parsed {
    urls: Vec<String>,
    method: Option<String>,
    headers: Vec<KeyValue>,
    /// `(text, url-encode)` pieces of `-d` and friends, in order.
    data: Vec<String>,
    data_files: Vec<String>,
    json: bool,
    form: Vec<MultipartPart>,
    user: Option<String>,
    auth_kind: Option<&'static str>,
    aws: Option<String>,
    bearer: Option<String>,
    get: bool,
    head: bool,
    location: bool,
    max_redirs: Option<u32>,
    max_time: Option<u64>,
    url_query: Vec<String>,
    upload: Option<String>,
}

fn header(line: &str) -> Option<KeyValue> {
    if let Some((n, v)) = line.split_once(':') {
        let n = n.trim();
        if n.is_empty() {
            return None;
        }
        return Some(KeyValue::new(n, v.trim_start()));
    }
    // `Name;` sends an empty header.
    line.strip_suffix(';')
        .filter(|n| !n.trim().is_empty())
        .map(|n| KeyValue::new(n.trim(), ""))
}

/// Percent-decodes `s` (and `+` as space), lossily as UTF-8.
pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit() =>
            {
                let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
                out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn command(args: &[String], folder: &[String], losses: &mut Losses) -> RequestOut {
    let mut p = Parsed::default();
    // Losses are collected first and attached once the request has a name.
    let mut pending: Vec<(LossKind, bool, String)> = Vec::new();
    let mut i = 0;
    let mut only_urls = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if only_urls || !arg.starts_with('-') || arg == "-" {
            p.urls.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_urls = true;
            continue;
        }
        // Split `--opt=value`, `-Xvalue` and clusters like `-sSL`.
        let (name, inline): (String, Option<String>) = if arg.starts_with("--") {
            match arg.split_once('=') {
                Some((n, v))
                    if WITH_ARG.contains(&n)
                        || SETTINGS_WITH_ARG.contains(&n)
                        || IGNORED_WITH_ARG.contains(&n) =>
                {
                    (n.to_owned(), Some(v.to_owned()))
                }
                _ => (arg.clone(), None),
            }
        } else {
            let mut cs = arg[1..].chars();
            let first = cs.next().map(|c| format!("-{c}")).unwrap_or_default();
            let rest: String = cs.collect();
            if rest.is_empty() {
                (first, None)
            } else if WITH_ARG.contains(&first.as_str())
                || SETTINGS_WITH_ARG.contains(&first.as_str())
                || IGNORED_WITH_ARG.contains(&first.as_str())
            {
                (first, Some(rest))
            } else {
                // A cluster of flags: handle each; a flag with an argument ends the cluster.
                let mut expanded: Vec<String> = Vec::new();
                let mut chars = arg[1..].chars().peekable();
                while let Some(c) = chars.next() {
                    let f = format!("-{c}");
                    let takes = WITH_ARG.contains(&f.as_str())
                        || SETTINGS_WITH_ARG.contains(&f.as_str())
                        || IGNORED_WITH_ARG.contains(&f.as_str());
                    if takes {
                        let rest: String = chars.collect();
                        expanded.push(format!("{f}{rest}"));
                        break;
                    }
                    expanded.push(f);
                }
                let tail: Vec<String> = args[i..].to_vec();
                let mut new_args: Vec<String> = args[..i - 1].to_vec();
                new_args.extend(expanded);
                new_args.extend(tail);
                return command(&new_args, folder, losses);
            }
        };
        let takes_arg = WITH_ARG.contains(&name.as_str())
            || SETTINGS_WITH_ARG.contains(&name.as_str())
            || IGNORED_WITH_ARG.contains(&name.as_str());
        let value = if takes_arg {
            match inline {
                Some(v) => Some(v),
                None if i < args.len() => {
                    i += 1;
                    Some(args[i - 1].clone())
                }
                None => {
                    pending.push((
                        LossKind::Other,
                        true,
                        format!("option `{name}` without a value ignored"),
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        let v = value.unwrap_or_default();
        match name.as_str() {
            "-X" | "--request" => p.method = Some(v.to_ascii_uppercase()),
            "-H" | "--header" => {
                if let Some(file) = v.strip_prefix('@') {
                    pending.push((
                        LossKind::Other,
                        true,
                        format!("headers from file `{file}` not imported"),
                    ));
                } else if let Some(h) = header(&v) {
                    p.headers.push(h);
                }
            }
            "-d" | "--data" | "--data-ascii" | "--data-binary" => {
                if let Some(file) = v.strip_prefix('@') {
                    p.data_files.push(file.to_owned());
                } else if name == "--data-binary" {
                    p.data.push(v);
                } else {
                    // curl strips line breaks from -d data.
                    p.data.push(v.replace(['\r', '\n'], ""));
                }
            }
            "--data-raw" => p.data.push(v),
            "--json" => {
                p.json = true;
                if let Some(file) = v.strip_prefix('@') {
                    p.data_files.push(file.to_owned());
                } else {
                    p.data.push(v);
                }
            }
            "--data-urlencode" => p.data.push(urlencode_arg(&v, &mut pending)),
            "-F" | "--form" | "--form-string" => {
                if let Some(part) = form_part(&v, name == "--form-string") {
                    if part.kind == PartKind::File {
                        pending.push((
                            LossKind::Body,
                            false,
                            format!(
                                "file `{}` for `{}` refers to the original computer; copy it into \
                                 the collection folder and update the path",
                                part.values.join(", "),
                                part.name
                            ),
                        ));
                    }
                    p.form.push(part);
                }
            }
            "-u" | "--user" => p.user = Some(v),
            "--digest" => p.auth_kind = Some("digest"),
            "--ntlm" | "--ntlm-wb" => p.auth_kind = Some("ntlm"),
            "--aws-sigv4" => p.aws = Some(v),
            "--oauth2-bearer" => p.bearer = Some(v),
            "-A" | "--user-agent" => p.headers.push(KeyValue::new("User-Agent", v)),
            "-e" | "--referer" => p.headers.push(KeyValue::new(
                "Referer",
                v.trim_end_matches(";auto").to_owned(),
            )),
            "-b" | "--cookie" => {
                if v.contains('=') {
                    p.headers.push(KeyValue::new("Cookie", v));
                } else {
                    pending.push((
                        LossKind::Setting,
                        true,
                        format!("cookie file `{v}` not imported"),
                    ));
                }
            }
            "--url" => p.urls.push(v),
            "--url-query" => p.url_query.push(v),
            "-G" | "--get" => p.get = true,
            "-I" | "--head" => p.head = true,
            "-L" | "--location" => p.location = true,
            "--max-redirs" => p.max_redirs = v.trim().parse().ok(),
            "-m" | "--max-time" => {
                p.max_time = v
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .map(|s| (s * 1000.0) as u64)
            }
            "-T" | "--upload-file" => p.upload = Some(v),
            n if IGNORED.contains(&n) || IGNORED_WITH_ARG.contains(&n) => {}
            n if SETTINGS.contains(&n) || SETTINGS_WITH_ARG.contains(&n) => {
                pending.push((
                    LossKind::Setting,
                    true,
                    format!("option `{n}` not imported"),
                ));
            }
            n => pending.push((
                LossKind::Other,
                true,
                format!("unknown option `{n}` ignored"),
            )),
        }
    }
    build(p, pending, folder, losses)
}

/// `--data-urlencode` argument → the encoded `name=value` piece.
fn urlencode_arg(v: &str, pending: &mut Vec<(LossKind, bool, String)>) -> String {
    // Forms: `content`, `=content`, `name=content`, `@file`, `name@file`.
    if let Some((name, content)) = v.split_once('=') {
        let enc = encode(content);
        return if name.is_empty() {
            enc
        } else {
            format!("{name}={enc}")
        };
    }
    if v.contains('@') {
        pending.push((
            LossKind::Body,
            true,
            format!("data from file in `--data-urlencode {v}` not imported"),
        ));
        return String::new();
    }
    encode(v)
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `-F name=value`, `name=@file;type=x`, `name=<file`.
fn form_part(v: &str, literal: bool) -> Option<MultipartPart> {
    let (name, value) = v.split_once('=')?;
    let mut part = MultipartPart {
        name: name.to_owned(),
        kind: PartKind::Text,
        values: vec![value.to_owned()],
        content_type: None,
        enabled: true,
    };
    if literal {
        return Some(part);
    }
    let file = value.strip_prefix('@').or_else(|| value.strip_prefix('<'));
    if let Some(file) = file {
        let mut pieces = file.split(';');
        let path = pieces.next().unwrap_or("").trim_matches('"').to_owned();
        for attr in pieces {
            if let Some(t) = attr.trim().strip_prefix("type=") {
                part.content_type = Some(t.to_owned());
            }
        }
        part.kind = PartKind::File;
        part.values = vec![path];
    } else if let Some((text, attrs)) = value.split_once(";type=") {
        part.values = vec![text.to_owned()];
        part.content_type = Some(attrs.to_owned());
    }
    Some(part)
}

fn is_form_pairs(data: &str) -> bool {
    !data.is_empty()
        && data.split('&').all(|pair| {
            let name = pair.split('=').next().unwrap_or("");
            pair.contains('=') && !name.is_empty() && !name.contains([' ', '{', '"', '<', '\n'])
        })
}

fn build(
    p: Parsed,
    pending: Vec<(LossKind, bool, String)>,
    folder: &[String],
    losses: &mut Losses,
) -> RequestOut {
    let mut url = p.urls.first().cloned().unwrap_or_default();
    if !url.is_empty() && !url.contains("://") && !url.starts_with("{{") {
        url = format!("http://{url}");
    }
    let mut headers = p.headers;
    let mut data = p.data.join("&");
    if p.get && !data.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&data);
        data.clear();
    }
    for q in &p.url_query {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(q);
    }
    let content_type = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.clone());
    let mut extra: Vec<(LossKind, bool, String)> = Vec::new();
    let mut body = Body::None;
    if !p.form.is_empty() {
        body = Body::Multipart { parts: p.form };
    } else if let Some(file) = p.data_files.first().or(p.upload.as_ref()) {
        extra.push((
            LossKind::Body,
            false,
            format!(
                "body file `{file}` refers to the original computer; copy it into the collection \
                 folder and update the path"
            ),
        ));
        body = Body::File {
            path: file.clone(),
            content_type: content_type.clone(),
        };
    } else if !data.is_empty() {
        if p.json {
            if content_type.is_none() {
                headers.push(KeyValue::new("Content-Type", "application/json"));
            }
            if !headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("accept"))
            {
                headers.push(KeyValue::new("Accept", "application/json"));
            }
            body = Body::Raw {
                kind: RawKind::Json,
                data,
            };
        } else {
            match &content_type {
                Some(ct) if !ct.to_ascii_lowercase().contains("x-www-form-urlencoded") => {
                    body = Body::Raw {
                        kind: raw_kind(ct),
                        data,
                    };
                }
                _ if is_form_pairs(&data) => {
                    body = Body::FormUrlEncoded {
                        fields: data
                            .split('&')
                            .map(|pair| {
                                let (n, v) = pair.split_once('=').unwrap_or((pair, ""));
                                KeyValue::new(percent_decode(n), percent_decode(v))
                            })
                            .collect(),
                    };
                }
                _ => {
                    // curl sends it as a form, whatever it looks like.
                    if content_type.is_none() {
                        headers.push(KeyValue::new(
                            "Content-Type",
                            "application/x-www-form-urlencoded",
                        ));
                    }
                    body = Body::Raw {
                        kind: RawKind::Text,
                        data,
                    };
                }
            }
        }
    }
    if p.data_files.len() > 1 {
        extra.push((
            LossKind::Body,
            true,
            "only the first `@file` body is imported".into(),
        ));
    }
    let has_body = body != Body::None;
    let method = p.method.clone().unwrap_or_else(|| {
        if p.head {
            "HEAD".into()
        } else if p.upload.is_some() {
            "PUT".into()
        } else if has_body {
            "POST".into()
        } else {
            "GET".into()
        }
    });

    let mut auth_out = AuthOut::Model(Auth::None);
    if let Some(token) = &p.bearer {
        auth_out = AuthOut::Model(Auth::Bearer {
            token: token.clone(),
        });
    }
    if let Some(user) = &p.user {
        let (u, pw) = user.split_once(':').unwrap_or((user, ""));
        auth_out = match (p.auth_kind, &p.aws) {
            (_, Some(aws)) => {
                let mut parts = aws.split(':');
                let _provider = parts.next();
                let _provider2 = parts.next();
                auth::awsv4(&auth::AwsV4 {
                    access_key_id: u.to_owned(),
                    secret_access_key: pw.to_owned(),
                    region: parts.next().unwrap_or("").to_owned(),
                    service: parts.next().unwrap_or("").to_owned(),
                    ..auth::AwsV4::default()
                })
            }
            (Some("digest"), None) => auth::user_password("digest", u, pw),
            (Some("ntlm"), None) => {
                let (domain, user) = u.split_once('\\').unwrap_or(("", u));
                auth::ntlm(user, pw, domain)
            }
            _ => AuthOut::Model(Auth::Basic {
                username: u.to_owned(),
                password: pw.to_owned(),
            }),
        };
    }

    let name = request_name(&method, &url);
    let here = location(folder, &name);
    for (kind, dropped, detail) in pending.into_iter().chain(extra) {
        if dropped {
            losses.dropped(&here, kind, detail);
        } else {
            losses.approximated(&here, kind, detail);
        }
    }
    if p.urls.len() > 1 {
        losses.dropped(&here, LossKind::Other, {
            let n = p.urls.len() - 1;
            format!(
                "{n} more {} in the command not imported",
                plural(n, "URL", "URLs")
            )
        });
    }
    if p.auth_kind.is_some() && p.user.is_none() {
        losses.dropped(
            &here,
            LossKind::Auth,
            "auth option without `-u` not imported",
        );
    }
    let label = match &auth_out {
        AuthOut::Other { kind: "digest", .. } => "Digest auth",
        AuthOut::Other { kind: "ntlm", .. } => "NTLM auth",
        _ => "AWS Signature v4",
    };
    auth::report(losses, &here, &auth_out, label);
    let settings = Settings {
        follow_redirects: Some(p.location),
        max_redirects: p.max_redirs.filter(|_| p.location),
        timeout_ms: p.max_time,
        encode_url: None,
    };
    let spec = RequestSpec {
        actions: Vec::new(),
        assertions: Vec::new(),
        scripts: Vec::new(),
        name,
        seq: None,
        method,
        params: url_params(&url, &[]),
        url,
        headers,
        body,
        auth: Auth::None,
        vars: Vec::new(),
        settings,
    };
    let model_auth = match &auth_out {
        AuthOut::Model(a) => a.clone(),
        AuthOut::Other { .. } => Auth::None,
    };
    losses.literal_credentials(&here, &model_auth, &spec.headers);
    let mut r = RequestOut::new(spec);
    r.auth = auth_out;
    r
}

/// `METHOD host/path`, without the query string.
fn request_name(method: &str, url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        method.to_owned()
    } else {
        format!("{method} {}", rest.chars().take(120).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_words() {
        let c = split_commands("curl 'a b' \"c \\\"d\\\"\" e\\ f \\\n  $'x\\ny' $TOKEN \"${T}x\" # c\ncurl z; curl y && curl w").unwrap();
        assert_eq!(
            c,
            vec![
                vec![
                    "curl",
                    "a b",
                    "c \"d\"",
                    "e f",
                    "x\ny",
                    "{{TOKEN}}",
                    "{{T}}x"
                ],
                vec!["curl", "z"],
                vec!["curl", "y"],
                vec!["curl", "w"],
            ]
        );
    }

    #[test]
    fn cmd_quoting() {
        let c = split_commands("curl ^\"https://x/^\" ^\n  -H ^\"a: b^\"").unwrap();
        assert_eq!(c, vec![vec!["curl", "https://x/", "-H", "a: b"]]);
    }

    #[test]
    fn unterminated_quotes_are_errors() {
        assert!(split_commands("curl 'abc").is_err());
        assert!(split_commands("curl \"abc").is_err());
        assert!(split_commands("curl $'abc").is_err());
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b+c%zz%"), "a b c%zz%");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }
}
