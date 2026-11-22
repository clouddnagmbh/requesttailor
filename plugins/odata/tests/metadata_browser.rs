// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The entity browser (collection tab `entities`) through the plugin's JSON dispatch, as the
//! host calls it.

use rt_plugin_odata::OData;
use rt_plugin_odata::metadata::{parse, store};
use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{
    Block, Call, FieldKind, Permissions, PluginContext, PluginResponse, TabTarget, TreeItem,
    ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing::{self, MockHost};

const S4_V2: &str = include_str!("../testdata/s4/ZRT_SALES_SRV.edmx");
const S4_V4: &str = include_str!("../testdata/s4/zrt_salesorder_0001.edmx");
const CAP_V4: &str = include_str!("../testdata/cap-bookshop/admin-v4.edmx");

const V2_ROOT: &str = "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/";
const V4_ROOT: &str =
    "https://s4.example.com/sap/opu/odata4/sap/zrt_salesorder/srvd_a2x/sap/zrt_salesorder/0001/";

/// The manifest's permissions: network only `{request}`, storage, variables.
fn manifest_host() -> MockHost {
    MockHost::new().granted(Permissions {
        network: vec!["{request}".into()],
        storage: true,
        variables: true,
    })
}

fn target() -> TabTarget {
    TabTarget {
        request_path: String::new(),
        response_id: None,
    }
}

fn context(environment: Option<&str>, variables: &[(&str, &str)]) -> PluginContext {
    PluginContext {
        request_path: String::new(),
        environment: environment.map(str::to_owned),
        variables: variables
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
    }
}

fn render(state: Option<serde_json::Value>, ctx: Option<PluginContext>) -> ViewReply {
    testing::call(
        &OData,
        &Call::RenderTab {
            tab: "entities".into(),
            target: target(),
            spec: RequestSpec::default(),
            response: None,
            state,
            context: ctx,
        },
    )
    .unwrap()
}

fn act(
    action: &str,
    input: &[(&str, &str)],
    ctx: Option<PluginContext>,
) -> Result<ViewReply, String> {
    testing::call(
        &OData,
        &Call::Action {
            tab: "entities".into(),
            action: action.into(),
            target: target(),
            spec: RequestSpec::default(),
            response: None,
            state: None,
            input: input
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            context: ctx,
        },
    )
}

fn notices(reply: &ViewReply) -> Vec<(NoteLevel, String)> {
    reply
        .view
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Notice { level, text } => Some((*level, text.clone())),
            _ => None,
        })
        .collect()
}

fn action_ids(reply: &ViewReply) -> Vec<String> {
    reply
        .view
        .blocks
        .iter()
        .flat_map(|b| match b {
            Block::Actions { actions } => actions.iter().map(|a| a.id.clone()).collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn trees(reply: &ViewReply) -> Vec<&Vec<TreeItem>> {
    reply
        .view
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Tree { nodes } => Some(nodes),
            _ => None,
        })
        .collect()
}

fn form_value(reply: &ViewReply) -> String {
    reply
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Form { id, fields, submit } => {
                assert_eq!(id, "load");
                assert_eq!(submit, "Load $metadata");
                assert_eq!(fields[0].kind, FieldKind::Text);
                Some(fields[0].value.clone())
            }
            _ => None,
        })
        .expect("a load form")
}

#[test]
fn empty_cache_shows_the_load_form() {
    let host = manifest_host();
    let _guard = host.install();
    let reply = render(None, None);
    assert!(matches!(&reply.view.blocks[0], Block::Heading { text } if text == "OData services"));
    assert!(trees(&reply).is_empty());
    assert_eq!(form_value(&reply), "");
    // The form is prefilled with the environment's baseUrl.
    let reply = render(None, Some(context(Some("DEV"), &[("baseUrl", V2_ROOT)])));
    assert_eq!(form_value(&reply), V2_ROOT);
}

#[test]
fn loading_from_the_tab_explains_the_alternative() {
    // A collection tab has no request, so `{request}` covers no host: the host refuses.
    let host = manifest_host();
    let _guard = host.install();
    let reply = act(
        "load",
        &[(
            "root",
            "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/$metadata?x=1",
        )],
        None,
    )
    .unwrap();
    let n = notices(&reply);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].0, NoteLevel::Warning);
    assert!(
        n[0].1.contains(&format!("Send GET {V2_ROOT}$metadata")),
        "{}",
        n[0].1
    );
    assert_eq!(
        action_ids(&reply),
        vec![format!("add-metadata-request {V2_ROOT}")]
    );
    assert_eq!(form_value(&reply), V2_ROOT);
    assert_eq!(reply.state, Some(serde_json::json!({ "root": V2_ROOT })));
    assert!(host.storage().is_empty());

    // The offered request: GET <root>$metadata in the collection root.
    let reply = act(&format!("add-metadata-request {V2_ROOT}"), &[], None).unwrap();
    assert_eq!(reply.create_requests.len(), 1);
    let r = &reply.create_requests[0];
    assert_eq!((r.folder.as_str(), r.name.as_str()), ("", "$metadata"));
    assert_eq!(r.spec.method, "GET");
    assert_eq!(r.spec.url, format!("{V2_ROOT}$metadata"));

    let bad = act("load", &[("root", "ftp://x/")], None).unwrap();
    assert_eq!(notices(&bad)[0].0, NoteLevel::Error);
}

