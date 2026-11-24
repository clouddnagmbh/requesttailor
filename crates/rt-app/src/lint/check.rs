// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The four rules (wave 18, strand F). One walk over the model collects a check per item
//! (a request, a URL, a variable, a name); the settings then decide levels and what is
//! ignored, and the counts become the scores. Nothing is read from disk and nothing is sent;
//! messages name hosts and variable names, never values.

use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, Ipv6Addr};

use rt_core::model::{
    Auth, Collection, Folder, Item, OAuth2Flow, Protocol, RequestDefaults, RequestEntry,
    Script, ScriptPhase,
};

use super::score::{self, Counts};
use super::usage::{self, Usage};
use super::{FolderScore, LintFinding, LintLevel, LintReport, LintRule, LintSettings};

/// One thing a rule looked at.
struct Check {
    rule: LintRule,
    /// The folder it belongs to for the folder scores (`""`: the collection).
    folder: String,
    /// What a finding names: a request (`users/get.yml`), a folder (`users/`), `""` for the
    /// collection, `environments/<name>.yml` for an environment.
    path: String,
    /// Counts as satisfied in the score.
    ok: bool,
    findings: Vec<Found>,
}

struct Found {
    /// `None`: the rule's level; loopback hosts are `Info` at most.
    cap: Option<LintLevel>,
    message: String,
    location: Option<String>,
}

pub(super) fn run(collection: &Collection, extra: &[&str], settings: &LintSettings) -> LintReport {
    let mut checks: Vec<Check> = Vec::new();
    let level = |rule| settings.level(rule);

    if level(LintRule::NoTests) != LintLevel::Off {
        no_tests(collection, &mut checks);
    }
    if level(LintRule::HardCodedHost) != LintLevel::Off {
        hard_coded_hosts(collection, &mut checks);
    }
    if level(LintRule::OrphanVariable) != LintLevel::Off {
        orphan_variables(collection, extra, &mut checks);
    }
    if level(LintRule::DuplicateName) != LintLevel::Off {
        duplicate_names(&collection.items, "", &mut checks);
    }

    let patterns: Vec<Pattern> = settings.ignore.iter().map(|p| Pattern::parse(p)).collect();
    let mut kept: Vec<Check> = Vec::new();
    let mut ignored = 0u32;
    for check in checks {
        if patterns.iter().any(|p| p.matches(check.rule, &check.path)) {
            ignored += check.findings.len() as u32;
            continue;
        }
        kept.push(check);
    }

    let mut findings: Vec<LintFinding> = Vec::new();
    for check in &kept {
        let base = settings.level(check.rule);
        for f in &check.findings {
            findings.push(LintFinding {
                rule: check.rule,
                level: f.cap.map_or(base, |cap| base.min(cap)),
                path: check.path.clone(),
                message: f.message.clone(),
                location: f.location.clone(),
            });
        }
    }
    let rule_order = |r: LintRule| LintRule::ALL.iter().position(|x| *x == r).unwrap_or(0);
    findings.sort_by(|a, b| {
        (&a.path, rule_order(a.rule), &a.location, &a.message).cmp(&(
            &b.path,
            rule_order(b.rule),
            &b.location,
            &b.message,
        ))
    });

    let mut all: Counts = Default::default();
    let mut per_folder: BTreeMap<String, Counts> = BTreeMap::new();
    let mut folder_list = vec![String::new()];
    list_folders(&collection.items, &mut folder_list);
    for f in &folder_list {
        per_folder.insert(f.clone(), Default::default());
    }
    for check in &kept {
        score::add(&mut all, check.rule, check.ok);
        let mut folder = check.folder.as_str();
        loop {
            if let Some(c) = per_folder.get_mut(folder) {
                score::add(c, check.rule, check.ok);
            }
            if folder.is_empty() {
                break;
            }
            folder = folder.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    let folders = folder_list
        .into_iter()
        .map(|folder| {
            let counts = per_folder.get(&folder).copied().unwrap_or_default();
            FolderScore {
                folder,
                score: score::health(&counts),
            }
        })
        .collect();
    LintReport {
        collection: collection.name.clone(),
        score: score::health(&all),
        folders,
        findings,
        ignored,
    }
}

fn list_folders(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        if let Item::Folder(f) = item {
            out.push(f.path.clone());
            list_folders(&f.items, out);
        }
    }
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

// ---- ignoring ----

struct Pattern {
    rule: Option<LintRule>,
    glob: String,
}

impl Pattern {
    fn parse(text: &str) -> Pattern {
        if text.starts_with("lint/")
            && let Some((id, glob)) = text.split_once(':')
            && let Some(rule) = LintRule::from_id(id)
        {
            return Pattern {
                rule: Some(rule),
                glob: glob.to_owned(),
            };
        }
        Pattern {
            rule: None,
            glob: text.to_owned(),
        }
    }

    fn matches(&self, rule: LintRule, path: &str) -> bool {
        if self.rule.is_some_and(|r| r != rule) {
            return false;
        }
        if !self.glob.contains('*') && self.glob.ends_with('/') {
            return path.starts_with(&self.glob);
        }
        glob(&self.glob, path)
    }
}

/// `*` matches any run of characters (also `/`); everything else is literal.
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

// ---- no-tests ----

fn has_test_call(code: &str) -> bool {
    let chars: Vec<char> = code.chars().collect();
    let needle: Vec<char> = "test(".chars().collect();
    (0..chars.len().saturating_sub(needle.len() - 1)).any(|i| {
        chars[i..i + needle.len()] == needle[..]
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '$' | '.')))
    })
}

