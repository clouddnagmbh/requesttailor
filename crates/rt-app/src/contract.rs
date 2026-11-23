// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The OpenAPI contract for the app (wave 12): finding the spec, and the shared steps of the
//! contract check, breaking changes and `rt changelog` that desktop, web and CLI run the same
//! way on top of [`rt_format::contract`].
//!
//! The shell tells this module how to read a spec file (`read`: the work tree, or
//! `GitApi.show` at a revision); everything else is pure. The web shell does the same in
//! TypeScript over the functions of `rt-wasm` and uses [`changelog_title`] from here.
//!
//! Owner: wave 12, strand F.

use rt_core::model::{Auth, KeyValue, OAuth2Flow, RequestContext, RequestSpec};
use rt_format::FileSet;
use rt_format::contract::MAX_CHECKED_BODY;
pub use rt_format::contract::{
    ContractError, ContractFinding, ContractReport, ContractSpec, FindingKind, SPEC_CANDIDATES,
    SpecChange, SpecChangeKind, SpecChanges,
};

use crate::AppError;

impl From<ContractError> for AppError {
    fn from(e: ContractError) -> Self {
        match e {
            ContractError::Spec(m) => AppError::Format(m),
            ContractError::Unsupported(m) => AppError::Unsupported(m),
        }
    }
}

type Result<T> = std::result::Result<T, AppError>;

/// Reads one spec path (collection-relative, or repository-relative with a leading `/`);
/// `Ok(None)` if the file does not exist.
pub type ReadFile<'a> = &'a dyn Fn(&str) -> Result<Option<Vec<u8>>>;

/// The spec path of the collection in `files`: configured in `opencollection.yml`, else the
/// first of [`SPEC_CANDIDATES`] for which `exists` says yes (the shell asks its work tree or
/// `GitApi.show`).
pub fn locate(files: &rt_format::FileSet, exists: impl Fn(&str) -> bool) -> Option<String> {
    rt_format::contract::configured_spec(files).or_else(|| {
        SPEC_CANDIDATES
            .iter()
            .find(|p| exists(p))
            .map(|p| (*p).to_owned())
    })
}

/// A spec path must stay inside the collection or the repository.
pub fn safe_path(path: &str) -> Result<()> {
    let bad = path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|s| s == "..")
        || path.is_empty();
    if bad {
        return Err(AppError::Format(format!(
            "the spec path {path:?} must stay inside the collection or the repository"
        )));
    }
    Ok(())
}

/// The spec of one side: its description and bytes.
pub struct Loaded {
    pub spec: ContractSpec,
    pub bytes: Vec<u8>,
}

/// Finds and reads the spec of `files`. `None` if there is none. A configured spec that does
/// not exist is an error (the configuration is wrong), a found one that does not parse too.
pub fn load(files: &FileSet, read: ReadFile<'_>) -> Result<Option<Loaded>> {
    let configured = rt_format::contract::configured_spec(files);
    let (path, bytes) = match &configured {
        Some(path) => {
            safe_path(path)?;
            let Some(bytes) = read(path)? else {
                return Err(AppError::Format(format!(
                    "the spec {path} named in opencollection.yml does not exist"
                )));
            };
            (path.clone(), bytes)
        }
        None => {
            let mut found = None;
            for candidate in SPEC_CANDIDATES {
                if let Some(bytes) = read(candidate)? {
                    found = Some(((*candidate).to_owned(), bytes));
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => return Ok(None),
            }
        }
    };
    let (title, version) = rt_format::contract::spec_info(&bytes)?;
    Ok(Some(Loaded {
        spec: ContractSpec {
            path,
            title,
            version,
            configured: configured.is_some(),
        },
        bytes,
    }))
}

/// The collection checked against its spec.
pub fn check(files: &FileSet, read: ReadFile<'_>) -> Result<ContractReport> {
    let Some(loaded) = load(files, read)? else {
        return Err(AppError::Format(no_spec()));
    };
    Ok(rt_format::contract::check(files, &loaded.bytes)?)
}

fn no_spec() -> String {
    "no OpenAPI spec found: add openapi.yaml next to the collection or at the repository root, or name it in opencollection.yml under extensions.requesttailor.contract.spec".into()
}

/// One side of a range: the collection's files and how to read its spec.
pub struct Side<'a> {
    pub files: &'a FileSet,
    pub read: ReadFile<'a>,
}

