// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The DEV/QAS/PRD template and the collection tab `landscape` that fills it in.
//!
//! Each system becomes one environment with:
//! - `sapSystem` (when a system name is given), `baseHost` (`https://host[:port]`),
//!   `baseUrl` (`{{baseHost}}` plus the service path, if any), `sapClient`, `sapLanguage`,
//!   `user`;
//! - `password` as a secret without value (the user sets it; plugins never write secrets);
//! - PRD also gets `rtProduction=true`, and its name ends in `PRD`. The core has no notion of
//!   a production environment yet (`API-WUENSCHE.md`); the variable is the marker until then.
//!
//! The optional Hub sandbox environment has `baseUrl` on `sandbox.api.sap.com`,
//! `sapSandbox=true` (see [`super::params`]) and `apiKey` as a secret.
//!
//! The tab sees only the selected environment (`PluginContext`); it cannot list the
//! collection's environments (`API-WUENSCHE.md`). An existing environment of the same name
//! keeps its variables and gets only the missing ones (host rule, `API.md`).

use rt_plugin_sdk::api::{
    Block, FieldKind, FormField, PluginContext, PluginEnvironment, PluginVariable, View, ViewReply,
};
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab};
use serde::{Deserialize, Serialize};

use super::params::{SAP_CLIENT, SAP_LANGUAGE, SAP_SANDBOX};

/// Sandbox host of the SAP Business Accelerator Hub for S/4HANA Cloud APIs.
pub const SANDBOX_ROOT: &str = "https://sandbox.api.sap.com/s4hanacloud";
pub const SANDBOX_ENVIRONMENT: &str = "API Hub Sandbox";
/// Marks the production environment (until the core knows one, `API-WUENSCHE.md`).
pub const PRODUCTION_VARIABLE: &str = "rtProduction";

const FORM: &str = "landscape";
pub const STAGES: [&str; 3] = ["DEV", "QAS", "PRD"];

/// One system of the landscape.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct System {
    /// `https://host[:port]`, without trailing `/`; empty when not known yet.
    pub host: String,
    pub client: String,
}

/// What the template is made from; also the tab's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Landscape {
    /// E.g. `S4H`; prefixes the environment names (`S4H DEV`). May be empty.
    pub system: String,
    /// DEV, QAS, PRD.
    pub systems: [System; 3],
    pub language: String,
    pub user: String,
    /// Service path after the host, e.g. `/sap/opu/odata/sap/API_PRODUCT_SRV`; may be empty.
    pub service_path: String,
    /// Also create the Hub sandbox environment.
    pub sandbox: bool,
}

impl Default for Landscape {
    fn default() -> Self {
        Self {
            system: String::new(),
            systems: Default::default(),
            language: "EN".to_owned(),
            user: String::new(),
            service_path: String::new(),
            sandbox: false,
        }
    }
}

impl Landscape {
    /// The environment name of stage `i` (0 = DEV).
    pub fn name(&self, i: usize) -> String {
        if self.system.is_empty() {
            STAGES[i].to_owned()
        } else {
            format!("{} {}", self.system, STAGES[i])
        }
    }

    /// The environments: one per system with a host (all three with `all`), then the sandbox
    /// if asked for.
    pub fn environments(&self, all: bool) -> Vec<PluginEnvironment> {
        let mut out = Vec::new();
        for (i, system) in self.systems.iter().enumerate() {
            if !all && system.host.is_empty() {
                continue;
            }
            let mut variables = Vec::new();
            if !self.system.is_empty() {
                variables.push(var("sapSystem", &self.system));
            }
            variables.push(var("baseHost", &system.host));
            variables.push(var(
                "baseUrl",
                &format!("{{{{baseHost}}}}{}", self.service_path),
            ));
            variables.push(var(SAP_CLIENT, &system.client));
            variables.push(var(SAP_LANGUAGE, &self.language));
            variables.push(var("user", &self.user));
            variables.push(secret("password"));
            if STAGES[i] == "PRD" {
                variables.push(var(PRODUCTION_VARIABLE, "true"));
            }
            out.push(PluginEnvironment {
                name: self.name(i),
                variables,
            });
        }
        if self.sandbox {
            out.push(sandbox_environment(&format!(
                "{SANDBOX_ROOT}{}",
                self.service_path
            )));
        }
        out
    }

