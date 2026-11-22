// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand D, collection tab `landscape`: the form and the DEV/QAS/PRD environments it adds.

use rt_plugin_odata::OData;
use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{
    Block, Call, FieldKind, PluginContext, PluginEnvironment, PluginVariable, TabTarget, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing::{self, MockHost};

fn target() -> TabTarget {
    TabTarget {
        request_path: String::new(),
        response_id: None,
    }
}

fn context(environment: Option<&str>, vars: &[(&str, &str)]) -> PluginContext {
    PluginContext {
        request_path: String::new(),
        environment: environment.map(str::to_owned),
        variables: vars
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn render(context: Option<PluginContext>, state: Option<serde_json::Value>) -> ViewReply {
    let _guard = MockHost::new().install();
    testing::call(
        &OData,
        &Call::RenderTab {
            tab: "landscape".into(),
            target: target(),
            spec: RequestSpec::default(),
            response: None,
            state,
            context,
        },
    )
    .unwrap()
}

fn submit(input: &[(&str, &str)], state: Option<serde_json::Value>) -> ViewReply {
    let _guard = MockHost::new().install();
    testing::call(
        &OData,
        &Call::Action {
            tab: "landscape".into(),
            action: "landscape".into(),
            target: target(),
            spec: RequestSpec::default(),
            response: None,
            state,
            input: input
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            context: Some(context(None, &[])),
        },
    )
    .unwrap()
}

fn form(reply: &ViewReply) -> (&str, Vec<(&str, FieldKind, &str)>) {
    reply
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Form { id, fields, .. } => Some((
                id.as_str(),
                fields
                    .iter()
                    .map(|f| (f.name.as_str(), f.kind, f.value.as_str()))
                    .collect(),
            )),
            _ => None,
        })
        .expect("a form")
}

fn notices(reply: &ViewReply) -> Vec<(NoteLevel, &str)> {
    reply
        .view
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Notice { level, text } => Some((*level, text.as_str())),
            _ => None,
        })
        .collect()
}

fn var(env: &PluginEnvironment, name: &str) -> Option<PluginVariable> {
    env.variables.iter().find(|v| v.name == name).cloned()
}

const FULL: &[(&str, &str)] = &[
    ("system", "S4H"),
    ("dev_host", "s4d.example.corp:44300"),
    ("dev_client", "100"),
    ("qas_host", "https://s4q.example.corp:44300/"),
    ("qas_client", "200"),
    ("prd_host", "s4p.example.corp"),
    ("prd_client", "300"),
    ("language", "en"),
    ("user", "RT_USER"),
    ("service_path", "/sap/opu/odata/sap/API_RT_PRODUCT_SRV/"),
    ("sandbox", "true"),
];

#[test]
fn renders_the_form_with_the_selected_environment() {
    let reply = render(
        Some(context(
            Some("S4H DEV"),
            &[
                ("sapClient", "100"),
                ("sapLanguage", "DE"),
                (
                    "baseUrl",
                    "{{baseHost}}/sap/opu/odata/sap/API_RT_PRODUCT_SRV",
                ),
            ],
        )),
        None,
    );
    assert!(
        matches!(&reply.view.blocks[0], Block::Heading { text } if text == "SAP system landscape")
    );
    let kv = reply
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::KeyValue { rows } => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert!(kv.contains(&("Selected environment".into(), "S4H DEV".into())));
    assert!(kv.contains(&("sapClient".into(), "100".into())));
    let (id, fields) = form(&reply);
    assert_eq!(id, "landscape");
    let names: Vec<&str> = fields.iter().map(|f| f.0).collect();
    assert_eq!(
        names,
        [
            "system",
            "dev_host",
            "dev_client",
            "qas_host",
            "qas_client",
            "prd_host",
            "prd_client",
            "language",
            "user",
            "service_path",
            "sandbox"
        ]
    );
    // Prefilled from the environment; the language defaults to EN otherwise.
    assert!(fields.contains(&("language", FieldKind::Text, "DE")));
    assert!(fields.contains(&(
        "service_path",
        FieldKind::Text,
        "/sap/opu/odata/sap/API_RT_PRODUCT_SRV"
    )));
    assert!(fields.contains(&("sandbox", FieldKind::Checkbox, "false")));
    assert!(reply.add_environments.is_empty());

    let plain = render(None, None);
    assert!(
        form(&plain)
            .1
            .contains(&("language", FieldKind::Text, "EN"))
    );
}

#[test]
fn production_environment_is_flagged_in_the_view() {
    let reply = render(
        Some(context(Some("S4H PRD"), &[("rtProduction", "true")])),
        None,
    );
    assert!(
        notices(&reply)
            .iter()
            .any(|(l, t)| *l == NoteLevel::Warning && t.contains("production"))
    );
}