/// Changes of the spec between two sides. A side without a spec counts as "did not exist".
pub fn changes(old: &Side<'_>, new: &Side<'_>) -> Result<SpecChanges> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    Ok(rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?)
}

/// The heading of a changelog: `Orders API 1.1.0 → 2.0.0` when the spec's version is known,
/// else `Changes <from> → <to>`. `name`: the spec's title, if known.
pub fn changelog_title(name: Option<&str>, spec: &SpecChanges, from: &str, to: &str) -> String {
    match (name, &spec.from_version, &spec.to_version) {
        (Some(n), Some(a), Some(b)) => format!("{n} {a} → {b}"),
        (Some(n), None, Some(b)) => format!("{n} {b}"),
        (Some(n), _, _) => format!("{n}: {from} → {to}"),
        (None, _, _) => format!("Changes {from} → {to}"),
    }
}

/// A section a distribution adds to the changelog (since wave 13), e.g. the changes of
/// another description file next to the collection. `section` returns Markdown starting with
/// a `## ` heading, or `None` when it has nothing to say for this range. `Side::files` are the
/// collection files (`*.yml`); any other file is read with `Side::read` (collection-relative,
/// or repository-relative with a leading `/`), as the spec is.
pub trait ChangelogSection {
    fn section(
        &self,
        old: &Side<'_>,
        new: &Side<'_>,
        from: &str,
        to: &str,
    ) -> Result<Option<String>>;
}

/// [`changelog`] followed by the `sections` that have something to say, in the order given,
/// each separated by a blank line.
pub fn changelog_with(
    old: &Side<'_>,
    new: &Side<'_>,
    from: &str,
    to: &str,
    sections: &[&dyn ChangelogSection],
) -> Result<String> {
    let mut text = changelog(old, new, from, to)?;
    for section in sections {
        if let Some(more) = section.section(old, new, from, to)? {
            let more = more.trim();
            if more.is_empty() {
                continue;
            }
            while !text.ends_with("\n\n") {
                text.push('\n');
            }
            text.push_str(more);
            text.push('\n');
        }
    }
    Ok(text)
}

/// The API changelog (Markdown) of a range, as `rt changelog` writes it. `from` and `to` are
/// the labels of the sides (revisions).
pub fn changelog(old: &Side<'_>, new: &Side<'_>, from: &str, to: &str) -> Result<String> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    let spec = rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?;
    let collection = rt_format::diff::diff(old.files, new.files)?;
    let name = new_spec
        .as_ref()
        .or(old_spec.as_ref())
        .map(|l| l.spec.title.as_str());
    let title = changelog_title(name, &spec, from, to);
    Ok(rt_format::contract::changelog(&title, &spec, &collection))
}

// ---- the run wizard and the response check (interface) ----

pub use rt_format::contract::{
    DifferencePlace, InputField, InputPlace, InputProblem, ParsedSpec, ProblemKind, ResponseCheck,
    ResponseDifference, SendRequirements, ServerOption, VarFact, VarState,
};

/// The parsed spec of one collection, kept between sends: parsed again only when the path or
/// the bytes change (compared with the bytes parsed last; a memory compare is far cheaper than
/// any hash over a large spec).
#[derive(Debug, Default)]
pub struct SpecCache {
    entry: Option<CachedSpec>,
}

#[derive(Debug)]
struct CachedSpec {
    path: String,
    bytes: Vec<u8>,
    parsed: std::sync::Arc<ParsedSpec>,
}

impl SpecCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The spec of `files` (as [`load`] finds it), parsed or from the cache; `None` if the
    /// collection has none.
    pub fn get(
        &mut self,
        files: &FileSet,
        read: ReadFile<'_>,
    ) -> Result<Option<(String, std::sync::Arc<ParsedSpec>)>> {
        let Some(loaded) = load(files, read)? else {
            self.entry = None;
            return Ok(None);
        };
        if let Some(c) = &self.entry
            && c.path == loaded.spec.path
            && c.bytes == loaded.bytes
        {
            return Ok(Some((c.path.clone(), std::sync::Arc::clone(&c.parsed))));
        }
        let parsed = std::sync::Arc::new(ParsedSpec::parse(&loaded.bytes)?);
        self.entry = Some(CachedSpec {
            path: loaded.spec.path.clone(),
            bytes: loaded.bytes,
            parsed: std::sync::Arc::clone(&parsed),
        });
        Ok(Some((loaded.spec.path, parsed)))
    }
}

