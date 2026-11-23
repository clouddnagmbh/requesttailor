// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Variable completion: every variable a request, a folder or the collection sees, layer by
//! layer, for the `{{` list of the editor ([`scope`]), and new variables of the collection or
//! a folder ([`define`]).
//!
//! The layers and their order are those of `rt_core::prepare` (Global < Collection <
//! Environment < Folder, outermost first < Request < Runtime), but unlike the core's scope
//! this lists names without a value too: secrets whose value lives in the keychain or a
//! secret manager, and variables declared without a value. Secret values never appear:
//! a secret has `value: None`, and every other value is masked against the secret values
//! the shell passed in [`SendInput::secrets`] (a runtime value copied from a secret).

use rt_core::model::{
    Collection, Environment, Folder, Item, RequestDefaults, RequestSpec, SecretProvider, Variable,
};
use rt_core::vars::VarSource;
use serde::{Deserialize, Serialize};

use crate::{AppError, SendInput};

/// One variable as the completion list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeVar {
    pub name: String,
    /// The value as defined (not resolved), secret values masked; `None` for secrets and for
    /// variables declared without a value.
    pub value: Option<String>,
    pub source: VarSource,
    pub secret: bool,
    /// The secret manager an external secret of the environment comes from.
    pub external: Option<SecretProvider>,
    /// A higher layer, or a later definition in the same layer, defines the same name: this
    /// one is not what `{{name}}` gives.
    pub overridden: bool,
}

/// What [`scope`] returns.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VariableScope {
    /// Lowest precedence first; within a layer in file order.
    pub variables: Vec<ScopeVar>,
    /// Names of `{{placeholders}}` used somewhere in the collection (requests, collection and
    /// folder defaults, environments; not scripts) that no layer here defines. Sorted, at
    /// most [`UNDEFINED_LIMIT`].
    pub undefined: Vec<String>,
}

/// At most this many undefined names are listed.
pub const UNDEFINED_LIMIT: usize = 100;

/// The variables that `path` sees with `input`: `path` is a request path, a folder path, or
/// `""` for the collection (environment values, collection settings). `input.vars` is the
/// runtime layer (the shells put the session's variables there), `input.secrets` only serve
/// to mask values. An environment that does not exist adds no layer.
pub fn scope(
    collection: &Collection,
    path: &str,
    input: &SendInput,
) -> Result<VariableScope, AppError> {
    let (folders, request) = locate(collection, path)?;
    let environment = input
        .environment
        .as_deref()
        .and_then(|name| collection.environment(name));

    let mut masks: Vec<String> = input
        .secrets
        .iter()
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
        .collect();
    masks.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    masks.dedup();
    let masked = |value: &str| crate::workspace::mask(value, &masks);

    let mut out: Vec<ScopeVar> = Vec::new();
    let mut layer = |source: VarSource, vars: &[Variable]| {
        for v in vars.iter().filter(|v| v.enabled) {
            out.push(ScopeVar {
                name: v.name.clone(),
                value: if v.secret {
                    None
                } else {
                    v.value.as_deref().map(&masked)
                },
                source: source.clone(),
                secret: v.secret,
                external: None,
                overridden: false,
            });
        }
    };
    layer(VarSource::Collection, &collection.defaults.vars);
    if let Some(env) = environment {
        let source = VarSource::Environment {
            name: env.name.clone(),
        };
        // External secrets first: a variable of the environment with the same name (its
        // keychain value) wins, as when sending.
        if let Some(external) = &env.external_secrets {
            for s in external.variables.iter().filter(|s| s.enabled) {
                out.push(ScopeVar {
                    name: s.name.clone(),
                    value: None,
                    source: source.clone(),
                    secret: true,
                    external: Some(external.provider),
                    overridden: false,
                });
            }
        }
        let mut layer = |vars: &[Variable]| {
            for v in vars.iter().filter(|v| v.enabled) {
                out.push(ScopeVar {
                    name: v.name.clone(),
                    value: if v.secret {
                        None
                    } else {
                        v.value.as_deref().map(&masked)
                    },
                    source: source.clone(),
                    secret: v.secret,
                    external: None,
                    overridden: false,
                });
            }
        };
        layer(&env.variables);
    }
    for folder in &folders {
        let source = VarSource::Folder {
            path: folder.path.clone(),
        };
        for v in folder.defaults.vars.iter().filter(|v| v.enabled) {
            out.push(var(source.clone(), v, &masked));
        }
    }
    if let Some(spec) = request {
        for v in spec.vars.iter().filter(|v| v.enabled) {
            out.push(var(VarSource::Request, v, &masked));
        }
    }
    // A runtime value for a name declared secret in any layer stays secret.
    for (name, value) in &input.vars {
        let secret = out.iter().any(|v| v.secret && v.name == *name);
        out.push(ScopeVar {
            name: name.clone(),
            value: (!secret).then(|| masked(value)),
            source: VarSource::Runtime,
            secret,
            external: None,
            overridden: false,
        });
    }

    // Overridden: a later entry has the same name.
    let mut seen = std::collections::HashSet::new();
    for v in out.iter_mut().rev() {
        v.overridden = !seen.insert(v.name.clone());
    }

    let mut undefined: Vec<String> = used_names(collection)
        .into_iter()
        .filter(|name| !seen.contains(name))
        .collect();
    undefined.sort();
    undefined.dedup();
    undefined.truncate(UNDEFINED_LIMIT);
    Ok(VariableScope {
        variables: out,
        undefined,
    })
}