    fn from_input(action: &Action) -> std::result::Result<Self, Vec<String>> {
        let input = |name: &str| action.input(name).unwrap_or_default().trim().to_owned();
        let mut errors = Vec::new();
        let system = input("system");
        if !system
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            errors.push("System name: letters, digits, `_` and `-` only (e.g. S4H)".to_owned());
        }
        let mut systems: [System; 3] = Default::default();
        for (i, stage) in STAGES.iter().enumerate() {
            let key = stage.to_ascii_lowercase();
            let host = input(&format!("{key}_host"));
            let client = input(&format!("{key}_client"));
            match normalize_host(&host) {
                Ok(h) => systems[i].host = h,
                Err(e) => errors.push(format!("{stage} host: {e}")),
            }
            let three_digits = client.len() == 3 && client.bytes().all(|b| b.is_ascii_digit());
            if !client.is_empty() && !three_digits {
                errors.push(format!("{stage} client: three digits, e.g. 100"));
            }
            systems[i].client = client;
        }
        let sandbox = input("sandbox") == "true";
        if systems.iter().all(|s| s.host.is_empty()) && !sandbox {
            errors.push("Enter the host of at least one system".to_owned());
        }
        let language = input("language").to_ascii_uppercase();
        if !(1..=2).contains(&language.len())
            || !language.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            errors.push("Language: an SAP language key such as EN or DE".to_owned());
        }
        let mut service_path = input("service_path");
        if !service_path.is_empty() {
            if service_path.contains(char::is_whitespace) || service_path.contains("://") {
                errors.push("Service path: a path such as /sap/opu/odata/sap/API_X".to_owned());
            }
            if !service_path.starts_with('/') {
                service_path.insert(0, '/');
            }
            while service_path.ends_with('/') {
                service_path.pop();
            }
        }
        let landscape = Landscape {
            system,
            systems,
            language,
            user: input("user"),
            service_path,
            sandbox,
        };
        if errors.is_empty() {
            Ok(landscape)
        } else {
            Err(errors)
        }
    }

    /// The form values as entered (for showing the form again).
    fn raw(action: &Action) -> Self {
        let input = |name: &str| action.input(name).unwrap_or_default().to_owned();
        let mut systems: [System; 3] = Default::default();
        for (i, stage) in STAGES.iter().enumerate() {
            let key = stage.to_ascii_lowercase();
            systems[i] = System {
                host: input(&format!("{key}_host")),
                client: input(&format!("{key}_client")),
            };
        }
        Landscape {
            system: input("system"),
            systems,
            language: input("language"),
            user: input("user"),
            service_path: input("service_path"),
            sandbox: input("sandbox") == "true",
        }
    }
}

/// The Hub sandbox environment for `base_url` (the sandbox service root).
pub fn sandbox_environment(base_url: &str) -> PluginEnvironment {
    PluginEnvironment {
        name: SANDBOX_ENVIRONMENT.to_owned(),
        variables: vec![
            var("baseUrl", base_url),
            var(SAP_SANDBOX, "true"),
            secret("apiKey"),
        ],
    }
}

fn var(name: &str, value: &str) -> PluginVariable {
    PluginVariable {
        name: name.to_owned(),
        value: value.to_owned(),
        secret: false,
    }
}

fn secret(name: &str) -> PluginVariable {
    PluginVariable {
        name: name.to_owned(),
        value: String::new(),
        secret: true,
    }
}

/// `s4d.example.corp:44300` → `https://s4d.example.corp:44300`; empty stays empty.
pub fn normalize_host(host: &str) -> std::result::Result<String, &'static str> {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() {
        return Ok(String::new());
    }
    if host.contains(char::is_whitespace) || host.contains("***") {
        return Err("no spaces, e.g. s4d.example.corp:44300");
    }
    let with_scheme = if host.contains("://") {
        host.to_owned()
    } else {
        format!("https://{host}")
    };
    let (scheme, rest) = with_scheme.split_once("://").unwrap_or(("", ""));
    if !(scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        || rest.is_empty()
    {
        return Err("http or https, e.g. https://s4d.example.corp:44300");
    }
    if rest.contains(['/', '?', '#']) {
        return Err("only the host and port; the service path has its own field");
    }
    Ok(with_scheme)
}