/// What the request at `path` still needs before sending, per its spec operation (`None`: the
/// collection has no spec, or no operation matches). `input` as for sending (the shell has
/// added the secrets it holds), so unresolved variables are those `send` would leave
/// unresolved.
///
/// The request is the one being edited (unsaved changes included), with the headers and the
/// auth it inherits from its folders and the collection. Variables: those the request uses
/// (as `Workspace::preview` reports them, plus the ones of the effective auth); a variable
/// declared secret counts as set only when `input.secrets` (or a runtime variable) gives it a
/// non-empty value. Secret values never leave this function.
pub fn requirements(
    workspace: &crate::Workspace,
    path: &str,
    input: &crate::SendInput,
    cache: &mut SpecCache,
    read: ReadFile<'_>,
) -> Result<Option<SendRequirements>> {
    let Some(context) = workspace.collection().request(path) else {
        return Err(AppError::RequestNotFound(path.to_owned()));
    };
    let Some((spec_path, spec)) = spec_of(workspace, cache, read)? else {
        return Ok(None);
    };
    let preview = workspace.preview(path, input.clone())?;
    let auth = effective_auth(&context);
    let request = effective_request(&context, &auth);
    let variables = var_facts(&context, input, &preview, &auth);
    let facts = rt_format::contract::RequestFacts {
        request: &request,
        variables: &variables,
    };
    Ok(
        rt_format::contract::requirements(&spec, &facts).map(|mut r| {
            r.spec = spec_path;
            r
        }),
    )
}

/// The stored response `response_id` of the request at `path` against its spec operation
/// (`None`: no spec, no matching operation, or no response).
pub fn validate_response(
    workspace: &crate::Workspace,
    path: &str,
    responses: &crate::ResponseStore,
    response_id: u64,
    cache: &mut SpecCache,
    read: ReadFile<'_>,
) -> Result<Option<ResponseCheck>> {
    let Some(context) = workspace.collection().request(path) else {
        return Err(AppError::RequestNotFound(path.to_owned()));
    };
    let Some((_, spec)) = spec_of(workspace, cache, read)? else {
        return Ok(None);
    };
    let report = responses.report(response_id)?;
    let Some(response) = &report.response else {
        return Ok(None);
    };
    let body = if response.truncated || response.size > MAX_CHECKED_BODY as u64 {
        None
    } else {
        let len = u32::try_from(response.size).unwrap_or(u32::MAX);
        Some(responses.bytes(response_id, 0, len)?)
    };
    let facts = rt_format::contract::ResponseFacts {
        status: response.status,
        headers: &response.headers,
        body: body.as_deref(),
    };
    let auth = effective_auth(&context);
    let request = effective_request(&context, &auth);
    let mut check = rt_format::contract::validate_response(&spec, &request, &facts);
    // Say why the body was not checked when it was too large rather than truncated.
    if let Some(c) = &mut check
        && body.is_none()
        && !response.truncated
        && c.body_skipped.as_deref() == Some("the body was not kept in full")
    {
        c.body_skipped = Some(format!(
            "the body is larger than {} MB",
            MAX_CHECKED_BODY / (1024 * 1024)
        ));
    }
    Ok(check)
}

/// The spec of the workspace's collection, parsed or from `cache`. The collection files are
/// the workspace's, with an unsaved `opencollection.yml` (where the spec may be named) and
/// staged companion files (an imported spec not saved yet) taking precedence.
/// The workspace's files with the unsaved `opencollection.yml`, as `save` would write it.
fn current_files(workspace: &crate::Workspace, dirty: &[String]) -> Result<FileSet> {
    let mut files = workspace.files().clone();
    if dirty.iter().any(|p| p == rt_format::COLLECTION_FILE) {
        let mut copy = workspace.clone();
        for change in copy.save()? {
            if change.deleted {
                files.files.remove(&change.path);
            } else {
                files.files.insert(change.path, change.text.into_bytes());
            }
        }
    }
    Ok(files)
}

/// The spec path the open collection names in `opencollection.yml`, unsaved edits included
/// (`None`: it names none, so [`load`] looks for `openapi.{yaml,yml,json}`). For shells that
/// read the spec's bytes before calling [`requirements`] (the web shell).
pub fn configured_spec(workspace: &crate::Workspace) -> Result<Option<String>> {
    let files = current_files(workspace, &workspace.dirty())?;
    Ok(rt_format::contract::configured_spec(&files))
}