fn var(source: VarSource, v: &Variable, masked: &impl Fn(&str) -> String) -> ScopeVar {
    ScopeVar {
        name: v.name.clone(),
        value: if v.secret {
            None
        } else {
            v.value.as_deref().map(masked)
        },
        source,
        secret: v.secret,
        external: None,
        overridden: false,
    }
}

/// The folder chain (outermost first) and the request of `path`.
fn locate<'a>(
    collection: &'a Collection,
    path: &str,
) -> Result<(Vec<&'a Folder>, Option<&'a RequestSpec>), AppError> {
    if path.is_empty() {
        return Ok((Vec::new(), None));
    }
    if let Some(context) = collection.request(path) {
        return Ok((context.folders, Some(&context.request.spec)));
    }
    fn walk<'a>(items: &'a [Item], path: &str, chain: &mut Vec<&'a Folder>) -> bool {
        for item in items {
            if let Item::Folder(f) = item {
                chain.push(f);
                if f.path == path || walk(&f.items, path, chain) {
                    return true;
                }
                chain.pop();
            }
        }
        false
    }
    let mut chain = Vec::new();
    if walk(&collection.items, path, &mut chain) {
        Ok((chain, None))
    } else {
        Err(AppError::RequestNotFound(path.to_owned()))
    }
}