fn value<'a>(context: Option<&'a PluginContext>, name: &str) -> Option<&'a str> {
    context?
        .variables
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The SAP service path of a URL (`/sap/opu/…` up to the service root), if it has one. The
/// variable may still be a template such as `{{baseHost}}/sap/opu/odata/sap/API_X`.
fn sap_service_path(url: &str) -> Option<String> {
    let start = url.to_ascii_lowercase().find("/sap/opu/")?;
    let target = crate::service::split(&format!("http://h{}", &url[start..]), &[])?;
    let root = target.service_root.trim_end_matches('/');
    Some(root["http://h".len()..].to_owned())
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let landscape = match tab.state.clone().map(serde_json::from_value::<Landscape>) {
        Some(Ok(l)) => l,
        _ => initial(tab.context.as_ref()),
    };
    Ok(ViewReply {
        view: View {
            blocks: page(tab.context.as_ref(), &landscape, None),
        },
        state: Some(serde_json::to_value(&landscape)?),
        ..ViewReply::default()
    })
}

pub fn action(action: Action) -> Result<ViewReply> {
    if action.action != FORM {
        rt_plugin_sdk::bail!("unknown action {}", action.action);
    }
    let landscape = match Landscape::from_input(&action) {
        Ok(l) => l,
        Err(errors) => {
            let entered = Landscape::raw(&action);
            return Ok(ViewReply {
                view: View {
                    blocks: page(action.context.as_ref(), &entered, Some(errors)),
                },
                state: Some(serde_json::to_value(&entered)?),
                ..ViewReply::default()
            });
        }
    };
    let environments = landscape.environments(false);
    let mut blocks = vec![Block::Heading {
        text: "SAP system landscape".to_owned(),
    }];
    blocks.push(Block::Notice {
        level: NoteLevel::Info,
        text: created_notice(&landscape, &environments),
    });
    blocks.push(Block::Table {
        columns: vec![
            "Environment".into(),
            "Host".into(),
            "Client".into(),
            "Language".into(),
        ],
        rows: environments
            .iter()
            .map(|e| {
                let get = |n: &str| {
                    e.variables
                        .iter()
                        .find(|v| v.name == n)
                        .map(|v| v.value.clone())
                        .unwrap_or_default()
                };
                let host = if e.name == SANDBOX_ENVIRONMENT {
                    get("baseUrl")
                } else {
                    get("baseHost")
                };
                vec![e.name.clone(), host, get(SAP_CLIENT), get(SAP_LANGUAGE)]
            })
            .collect(),
    });
    blocks.extend(form(&landscape));
    Ok(ViewReply {
        view: View { blocks },
        state: Some(serde_json::to_value(&landscape)?),
        add_environments: environments,
        ..ViewReply::default()
    })
}

fn created_notice(landscape: &Landscape, environments: &[PluginEnvironment]) -> String {
    let names: Vec<&str> = environments.iter().map(|e| e.name.as_str()).collect();
    let mut text = format!(
        "Adds the environments {} to the collection (unsaved; an existing environment of the \
         same name keeps its values and gets only the missing variables). Set the secret \
         values in Environments: password",
        names.join(", ")
    );
    if landscape.sandbox {
        text.push_str(", apiKey (sandbox)");
    }
    text.push('.');
    if environments
        .iter()
        .any(|e| e.variables.iter().any(|v| v.name == PRODUCTION_VARIABLE))
    {
        text.push_str(&format!(
            " {} is marked as production ({PRODUCTION_VARIABLE}=true).",
            landscape.name(2)
        ));
    }
    text.push_str(
        " Requests authenticate with these values when the collection's auth is Basic with \
         {{user}} and {{password}}; sap-client and sap-language are added to every SAP request.",
    );
    text
}

/// Defaults for the form: what the selected environment already says.
fn initial(context: Option<&PluginContext>) -> Landscape {
    let mut l = Landscape::default();
    if let Some(lang) = value(context, SAP_LANGUAGE).filter(|v| !v.is_empty()) {
        lang.clone_into(&mut l.language);
    }
    if let Some(user) = value(context, "user") {
        user.clone_into(&mut l.user);
    }
    if let Some(system) = value(context, "sapSystem") {
        system.clone_into(&mut l.system);
    }
    if let Some(path) = value(context, "baseUrl").and_then(sap_service_path) {
        l.service_path = path;
    }
    l
}