fn spec_of(
    workspace: &crate::Workspace,
    cache: &mut SpecCache,
    read: ReadFile<'_>,
) -> Result<Option<(String, std::sync::Arc<ParsedSpec>)>> {
    let dirty = workspace.dirty();
    let files = current_files(workspace, &dirty)?;
    let staged = |p: &str| -> Result<Option<Vec<u8>>> {
        if !p.starts_with('/') && dirty.iter().any(|d| d == p) {
            return Ok(workspace.companion_file(p).map(<[u8]>::to_vec));
        }
        read(p)
    };
    cache.get(&files, &staged)
}

/// The auth that applies after inheritance (as `rt_core::prepare` resolves it), with
/// templates.
fn effective_auth(context: &RequestContext<'_>) -> Auth {
    std::iter::once(&context.request.spec.auth)
        .chain(context.folders.iter().rev().map(|f| &f.defaults.auth))
        .chain(std::iter::once(&context.collection.defaults.auth))
        .find(|a| **a != Auth::Inherit)
        .cloned()
        .unwrap_or(Auth::None)
}

/// The request as it is sent: its headers merged over the collection's and folders' (a later
/// layer replaces a header of the same name), and the effective auth.
fn effective_request(context: &RequestContext<'_>, auth: &Auth) -> RequestSpec {
    let mut request = context.request.spec.clone();
    let mut headers: Vec<KeyValue> = Vec::new();
    let layers = std::iter::once(&context.collection.defaults.headers)
        .chain(context.folders.iter().map(|f| &f.defaults.headers))
        .chain(std::iter::once(&request.headers));
    for h in layers.flatten().filter(|h| h.enabled && !h.name.is_empty()) {
        match headers
            .iter_mut()
            .find(|x| x.name.eq_ignore_ascii_case(&h.name))
        {
            Some(existing) => existing.value = h.value.clone(),
            None => headers.push(h.clone()),
        }
    }
    request.headers = headers;
    request.auth = auth.clone();
    request
}

/// The templates of the auth's fields: `(text, credential)`.
fn auth_fields(auth: &Auth) -> Vec<(&str, bool)> {
    match auth {
        Auth::Inherit | Auth::None | Auth::Unsupported { .. } => Vec::new(),
        Auth::Basic { username, password } => vec![(username, false), (password, true)],
        Auth::Bearer { token } => vec![(token, true)],
        Auth::ApiKey { key, value, .. } => vec![(key, false), (value, true)],
        Auth::OAuth2(o) => {
            let mut out = vec![
                (o.access_token_url.as_str(), false),
                (o.refresh_token_url.as_str(), false),
                (o.client_id.as_str(), false),
                (o.client_secret.as_str(), true),
                (o.scope.as_str(), false),
            ];
            match &o.flow {
                OAuth2Flow::ClientCredentials => {}
                OAuth2Flow::Password { username, password } => {
                    out.extend([(username.as_str(), false), (password.as_str(), true)]);
                }
                OAuth2Flow::AuthorizationCode {
                    authorization_url,
                    callback_url,
                    state,
                    ..
                }
                | OAuth2Flow::Implicit {
                    authorization_url,
                    callback_url,
                    state,
                } => out.extend([
                    (authorization_url.as_str(), false),
                    (callback_url.as_str(), false),
                    (state.as_str(), false),
                ]),
                OAuth2Flow::DeviceCode {
                    device_authorization_url,
                } => out.push((device_authorization_url.as_str(), false)),
                OAuth2Flow::Saml2Bearer { assertion } => out.push((assertion.as_str(), true)),
            }
            for p in &o.parameters {
                out.extend([(p.name.as_str(), false), (p.value.as_str(), false)]);
            }
            out
        }
        Auth::AwsSigV4(a) => vec![
            (a.access_key_id.as_str(), false),
            (a.secret_access_key.as_str(), true),
            (a.session_token.as_str(), true),
            (a.service.as_str(), false),
            (a.region.as_str(), false),
            (a.profile_name.as_str(), false),
        ],
    }
}

/// Names of the `{{placeholders}}` in `text`, in order.
fn placeholders(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else { break };
        let name = after[..j].trim();
        if !name.is_empty() && !name.contains('{') {
            out.push(name);
        }
        rest = &after[j + 2..];
    }
    out
}