fn tests_in(scripts: &[Script]) -> bool {
    scripts.iter().any(|s| {
        !s.code.trim().is_empty() && (s.phase == ScriptPhase::Tests || has_test_call(&s.code))
    })
}

fn no_tests(collection: &Collection, out: &mut Vec<Check>) {
    fn walk(items: &[Item], covered: bool, out: &mut Vec<Check>) {
        for item in items {
            match item {
                Item::Folder(f) => walk(&f.items, covered || tests_in(&f.defaults.scripts), out),
                Item::Request(r) => {
                    if !matches!(r.spec.protocol, Protocol::Http) {
                        continue;
                    }
                    let tested = covered
                        || tests_in(&r.spec.scripts)
                        || r.spec.assertions.iter().any(|a| a.enabled);
                    out.push(Check {
                        rule: LintRule::NoTests,
                        folder: parent_of(&r.path).to_owned(),
                        path: r.path.clone(),
                        ok: tested,
                        findings: if tested {
                            Vec::new()
                        } else {
                            vec![Found {
                                cap: None,
                                message: "The request has no test, assertion or snapshot.".into(),
                                location: None,
                            }]
                        },
                    });
                }
            }
        }
    }
    walk(
        &collection.items,
        tests_in(&collection.defaults.scripts),
        out,
    );
}

// ---- hard-coded-host ----

enum Host {
    Fixed(String),
    Loopback(String),
}

/// The host of a URL with a literal scheme and a literal host; `None` for anything that
/// involves a variable or has no scheme.
fn literal_host(url: &str) -> Option<Host> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.is_empty() || host.contains("{{") || host.contains('{') {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    let loopback = lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
        || lower.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    Some(if loopback {
        Host::Loopback(host.to_owned())
    } else {
        Host::Fixed(host.to_owned())
    })
}

fn url_check(rule_folder: &str, path: &str, location: &str, url: &str) -> Option<Check> {
    if url.trim().is_empty() {
        return None;
    }
    let Some(host) = literal_host(url) else {
        // A variable, a relative URL or no scheme: fine, and one more URL checked.
        return Some(Check {
            rule: LintRule::HardCodedHost,
            folder: rule_folder.to_owned(),
            path: path.to_owned(),
            ok: true,
            findings: Vec::new(),
        });
    };
    let (ok, found) = match host {
        Host::Fixed(host) => (
            false,
            Found {
                cap: None,
                message: format!(
                    "The {} has a fixed host ({host}); use a variable such as {{{{baseUrl}}}}.",
                    describe(location)
                ),
                location: Some(location.to_owned()),
            },
        ),
        Host::Loopback(host) => (
            true,
            Found {
                cap: Some(LintLevel::Info),
                message: format!(
                    "The {} points at {host}; a variable keeps it out of the file.",
                    describe(location)
                ),
                location: Some(location.to_owned()),
            },
        ),
    };
    Some(Check {
        rule: LintRule::HardCodedHost,
        folder: rule_folder.to_owned(),
        path: path.to_owned(),
        ok,
        findings: vec![found],
    })
}