fn page(
    context: Option<&PluginContext>,
    landscape: &Landscape,
    errors: Option<Vec<String>>,
) -> Vec<Block> {
    let mut blocks = vec![
        Block::Heading {
            text: "SAP system landscape".to_owned(),
        },
        Block::Text {
            text: "Creates one environment per SAP system (DEV, QAS, PRD) with host, client \
                   and language. The plugin adds sap-client and sap-language to every SAP \
                   request from the selected environment. Passwords and API keys are created \
                   as secrets without value."
                .to_owned(),
        },
    ];
    match context {
        Some(c) => {
            let mut rows = vec![(
                "Selected environment".to_owned(),
                c.environment.clone().unwrap_or_else(|| "(none)".to_owned()),
            )];
            for name in [SAP_CLIENT, SAP_LANGUAGE, "baseUrl"] {
                if let Some(v) = value(context, name) {
                    rows.push((name.to_owned(), v.to_owned()));
                }
            }
            blocks.push(Block::KeyValue { rows });
            if value(context, PRODUCTION_VARIABLE) == Some("true") {
                blocks.push(Block::Notice {
                    level: NoteLevel::Warning,
                    text: "The selected environment is a production system.".to_owned(),
                });
            }
        }
        None => blocks.push(Block::Text {
            text: "This app does not pass the selected environment to plugins.".to_owned(),
        }),
    }
    if let Some(errors) = errors {
        blocks.push(Block::Notice {
            level: NoteLevel::Error,
            text: errors.join("; "),
        });
    }
    blocks.extend(form(landscape));
    blocks
}

fn form(l: &Landscape) -> Vec<Block> {
    let text = |name: &str, label: &str, value: &str| FormField {
        name: name.to_owned(),
        label: label.to_owned(),
        kind: FieldKind::Text,
        value: value.to_owned(),
        options: Vec::new(),
    };
    let mut fields = vec![text("system", "System name (e.g. S4H)", &l.system)];
    for (i, stage) in STAGES.iter().enumerate() {
        let key = stage.to_ascii_lowercase();
        fields.push(text(
            &format!("{key}_host"),
            &format!("{stage} host (e.g. s4{}.example.corp:44300)", &key[..1]),
            &l.systems[i].host,
        ));
        fields.push(text(
            &format!("{key}_client"),
            &format!("{stage} client (e.g. 100)"),
            &l.systems[i].client,
        ));
    }
    fields.push(text("language", "Language (sap-language)", &l.language));
    fields.push(text(
        "user",
        "User (Basic auth; the password is a secret)",
        &l.user,
    ));
    fields.push(text(
        "service_path",
        "Service path (optional, e.g. /sap/opu/odata/sap/API_PRODUCT_SRV)",
        &l.service_path,
    ));
    fields.push(FormField {
        name: "sandbox".to_owned(),
        label: "Also add the SAP Business Accelerator Hub sandbox (API key as a secret)".to_owned(),
        kind: FieldKind::Checkbox,
        value: l.sandbox.to_string(),
        options: Vec::new(),
    });
    vec![Block::Form {
        id: FORM.to_owned(),
        fields,
        submit: "Create environments".to_owned(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            normalize_host("s4d.corp:44300/").unwrap(),
            "https://s4d.corp:44300"
        );
        assert_eq!(normalize_host("http://h").unwrap(), "http://h");
        assert_eq!(normalize_host(" ").unwrap(), "");
        assert!(normalize_host("ftp://h").is_err());
        assert!(normalize_host("https://h/sap/opu").is_err());
        assert!(normalize_host("a b").is_err());
    }

    #[test]
    fn service_paths() {
        assert_eq!(
            sap_service_path("https://h:1/sap/opu/odata/sap/API_X/").as_deref(),
            Some("/sap/opu/odata/sap/API_X")
        );
        assert_eq!(
            sap_service_path("{{baseHost}}/sap/opu/odata/sap/API_X/A_X").as_deref(),
            Some("/sap/opu/odata/sap/API_X")
        );
        assert_eq!(sap_service_path("https://h/odata/v4/x/"), None);
    }
}