fn var_facts(
    context: &RequestContext<'_>,
    input: &crate::SendInput,
    preview: &crate::Preview,
    auth: &Auth,
) -> Vec<VarFact> {
    // Names in use: what the preparation substituted or left unresolved, then the auth's
    // (OAuth 2.0 and SigV4 are applied after the preparation, so the preview misses them).
    let mut names: Vec<String> = Vec::new();
    let mut add = |n: &str| {
        if !names.iter().any(|x| x == n) {
            names.push(n.to_owned());
        }
    };
    for v in &preview.variables {
        add(&v.name);
    }
    for n in &preview.unresolved {
        add(n);
    }
    let fields = auth_fields(auth);
    let mut auth_vars: Vec<(&str, bool)> = Vec::new();
    for (text, credential) in &fields {
        for name in placeholders(text) {
            add(name);
            match auth_vars.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 |= credential,
                None => auth_vars.push((name, *credential)),
            }
        }
    }

    let environment = input
        .environment
        .as_deref()
        .and_then(|name| context.collection.environment(name));
    let declared_secret = |name: &str| {
        context
            .collection
            .defaults
            .vars
            .iter()
            .chain(environment.into_iter().flat_map(|e| &e.variables))
            .chain(context.folders.iter().flat_map(|f| &f.defaults.vars))
            .chain(&context.request.spec.vars)
            .any(|v| v.secret && v.name == name)
    };
    let prepare_input = rt_core::prepare::PrepareInput {
        environment: environment.map(|e| e.name.as_str()),
        globals: Vec::new(),
        runtime: input.vars.clone(),
    };
    let scope = rt_core::prepare::scope(context, &prepare_input).unwrap_or_default();
    let given = |name: &str| {
        input
            .secrets
            .iter()
            .chain(&input.vars)
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };

    // Those of the URL first (`baseUrl`), the auth's last.
    let url = &context.request.spec.url;
    let rank = |name: &str| {
        if placeholders(url).contains(&name) {
            0
        } else if auth_vars.iter().any(|(n, _)| *n == name) {
            2
        } else {
            1
        }
    };
    names.sort_by_key(|n| rank(n));

    names
        .into_iter()
        .map(|name| {
            let auth_var = auth_vars.iter().find(|(n, _)| *n == name);
            let previewed_secret = preview.variables.iter().any(|v| v.name == name && v.secret);
            let declared = declared_secret(&name) || previewed_secret;
            let secret = declared || auth_var.is_some_and(|(_, credential)| *credential);
            let state = if declared {
                match given(&name) {
                    Some(v) if !v.is_empty() => VarState::Set,
                    Some(_) => VarState::Empty,
                    None => VarState::Unresolved,
                }
            } else if preview.unresolved.contains(&name) {
                VarState::Unresolved
            } else {
                match scope.get(&name) {
                    Some((v, _, _)) if !v.is_empty() => VarState::Set,
                    Some(_) => VarState::Empty,
                    None => VarState::Unresolved,
                }
            };
            let value = if secret || state == VarState::Unresolved {
                None
            } else {
                preview
                    .variables
                    .iter()
                    .find(|v| v.name == name)
                    .and_then(|v| v.value.clone())
                    .or_else(|| scope.get(&name).map(|(v, _, _)| v.to_owned()))
            };
            VarFact {
                auth: auth_var.is_some(),
                name,
                state,
                secret,
                value,
            }
        })
        .collect()
}

// ---- Wave 16 interface: changelog writers (e.g. release notes written by a model) ----

/// A commit of a changelog's range (first-parent chain from `from` to `to`), for writers that
/// turn the deterministic changelog into release notes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangelogCommit {
    pub oid: String,
    pub summary: String,
    /// The message after the summary line.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    pub author: String,
    /// A pull or merge request number found in the message (`Merge pull request #12`,
    /// `See merge request group/repo!12`, `(#12)`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<u64>,
}

impl ChangelogCommit {
    /// A commit from its full message: the first line is the summary, the rest (trimmed) the
    /// body, and the pull or merge request number is read by [`pull_number`].
    pub fn from_message(oid: &str, message: &str, author: &str) -> Self {
        let message = message.trim();
        let (summary, body) = match message.split_once('\n') {
            Some((s, b)) => (s.trim_end(), b.trim()),
            None => (message, ""),
        };
        ChangelogCommit {
            oid: oid.to_owned(),
            summary: summary.to_owned(),
            body: body.to_owned(),
            author: author.to_owned(),
            pull: pull_number(message),
        }
    }
}