fn describe(location: &str) -> &'static str {
    match location {
        "url" => "URL",
        "auth.access_token_url" => "token URL",
        "auth.refresh_token_url" => "refresh URL",
        "auth.authorization_url" => "authorization URL",
        "auth.device_authorization_url" => "device authorization URL",
        _ => "URL",
    }
}

/// The endpoints OAuth 2.0 auth names.
fn auth_urls(auth: &Auth) -> Vec<(&'static str, &str)> {
    let Auth::OAuth2(o) = auth else {
        return Vec::new();
    };
    let mut urls = vec![
        ("auth.access_token_url", o.access_token_url.as_str()),
        ("auth.refresh_token_url", o.refresh_token_url.as_str()),
    ];
    match &o.flow {
        OAuth2Flow::AuthorizationCode {
            authorization_url, ..
        }
        | OAuth2Flow::Implicit {
            authorization_url, ..
        } => urls.push(("auth.authorization_url", authorization_url.as_str())),
        OAuth2Flow::DeviceCode {
            device_authorization_url,
        } => urls.push((
            "auth.device_authorization_url",
            device_authorization_url.as_str(),
        )),
        _ => {}
    }
    urls
}

fn defaults_hosts(
    defaults: &RequestDefaults,
    folder: &str,
    path: &str,
    out: &mut Vec<Check>,
) {
    for (location, url) in auth_urls(&defaults.auth) {
        out.extend(url_check(folder, path, location, url));
    }
}

fn hard_coded_hosts(collection: &Collection, out: &mut Vec<Check>) {
    defaults_hosts(&collection.defaults, "", "", out);
    fn walk(items: &[Item], out: &mut Vec<Check>) {
        for item in items {
            match item {
                Item::Folder(f) => {
                    defaults_hosts(&f.defaults, &f.path, &format!("{}/", f.path), out);
                    walk(&f.items, out);
                }
                Item::Request(r) => request_hosts(r, out),
            }
        }
    }
    walk(&collection.items, out);
}

fn request_hosts(r: &RequestEntry, out: &mut Vec<Check>) {
    let folder = parent_of(&r.path);
    out.extend(url_check(folder, &r.path, "url", &r.spec.url));
    for (location, url) in auth_urls(&r.spec.auth) {
        out.extend(url_check(folder, &r.path, location, url));
    }
}

// ---- orphan-variable ----

struct Defined {
    name: String,
    secret: bool,
    folder: String,
    path: String,
    place: String,
}