#[test]
fn loading_works_where_the_host_allows_it() {
    let host = MockHost::new()
        .granted(Permissions {
            network: vec!["*".into()],
            storage: true,
            variables: false,
        })
        .on_http(|request, credentials| {
            assert_eq!(request.url, format!("{V4_ROOT}$metadata"));
            assert!(!credentials);
            Ok::<PluginResponse, String>(testing::response(200, "application/xml", S4_V4))
        });
    let _guard = host.install();
    let reply = act("load", &[("root", V4_ROOT.trim_end_matches('/'))], None).unwrap();
    assert_eq!(
        notices(&reply),
        vec![(
            NoteLevel::Info,
            "OData V4 metadata cached: 2 entity sets".to_owned()
        )]
    );
    assert_eq!(trees(&reply).len(), 1);
}

#[test]
fn the_tree_shows_sets_properties_navigation_and_operations() {
    let host = manifest_host();
    let _guard = host.install();
    store(&parse(S4_V2, V2_ROOT).unwrap()).unwrap();
    store(&parse(S4_V4, V4_ROOT).unwrap()).unwrap();
    let reply = render(None, None);
    let trees = trees(&reply);
    assert_eq!(trees.len(), 2);

    let v2 = trees[0];
    let labels: Vec<_> = v2.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "SalesOrderSet",
            "SalesOrderItemSet",
            "CustomerSet",
            "Operations"
        ]
    );
    assert_eq!(v2[0].detail.as_deref(), Some("SalesOrder · no delete"));
    assert_eq!(
        v2[2].detail.as_deref(),
        Some("Customer · no create/update/delete")
    );
    let id = &v2[0].children[0];
    assert_eq!(id.label, "SalesOrderID");
    assert_eq!(
        id.detail.as_deref(),
        Some("key · Edm.String(10) · \"Sales Order\"")
    );
    let amount = v2[0]
        .children
        .iter()
        .find(|c| c.label == "GrossAmount")
        .unwrap();
    assert_eq!(
        amount.detail.as_deref(),
        Some("Edm.Decimal(15,2) · \"Gross Amount\"")
    );
    let to_items = v2[0]
        .children
        .iter()
        .find(|c| c.label == "ToItems")
        .unwrap();
    assert_eq!(to_items.detail.as_deref(), Some("→ SalesOrderItem (many)"));
    let ops = &v2[3];
    assert_eq!(ops.children[0].label, "ConfirmSalesOrder");
    assert_eq!(
        ops.children[0].detail.as_deref(),
        Some("action · returns ConfirmationResult")
    );

    let v4 = trees[1];
    assert_eq!(
        v4[0].detail.as_deref(),
        Some("SalesOrderType · draft · no delete")
    );
    assert_eq!(
        v4[2].children[0].detail.as_deref(),
        Some("action · bound to SalesOrderType · returns SalesOrderType")
    );

    assert_eq!(
        action_ids(&reply),
        vec![
            format!("create-requests {V2_ROOT}"),
            format!("forget {V2_ROOT}"),
            format!("create-requests {V4_ROOT}"),
            format!("forget {V4_ROOT}"),
        ]
    );
}

#[test]
fn create_requests_and_forget() {
    let host = manifest_host();
    let _guard = host.install();
    let model = parse(CAP_V4, "http://localhost:4004/odata/v4/admin/").unwrap();
    store(&model).unwrap();
    let root = &model.service_root;

    let reply = act(
        &format!("create-requests {root}"),
        &[],
        Some(context(Some("Local"), &[])),
    )
    .unwrap();
    assert_eq!(
        reply.create_requests,
        rt_plugin_odata::metadata::requests_for(&model)
    );
    assert_eq!(reply.add_environments.len(), 1);
    let env = &reply.add_environments[0];
    assert_eq!(env.name, "Local");
    assert_eq!(env.variables.len(), 1);
    assert_eq!(env.variables[0].name, "baseUrl");
    assert_eq!(&env.variables[0].value, root);
    assert!(!env.variables[0].secret);
    assert_eq!(notices(&reply)[0].0, NoteLevel::Info);

    // Without a selected environment: `Default`.
    let reply = act(&format!("create-requests {root}"), &[], None).unwrap();
    assert_eq!(reply.add_environments[0].name, "Default");

    let reply = act(&format!("forget {root}"), &[], None).unwrap();
    assert!(trees(&reply).is_empty());
    assert_eq!(host.storage()["metadata-roots"], serde_json::json!([]));

    let reply = act(&format!("create-requests {root}"), &[], None).unwrap();
    assert!(reply.create_requests.is_empty());
    assert_eq!(notices(&reply)[0].0, NoteLevel::Warning);

    assert!(
        act("dance", &[], None)
            .unwrap_err()
            .contains("unknown action")
    );
}