#[test]
fn submit_adds_dev_qas_prd_and_the_sandbox() {
    let reply = submit(FULL, None);
    let names: Vec<&str> = reply
        .add_environments
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["S4H DEV", "S4H QAS", "S4H PRD", "API Hub Sandbox"]);

    let dev = &reply.add_environments[0];
    let value = |e: &PluginEnvironment, n: &str| var(e, n).map(|v| v.value);
    assert_eq!(
        value(dev, "baseHost").as_deref(),
        Some("https://s4d.example.corp:44300")
    );
    assert_eq!(
        value(dev, "baseUrl").as_deref(),
        Some("{{baseHost}}/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(value(dev, "sapClient").as_deref(), Some("100"));
    assert_eq!(value(dev, "sapLanguage").as_deref(), Some("EN"));
    assert_eq!(value(dev, "user").as_deref(), Some("RT_USER"));
    assert_eq!(value(dev, "sapSystem").as_deref(), Some("S4H"));
    assert_eq!(
        value(&reply.add_environments[1], "baseHost").as_deref(),
        Some("https://s4q.example.corp:44300")
    );

    // Secrets: created without value, never written by the plugin.
    for env in &reply.add_environments {
        for v in env.variables.iter().filter(|v| v.secret) {
            assert_eq!(v.value, "", "{} / {}", env.name, v.name);
        }
    }
    assert_eq!(
        var(dev, "password"),
        Some(PluginVariable {
            name: "password".into(),
            value: String::new(),
            secret: true
        })
    );

    // Only PRD is marked as production.
    let prod: Vec<&str> = reply
        .add_environments
        .iter()
        .filter(|e| var(e, "rtProduction").is_some_and(|v| v.value == "true"))
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(prod, ["S4H PRD"]);

    let sandbox = &reply.add_environments[3];
    assert_eq!(
        value(sandbox, "baseUrl").as_deref(),
        Some("https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(value(sandbox, "sapSandbox").as_deref(), Some("true"));
    assert!(var(sandbox, "apiKey").is_some_and(|v| v.secret && v.value.is_empty()));
    assert!(var(sandbox, "password").is_none());

    // The notice says what will be created and what is left to do.
    let n = notices(&reply);
    let (level, text) = n[0];
    assert_eq!(level, NoteLevel::Info);
    for part in [
        "S4H DEV, S4H QAS, S4H PRD, API Hub Sandbox",
        "password",
        "apiKey",
        "S4H PRD is marked as production",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    // The state keeps what was entered for the next render.
    let again = render(None, reply.state.clone());
    assert!(
        form(&again)
            .1
            .contains(&("dev_client", FieldKind::Text, "100"))
    );
}

#[test]
fn systems_without_host_are_left_out_and_names_need_no_system() {
    let reply = submit(
        &[
            ("dev_host", "s4d.example.corp"),
            ("dev_client", "100"),
            ("language", "EN"),
        ],
        None,
    );
    let names: Vec<&str> = reply
        .add_environments
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["DEV"]);
    assert_eq!(
        var(&reply.add_environments[0], "baseUrl")
            .map(|v| v.value)
            .as_deref(),
        Some("{{baseHost}}")
    );
}

#[test]
fn invalid_input_shows_errors_and_adds_nothing() {
    let reply = submit(
        &[
            ("system", "S4 H"),
            ("dev_host", "s4d.example.corp/sap/opu"),
            ("dev_client", "1000"),
            ("language", "English"),
        ],
        None,
    );
    assert!(reply.add_environments.is_empty());
    let n = notices(&reply);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].0, NoteLevel::Error);
    for part in ["System name", "DEV host", "DEV client", "Language"] {
        assert!(n[0].1.contains(part), "{part}: {}", n[0].1);
    }
    // The form keeps what was typed.
    assert!(
        form(&reply)
            .1
            .contains(&("dev_client", FieldKind::Text, "1000"))
    );

    let empty = submit(&[("language", "EN")], None);
    assert!(empty.add_environments.is_empty());
    assert!(notices(&empty)[0].1.contains("at least one system"));
}

#[test]
fn unknown_action_is_an_error() {
    let _guard = MockHost::new().install();
    let reply: Result<ViewReply, String> = testing::call(
        &OData,
        &Call::Action {
            tab: "landscape".into(),
            action: "nope".into(),
            target: target(),
            spec: RequestSpec::default(),
            response: None,
            state: None,
            input: Vec::new(),
            context: None,
        },
    );
    assert!(reply.is_err());
}