/// The number of the pull or merge request a commit message names: `Merge pull request #12
/// from ...` (GitHub), `See merge request group/repo!12` (GitLab, in the body) or a trailing
/// `(#12)` on the summary (squash merges). The summary is looked at first, then the body.
pub fn pull_number(message: &str) -> Option<u64> {
    fn digits(s: &str) -> Option<u64> {
        let n: String = s.chars().take_while(char::is_ascii_digit).collect();
        if n.is_empty() { None } else { n.parse().ok() }
    }
    let message = message.trim();
    let (summary, body) = message.split_once('\n').unwrap_or((message, ""));
    let summary = summary.trim();
    if let Some(rest) = summary.strip_prefix("Merge pull request #")
        && let Some(n) = digits(rest)
    {
        return Some(n);
    }
    if let Some(inner) = summary.strip_suffix(')')
        && let Some((_, n)) = inner.rsplit_once("(#")
        && n.chars().all(|c| c.is_ascii_digit())
        && let Some(n) = digits(n)
    {
        return Some(n);
    }
    for line in std::iter::once(summary).chain(body.lines()) {
        if let Some(rest) = line.trim().strip_prefix("See merge request ")
            && let Some((_, n)) = rest.rsplit_once('!')
            && let Some(n) = digits(n)
        {
            return Some(n);
        }
    }
    None
}

/// Everything deterministic about a range that a writer may build on: the changelog as `rt
/// changelog` writes it, its parts as data, the distribution's sections and the commits. A
/// writer must not state anything that is not in here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangelogFacts {
    pub from: String,
    pub to: String,
    pub title: String,
    pub spec: SpecChanges,
    pub collection: rt_format::diff::CollectionDiff,
    /// Markdown of each distribution section that had something to say (`ChangelogSection`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<ChangelogCommit>,
    /// [`changelog_with`] of the range.
    pub markdown: String,
}

/// The facts of a range: [`changelog_with`] and its parts. `commits` come from the shell
/// (`rt-git` natively, isomorphic-git in the browser).
pub fn changelog_facts(
    old: &Side<'_>,
    new: &Side<'_>,
    from: &str,
    to: &str,
    sections: &[&dyn ChangelogSection],
    commits: Vec<ChangelogCommit>,
) -> Result<ChangelogFacts> {
    let old_spec = load(old.files, old.read)?;
    let new_spec = load(new.files, new.read)?;
    let spec = rt_format::contract::changes(
        old_spec.as_ref().map(|l| l.bytes.as_slice()),
        new_spec.as_ref().map(|l| l.bytes.as_slice()),
        new.files,
    )?;
    let collection = rt_format::diff::diff(old.files, new.files)?;
    let name = new_spec
        .as_ref()
        .or(old_spec.as_ref())
        .map(|l| l.spec.title.as_str());
    let title = changelog_title(name, &spec, from, to);
    let mut extra = Vec::new();
    for section in sections {
        if let Some(text) = section.section(old, new, from, to)? {
            let text = text.trim();
            if !text.is_empty() {
                extra.push(text.to_owned());
            }
        }
    }
    let markdown = changelog_with(old, new, from, to, sections)?;
    Ok(ChangelogFacts {
        from: from.to_owned(),
        to: to.to_owned(),
        title,
        spec,
        collection,
        sections: extra,
        commits,
        markdown,
    })
}

/// Another way to write the changelog of a range, chosen with a flag of `rt changelog` (since
/// wave 16), e.g. `rt changelog v1..v2 --notes --audience developers`. A distribution passes
/// writers in `rt_cli::Distribution::changelog_writers`; the builds of this repository pass
/// none, and `rt changelog` without the flag is unchanged. Called outside any async runtime.
pub trait ChangelogWriter {
    /// The flag without dashes, e.g. `notes` for `--notes`.
    fn flag(&self) -> &'static str;
    /// Options it takes as `--<name> <value>` (names without dashes).
    fn options(&self) -> &'static [&'static str];
    /// Lines for `rt changelog --help`.
    fn help(&self) -> &'static str;
    /// The text to write instead of [`ChangelogFacts::markdown`]; an error ends `rt changelog`
    /// with its message (exit code 2).
    fn write(&self, facts: &ChangelogFacts, options: &[(String, String)]) -> Result<String>;
}