/// Placeholder names in the collection's requests (without scripts), collection and folder
/// defaults (without scripts) and environment values.
fn used_names(collection: &Collection) -> Vec<String> {
    fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => placeholders(s, out),
            serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
            serde_json::Value::Object(map) => map
                .iter()
                .filter(|(k, _)| *k != "scripts")
                .for_each(|(_, v)| strings(v, out)),
            _ => {}
        }
    }
    fn defaults(d: &RequestDefaults, out: &mut Vec<String>) {
        if let Ok(v) = serde_json::to_value(d) {
            strings(&v, out);
        }
    }
    fn items(list: &[Item], out: &mut Vec<String>) {
        for item in list {
            match item {
                Item::Folder(f) => {
                    defaults(&f.defaults, out);
                    items(&f.items, out);
                }
                Item::Request(r) => {
                    if let Ok(v) = serde_json::to_value(&r.spec) {
                        strings(&v, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    defaults(&collection.defaults, &mut out);
    items(&collection.items, &mut out);
    for env in &collection.environments {
        env_names(env, &mut out);
    }
    out
}

fn env_names(env: &Environment, out: &mut Vec<String>) {
    for v in &env.variables {
        if let Some(value) = &v.value {
            placeholders(value, out);
        }
    }
}

/// `{{name}}` occurrences, same matches as rt-core (`\{\{([^}]+)\}\}`), without names that
/// cannot be defined ([`valid_name`]) or start with `$` (dynamic values).
fn placeholders(text: &str, out: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if after[end..].starts_with("}}") {
            if valid_name(name) && !name.starts_with('$') {
                out.push(name.to_owned());
            }
            rest = &after[end + 2..];
        } else {
            rest = &after[end + 1..];
        }
    }
}

/// Adds `variable` to the collection (`folder` is `""`) or a folder, or replaces the one of
/// the same name there. Only plain variables: a secret belongs in an environment, where its
/// value lives in the keychain (`SecretsApi`); a name must be usable as `{{name}}`.
pub fn define(
    collection: &Collection,
    folder: &str,
    variable: &Variable,
) -> Result<Vec<Variable>, AppError> {
    if variable.secret {
        return Err(AppError::Unsupported(
            "secret variables belong in an environment".into(),
        ));
    }
    if !valid_name(&variable.name) {
        return Err(AppError::Format(format!(
            "not a variable name: {:?}",
            variable.name
        )));
    }
    let current = if folder.is_empty() {
        &collection.defaults.vars
    } else {
        let (chain, request) =
            locate(collection, folder).map_err(|_| AppError::FolderNotFound(folder.to_owned()))?;
        match (chain.last(), request) {
            (Some(f), None) => &f.defaults.vars,
            _ => return Err(AppError::FolderNotFound(folder.to_owned())),
        }
    };
    let mut vars = current.clone();
    match vars.iter_mut().find(|v| v.name == variable.name) {
        Some(v) => *v = variable.clone(),
        None => vars.push(variable.clone()),
    }
    Ok(vars)
}

/// A name `{{name}}` resolves: not blank, no braces, no surrounding whitespace.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.trim() == name
        && !name.contains(['{', '}'])
        && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use rt_core::model::{
        Environment, ExternalSecret, ExternalSecrets, Folder, Item, RequestDefaults, RequestEntry,
        RequestSpec, SecretProvider, Variable,
    };
    use rt_core::vars::VarSource;

    use super::*;

    fn secret(name: &str) -> Variable {
        Variable {
            name: name.into(),
            value: None,
            enabled: true,
            secret: true,
        }
    }

    fn collection() -> Collection {
        let request = RequestSpec {
            name: "List".into(),
            url: "{{baseUrl}}/users/{{userId}}?q={{missing}}".into(),
            vars: vec![Variable::new("page", "1")],
            ..RequestSpec::default()
        };
        Collection {
            name: "Demo".into(),
            defaults: RequestDefaults {
                vars: vec![
                    Variable::new("baseUrl", "http://c"),
                    Variable::new("apiVersion", "v2"),
                ],
                ..RequestDefaults::default()
            },
            items: vec![Item::Folder(Folder {
                name: "users".into(),
                path: "users".into(),
                defaults: RequestDefaults {
                    vars: vec![Variable::new("userId", "42"), Variable::new("page", "0")],
                    ..RequestDefaults::default()
                },
                items: vec![Item::Request(RequestEntry {
                    path: "users/list.yml".into(),
                    spec: request,
                })],
                ..Folder::default()
            })],
            environments: vec![Environment {
                name: "Local".into(),
                variables: vec![
                    Variable::new("baseUrl", "http://local"),
                    secret("token"),
                    Variable {
                        enabled: false,
                        ..Variable::new("off", "x")
                    },
                ],
                client_certificates: Vec::new(),
                external_secrets: Some(ExternalSecrets {
                    provider: SecretProvider::OnePassword,
                    variables: vec![ExternalSecret {
                        name: "dbPassword".into(),
                        reference: "op://vault/db/password".into(),
                        enabled: true,
                    }],
                }),
            }],
            ..Collection::default()
        }
    }

    fn input(vars: &[(&str, &str)], secrets: &[(&str, &str)]) -> SendInput {
        SendInput {
            environment: Some("Local".into()),
            vars: vars
                .iter()
                .map(|(n, v)| ((*n).into(), (*v).into()))
                .collect(),
            secrets: secrets
                .iter()
                .map(|(n, v)| ((*n).into(), (*v).into()))
                .collect(),
        }
    }

    fn row<'a>(s: &'a VariableScope, name: &str, source: &VarSource) -> &'a ScopeVar {
        s.variables
            .iter()
            .find(|v| v.name == name && v.source == *source)
            .unwrap_or_else(|| panic!("{name} in {source:?}"))
    }

    #[test]
    fn lists_every_layer_in_precedence_order() {
        let s = scope(
            &collection(),
            "users/list.yml",
            &input(&[("session", "s1")], &[]),
        )
        .unwrap();
        let order: Vec<(&str, String)> = s
            .variables
            .iter()
            .map(|v| (v.name.as_str(), format!("{:?}", v.source)))
            .collect();
        assert_eq!(
            order.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            [
                "baseUrl",
                "apiVersion",
                "dbPassword",
                "baseUrl",
                "token",
                "userId",
                "page",
                "page",
                "session"
            ]
        );
        let env = VarSource::Environment {
            name: "Local".into(),
        };
        assert!(row(&s, "baseUrl", &VarSource::Collection).overridden);
        assert!(!row(&s, "baseUrl", &env).overridden);
        let folder = VarSource::Folder {
            path: "users".into(),
        };
        assert!(row(&s, "page", &folder).overridden);
        assert!(!row(&s, "page", &VarSource::Request).overridden);
        assert_eq!(
            row(&s, "session", &VarSource::Runtime).value.as_deref(),
            Some("s1")
        );
        assert!(!s.variables.iter().any(|v| v.name == "off"));
        assert_eq!(s.undefined, ["missing"]);
    }

    #[test]
    fn never_shows_secret_values() {
        let s = scope(
            &collection(),
            "users/list.yml",
            &input(
                &[("token", "t0p"), ("copy", "Bearer t0p")],
                &[("token", "t0p")],
            ),
        )
        .unwrap();
        let env = VarSource::Environment {
            name: "Local".into(),
        };
        let token = row(&s, "token", &env);
        assert!(token.secret && token.value.is_none());
        let external = row(&s, "dbPassword", &env);
        assert_eq!(external.external, Some(SecretProvider::OnePassword));
        assert!(external.secret && external.value.is_none());
        // A runtime value for a declared secret stays secret; copies are masked.
        let runtime = row(&s, "token", &VarSource::Runtime);
        assert!(runtime.secret && runtime.value.is_none());
        assert_eq!(
            row(&s, "copy", &VarSource::Runtime).value.as_deref(),
            Some("Bearer ***")
        );
        assert!(!serde_json::to_string(&s).unwrap().contains("t0p"));
    }

    #[test]
    fn folders_and_the_collection_see_their_own_layers() {
        let s = scope(&collection(), "users", &input(&[], &[])).unwrap();
        assert!(s.variables.iter().any(|v| v.name == "userId"));
        assert!(!s.variables.iter().any(|v| v.source == VarSource::Request));
        let root = scope(&collection(), "", &input(&[], &[])).unwrap();
        assert!(!root.variables.iter().any(|v| v.name == "userId"));
        assert!(root.undefined.contains(&"userId".to_owned()));
        assert!(matches!(
            scope(&collection(), "nope", &input(&[], &[])),
            Err(AppError::RequestNotFound(_))
        ));
    }

    #[test]
    fn define_adds_or_replaces_plain_variables_only() {
        let c = collection();
        let vars = define(&c, "", &Variable::new("tenant", "a")).unwrap();
        assert_eq!(vars.last().unwrap(), &Variable::new("tenant", "a"));
        let vars = define(&c, "users", &Variable::new("userId", "7")).unwrap();
        assert_eq!(
            vars,
            [Variable::new("userId", "7"), Variable::new("page", "0")]
        );
        assert!(define(&c, "", &secret("x")).is_err());
        assert!(define(&c, "", &Variable::new("a}b", "1")).is_err());
        assert!(define(&c, "", &Variable::new(" a", "1")).is_err());
        assert!(matches!(
            define(&c, "users/list.yml", &Variable::new("a", "1")),
            Err(AppError::FolderNotFound(_))
        ));
    }

    #[test]
    fn placeholders_match_like_the_core() {
        let mut out = Vec::new();
        placeholders("{{a}}/{{}}/{{ b }}/{{{c}}/{{d}/{{$rand}}", &mut out);
        assert_eq!(out, ["a"]);
    }
}