fn orphan_variables(collection: &Collection, extra: &[&str], out: &mut Vec<Check>) {
    let usage: Usage = usage::of(collection, extra);
    let mut defined: Vec<Defined> = Vec::new();
    for v in collection.defaults.vars.iter().filter(|v| v.enabled) {
        defined.push(Defined {
            name: v.name.clone(),
            secret: v.secret,
            folder: String::new(),
            path: String::new(),
            place: "the collection".into(),
        });
    }
    fn folders(items: &[Item], defined: &mut Vec<Defined>) {
        for item in items {
            if let Item::Folder(f) = item {
                folder_vars(f, defined);
                folders(&f.items, defined);
            }
        }
    }
    fn folder_vars(f: &Folder, defined: &mut Vec<Defined>) {
        for v in f.defaults.vars.iter().filter(|v| v.enabled) {
            defined.push(Defined {
                name: v.name.clone(),
                secret: v.secret,
                folder: f.path.clone(),
                path: format!("{}/", f.path),
                place: format!("the folder {}", f.name),
            });
        }
    }
    folders(&collection.items, &mut defined);
    for env in &collection.environments {
        let path = format!("environments/{}.yml", env.name);
        let place = format!("the environment {}", env.name);
        for v in env.variables.iter().filter(|v| v.enabled) {
            defined.push(Defined {
                name: v.name.clone(),
                secret: v.secret,
                folder: String::new(),
                path: path.clone(),
                place: place.clone(),
            });
        }
        if let Some(ext) = &env.external_secrets {
            for s in ext.variables.iter().filter(|s| s.enabled) {
                defined.push(Defined {
                    name: s.name.clone(),
                    secret: true,
                    folder: String::new(),
                    path: path.clone(),
                    place: place.clone(),
                });
            }
        }
    }
    // One item per definition; a script that reads variables by a computed name makes every
    // definition "maybe used", so nothing is reported (but the items still count as fine).
    let used: HashMap<&str, ()> = usage.names.iter().map(|n| (n.as_str(), ())).collect();
    for d in defined {
        let unused = !usage.dynamic && !used.contains_key(d.name.as_str());
        let kind = if d.secret { "Secret" } else { "Variable" };
        out.push(Check {
            rule: LintRule::OrphanVariable,
            folder: d.folder,
            path: d.path,
            ok: !unused,
            findings: if unused {
                vec![Found {
                    cap: None,
                    message: format!(
                        "{kind} `{}` of {} is never used.",
                        d.name, d.place
                    ),
                    location: Some(format!("variable {}", d.name)),
                }]
            } else {
                Vec::new()
            },
        });
    }
}

// ---- duplicate-name ----

fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn duplicate_names(items: &[Item], folder: &str, out: &mut Vec<Check>) {
    // Requests by name and by file name, folders by name and by directory name.
    let mut request_names: HashMap<String, Vec<usize>> = HashMap::new();
    let mut request_files: HashMap<String, Vec<usize>> = HashMap::new();
    let mut folder_names: HashMap<String, Vec<usize>> = HashMap::new();
    let mut folder_dirs: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Request(r) => {
                request_names.entry(r.spec.name.trim().to_owned()).or_default().push(i);
                request_files
                    .entry(last_segment(&r.path).to_lowercase())
                    .or_default()
                    .push(i);
            }
            Item::Folder(f) => {
                folder_names.entry(f.name.trim().to_owned()).or_default().push(i);
                folder_dirs
                    .entry(last_segment(&f.path).to_lowercase())
                    .or_default()
                    .push(i);
            }
        }
    }
    let mut messages: HashMap<usize, Vec<Found>> = HashMap::new();
    let mut flag = |groups: &HashMap<String, Vec<usize>>, what: &str, location: &str| {
        for (key, members) in groups {
            if members.len() < 2 || key.is_empty() {
                continue;
            }
            for &i in members {
                messages.entry(i).or_default().push(Found {
                    cap: None,
                    message: format!(
                        "{} others in this {} have the same {what} “{key}”.",
                        members.len() - 1,
                        if folder.is_empty() { "collection" } else { "folder" }
                    ),
                    location: Some(location.to_owned()),
                });
            }
        }
    };
    flag(&request_names, "name", "name");
    flag(&request_files, "file name", "file name");
    flag(&folder_names, "name", "name");
    flag(&folder_dirs, "folder name", "file name");
    for (i, item) in items.iter().enumerate() {
        let (path, child) = match item {
            Item::Request(r) => (r.path.clone(), None),
            Item::Folder(f) => (format!("{}/", f.path), Some(f)),
        };
        let mut findings = messages.remove(&i).unwrap_or_default();
        findings.sort_by(|a, b| (&a.location, &a.message).cmp(&(&b.location, &b.message)));
        out.push(Check {
            rule: LintRule::DuplicateName,
            folder: folder.to_owned(),
            path,
            ok: findings.is_empty(),
            findings,
        });
        if let Some(f) = child {
            duplicate_names(&f.items, &f.path, out);
        }
    }
}
