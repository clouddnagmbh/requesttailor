// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The request tab `query`, called through the SDK's JSON dispatch (what `rt_call` runs), with
//! the cached model seeded into the mock host's storage in the fixed format of strand A
//! (`metadata:<root>` holds the model, `metadata-roots` the known roots). The models are built
//! by hand after the fixture EDMX files in `testdata/`.

use rt_plugin_odata::OData;
use rt_plugin_odata::edm::{EntitySet, EntityType, Model, Navigation, Property, Version};
use rt_plugin_odata::service::storage_key;
use rt_plugin_sdk::api::{
    Block, Call, FieldKind, FormField, Permissions, PluginContext, TabTarget, ViewReply,
};
use rt_plugin_sdk::model::{Param, ParamKind, RequestSpec};
use rt_plugin_sdk::testing::{self, HostGuard, MockHost};
use rt_plugin_sdk::{NoteLevel, host};

const V2_ROOT: &str = "http://localhost:4010/sap/opu/odata/sap/ZRT_SALES_SRV/";
const V4_ROOT: &str = "http://localhost:4010/odata/v4/admin/";

fn prop(name: &str, ty: &str) -> Property {
    Property {
        name: name.into(),
        r#type: ty.into(),
        nullable: true,
        max_length: None,
        precision: None,
        scale: None,
        annotations: Vec::new(),
    }
}

fn set(name: &str, ty: &str) -> EntitySet {
    EntitySet {
        name: name.into(),
        entity_type: ty.into(),
        insertable: true,
        updatable: true,
        deletable: true,
        draft: false,
    }
}

fn nav(name: &str, target: &str, many: bool) -> Navigation {
    Navigation {
        name: name.into(),
        target: target.into(),
        many,
    }
}

/// After `testdata/s4/ZRT_SALES_SRV.edmx`.
fn sales_v2() -> Model {
    let mut note = prop("Note", "Edm.String");
    note.annotations.push(("filterable".into(), "false".into()));
    Model {
        version: Version::V2,
        service_root: V2_ROOT.into(),
        namespace: "ZRT_SALES_SRV".into(),
        entity_types: vec![
            EntityType {
                name: "ZRT_SALES_SRV.SalesOrder".into(),
                key: vec!["SalesOrderID".into()],
                properties: vec![
                    prop("SalesOrderID", "Edm.String"),
                    prop("CustomerName", "Edm.String"),
                    prop("GrossAmount", "Edm.Decimal"),
                    note,
                    prop("CreatedAt", "Edm.DateTime"),
                    prop("ChangedAt", "Edm.DateTimeOffset"),
                ],
                navigation: vec![
                    nav("ToItems", "ZRT_SALES_SRV.SalesOrderItem", true),
                    nav("ToCustomer", "ZRT_SALES_SRV.Customer", false),
                ],
                media: false,
            },
            EntityType {
                name: "ZRT_SALES_SRV.SalesOrderItem".into(),
                key: vec!["SalesOrderID".into(), "ItemPosition".into()],
                properties: vec![
                    prop("SalesOrderID", "Edm.String"),
                    prop("ItemPosition", "Edm.String"),
                    prop("Quantity", "Edm.Decimal"),
                ],
                navigation: vec![nav("ToSalesOrder", "ZRT_SALES_SRV.SalesOrder", false)],
                media: false,
            },
        ],
        complex_types: Vec::new(),
        entity_sets: vec![
            set("SalesOrderSet", "ZRT_SALES_SRV.SalesOrder"),
            set("SalesOrderItemSet", "ZRT_SALES_SRV.SalesOrderItem"),
        ],
        operations: Vec::new(),
    }
}

/// After `testdata/cap-bookshop/admin-v4.edmx`.
fn books_v4() -> Model {
    Model {
        version: Version::V4,
        service_root: V4_ROOT.into(),
        namespace: "AdminService".into(),
        entity_types: vec![
            EntityType {
                name: "AdminService.Books".into(),
                key: vec!["ID".into()],
                properties: vec![
                    prop("ID", "Edm.Int32"),
                    prop("title", "Edm.String"),
                    prop("price", "Edm.Decimal"),
                    prop("createdAt", "Edm.DateTimeOffset"),
                ],
                navigation: vec![nav("author", "AdminService.Authors", false)],
                media: false,
            },
            EntityType {
                name: "AdminService.Authors".into(),
                key: vec!["ID".into()],
                properties: vec![
                    prop("ID", "Edm.Int32"),
                    prop("name", "Edm.String"),
                    prop("dateOfBirth", "Edm.Date"),
                ],
                navigation: vec![nav("books", "AdminService.Books", true)],
                media: false,
            },
        ],
        complex_types: Vec::new(),
        entity_sets: vec![
            set("Books", "AdminService.Books"),
            set("Authors", "AdminService.Authors"),
        ],
        operations: Vec::new(),
    }
}

fn host_with(models: &[Model]) -> (MockHost, HostGuard) {
    let mock = MockHost::new().granted(Permissions {
        network: vec!["{request}".into()],
        storage: true,
        variables: true,
    });
    let guard = mock.install();
    let roots: Vec<String> = models.iter().map(|m| m.service_root.clone()).collect();
    for m in models {
        host::storage_set(
            storage_key("metadata", &m.service_root),
            serde_json::to_value(m).unwrap(),
        )
        .unwrap();
    }
    host::storage_set("metadata-roots", serde_json::to_value(roots).unwrap()).unwrap();
    (mock, guard)
}

fn spec(url: &str) -> RequestSpec {
    RequestSpec {
        name: "q".into(),
        method: "GET".into(),
        url: url.into(),
        ..RequestSpec::default()
    }
}

fn context(variables: &[(&str, &str)]) -> Option<PluginContext> {
    Some(PluginContext {
        request_path: "q.yml".into(),
        environment: Some("DEV".into()),
        variables: variables
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
    })
}

fn target() -> TabTarget {
    TabTarget {
        request_path: "q.yml".into(),
        response_id: None,
    }
}

fn render(
    spec: RequestSpec,
    state: Option<serde_json::Value>,
    ctx: Option<PluginContext>,
) -> ViewReply {
    testing::call(
        &OData,
        &Call::RenderTab {
            tab: "query".into(),
            target: target(),
            spec,
            response: None,
            state,
            context: ctx,
        },
    )
    .unwrap()
}

fn act(
    action: &str,
    spec: RequestSpec,
    state: Option<serde_json::Value>,
    input: Vec<(&str, &str)>,
    ctx: Option<PluginContext>,
) -> ViewReply {
    testing::call(
        &OData,
        &Call::Action {
            tab: "query".into(),
            action: action.into(),
            target: target(),
            spec,
            response: None,
            state,
            input: input
                .into_iter()
                .map(|(n, v)| (n.to_owned(), v.to_owned()))
                .collect(),
            context: ctx,
        },
    )
    .unwrap()
}

fn form<'a>(view: &'a ViewReply, id: &str) -> &'a [FormField] {
    view.view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Form { id: i, fields, .. } if i == id => Some(fields.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no form {id}: {:#?}", view.view.blocks))
}

fn field<'a>(view: &'a ViewReply, form_id: &str, name: &str) -> &'a FormField {
    form(view, form_id)
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

/// The form's values as the UI submits them, with some changed.
fn submit<'a>(
    view: &'a ViewReply,
    form_id: &str,
    changes: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    form(view, form_id)
        .iter()
        .map(|f| {
            let changed = changes.iter().find(|(n, _)| *n == f.name).map(|(_, v)| *v);
            (f.name.as_str(), changed.unwrap_or(f.value.as_str()))
        })
        .collect()
}

fn notices(view: &ViewReply) -> Vec<(NoteLevel, String)> {
    view.view
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Notice { level, text } => Some((*level, text.clone())),
            _ => None,
        })
        .collect()
}

fn code(view: &ViewReply) -> String {
    view.view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Code { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("a code block with the URL")
}

fn table(view: &ViewReply) -> Vec<Vec<String>> {
    view.view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table { rows, .. } => Some(rows.clone()),
            _ => None,
        })
        .expect("a help table")
}

#[test]
fn v2_form_from_the_cached_model() {
    let (_host, _guard) = host_with(&[sales_v2(), books_v4()]);
    let url = format!(
        "{V2_ROOT}SalesOrderSet?$filter=GrossAmount%20gt%20100M&$select=SalesOrderID,ToItems/Quantity&$orderby=CreatedAt%20desc,SalesOrderID&$top=5&$inlinecount=allpages&sap-client=100"
    );
    let view = render(spec(&url), None, None);
    assert!(notices(&view).is_empty(), "{:?}", notices(&view));
    let kv = view
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::KeyValue { rows } => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert!(kv.contains(&("Entity set".into(), "SalesOrderSet".into())));
    assert!(kv.contains(&("OData version".into(), "V2 (from $metadata)".into())));

    // Help: filterable properties (not Note) with V2 literal examples.
    let help = table(&view);
    let names: Vec<&str> = help.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(
        names,
        vec![
            "SalesOrderID",
            "CustomerName",
            "GrossAmount",
            "CreatedAt",
            "ChangedAt"
        ]
    );
    let example = |name: &str| help.iter().find(|r| r[0] == name).unwrap()[2].clone();
    assert!(example("CustomerName").contains("substringof('x',CustomerName)"));
    assert_eq!(example("GrossAmount"), "GrossAmount gt 12.5M");
    assert_eq!(
        example("CreatedAt"),
        "CreatedAt ge datetime'2026-01-01T00:00:00'"
    );
    assert_eq!(
        example("ChangedAt"),
        "ChangedAt ge datetimeoffset'2026-01-01T00:00:00Z'"
    );

    // Fields carry the URL's values.
    assert_eq!(field(&view, "apply", "filter").value, "GrossAmount gt 100M");
    let select_id = field(&view, "apply", "select.SalesOrderID");
    assert_eq!(
        (select_id.kind, select_id.value.as_str()),
        (FieldKind::Checkbox, "true")
    );
    assert_eq!(field(&view, "apply", "select.CustomerName").value, "false");
    assert_eq!(
        field(&view, "apply", "select_more").value,
        "ToItems/Quantity"
    );
    assert!(
        field(&view, "apply", "expand")
            .label
            .contains("ToItems (many), ToCustomer")
    );
    let orderby = field(&view, "apply", "orderby");
    assert_eq!(
        (orderby.kind, orderby.value.as_str()),
        (FieldKind::Select, "CreatedAt")
    );
    assert!(orderby.options.contains(&"GrossAmount".to_owned()));
    assert_eq!(field(&view, "apply", "orderby_desc").value, "true");
    assert_eq!(field(&view, "apply", "orderby_more").value, "SalesOrderID");
    assert_eq!(field(&view, "apply", "top").value, "5");
    assert_eq!(field(&view, "apply", "count").value, "true");
    assert!(
        field(&view, "apply", "count")
            .label
            .contains("$inlinecount")
    );
    assert_eq!(code(&view), url);

    // Change some fields and apply: the URL is rewritten, sap-client stays where it was.
    let input = submit(
        &view,
        "apply",
        &[
            ("select.CustomerName", "true"),
            ("select_more", ""),
            ("expand", "ToItems"),
            ("top", "10"),
            ("skip", "20"),
            ("orderby", "GrossAmount"),
            ("orderby_desc", "false"),
            ("orderby_more", ""),
        ],
    );
    let reply = act("apply", spec(&url), view.state.clone(), input, None);
    let updated = reply.update_request.clone().expect("request updated");
    assert_eq!(
        updated.url,
        format!(
            "{V2_ROOT}SalesOrderSet?$filter=GrossAmount%20gt%20100M&$select=SalesOrderID,CustomerName&$orderby=GrossAmount&$top=10&$inlinecount=allpages&sap-client=100&$expand=ToItems&$skip=20"
        )
    );
    // Query rows follow the URL.
    let rows: Vec<(&str, &str)> = updated
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(rows[0], ("$filter", "GrossAmount%20gt%20100M"));
    assert_eq!(rows.len(), 8);
    assert_eq!(code(&reply), updated.url);
    assert_eq!(notices(&reply)[0].0, NoteLevel::Info);
}

#[test]
fn v4_with_variables_navigation_and_templates() {
    let (_host, _guard) = host_with(&[books_v4()]);
    let ctx = context(&[("baseUrl", "http://localhost:4010/odata/v4/admin")]);
    let url = "{{baseUrl}}/Books(1)/author?$top={{top}}&$expand=books($select=title;$top=2)";
    let view = render(spec(url), None, ctx.clone());
    let kv = view
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::KeyValue { rows } => Some(rows.clone()),
            _ => None,
        })
        .unwrap();
    assert!(kv.contains(&("Service root".into(), V4_ROOT.into())));
    assert!(kv.contains(&("Entity set".into(), "Books".into())));
    assert!(
        kv.contains(&("Entity type".into(), "AdminService.Authors".into())),
        "navigation is followed: {kv:?}"
    );
    assert_eq!(field(&view, "apply", "top").value, "{{top}}");
    assert_eq!(
        field(&view, "apply", "expand").value,
        "books($select=title;$top=2)"
    );
    assert!(
        field(&view, "apply", "select.dateOfBirth")
            .label
            .contains("dateOfBirth")
    );
    let help = table(&view);
    assert!(help.iter().any(|r| r[2] == "dateOfBirth ge 2026-01-01"));
    assert!(help.iter().any(|r| r[2].contains("contains(name,'x')")));

    // Apply with the template untouched and $count on: template and base stay as written.
    let input = submit(
        &view,
        "apply",
        &[("count", "true"), ("select.name", "true")],
    );
    let reply = act("apply", spec(url), view.state.clone(), input, ctx);
    assert_eq!(
        reply.update_request.unwrap().url,
        "{{baseUrl}}/Books(1)/author?$top={{top}}&$expand=books($select=title;$top=2)&$select=name&$count=true"
    );
}

#[test]
fn invalid_input_keeps_the_draft_in_the_state() {
    let (_host, _guard) = host_with(&[books_v4()]);
    let url = format!("{V4_ROOT}Books");
    let view = render(spec(&url), None, None);
    let input = submit(&view, "apply", &[("top", "ten"), ("filter", "price gt 10")]);
    let reply = act("apply", spec(&url), view.state.clone(), input, None);
    assert!(reply.update_request.is_none());
    let n = notices(&reply);
    assert_eq!(n[0].0, NoteLevel::Warning);
    assert!(n[0].1.contains("$top must be a whole number"), "{n:?}");
    assert_eq!(field(&reply, "apply", "filter").value, "price gt 10");
    assert_eq!(field(&reply, "apply", "top").value, "ten");

    // Rendering again with that state shows the draft while the URL is the same...
    let again = render(spec(&url), reply.state.clone(), None);
    assert_eq!(field(&again, "apply", "top").value, "ten");
    // ...and the URL's values once it changed.
    let changed = render(spec(&format!("{url}?$top=3")), reply.state.clone(), None);
    assert_eq!(field(&changed, "apply", "top").value, "3");
    assert_eq!(field(&changed, "apply", "filter").value, "");
}

#[test]
fn filter_builder_v2_and_v4() {
    let (_host, _guard) = host_with(&[sales_v2(), books_v4()]);
    let url = format!("{V2_ROOT}SalesOrderSet('1')/ToItems?$top=5");
    let view = render(spec(&url), None, None);
    let props = &field(&view, "build", "prop.0").options;
    assert_eq!(props, &["", "SalesOrderID", "ItemPosition", "Quantity"]);
    assert_eq!(field(&view, "build", "op.0").value, "eq");
    let input = submit(
        &view,
        "build",
        &[
            ("prop.0", "Quantity"),
            ("op.0", "ge"),
            ("value.0", "2.5"),
            ("prop.1", "ProductID"),
            ("op.1", "contains"),
            ("value.1", "O'Brien"),
        ],
    );
    let reply = act("build", spec(&url), view.state.clone(), input, None);
    let updated = reply.update_request.clone().expect("request updated");
    assert_eq!(
        updated.url,
        format!(
            "{V2_ROOT}SalesOrderSet('1')/ToItems?$top=5&$filter=Quantity%20ge%202.5M%20and%20substringof('O''Brien',ProductID)"
        )
    );
    // The rows stay in the state.
    assert_eq!(field(&reply, "build", "value.1").value, "O'Brien");

    // V4, `or`, date and null literals.
    let url = format!("{V4_ROOT}Authors?$filter=old");
    let view = render(spec(&url), None, None);
    let input = submit(
        &view,
        "build",
        &[
            ("prop.0", "dateOfBirth"),
            ("op.0", "lt"),
            ("value.0", "1900-01-01"),
            ("prop.1", "name"),
            ("op.1", "eq"),
            ("value.1", "null"),
            ("join", "or"),
        ],
    );
    let reply = act("build", spec(&url), view.state.clone(), input, None);
    assert_eq!(
        reply.update_request.unwrap().url,
        format!("{V4_ROOT}Authors?$filter=dateOfBirth%20lt%201900-01-01%20or%20name%20eq%20null")
    );

    // A wrong literal is reported, nothing changes.
    let input = submit(&view, "build", &[("prop.0", "ID"), ("value.0", "abc")]);
    let reply = act("build", spec(&url), view.state.clone(), input, None);
    assert!(reply.update_request.is_none());
    assert!(
        notices(&reply)[0]
            .1
            .starts_with("ID: 'abc' is not a whole number")
    );
}

#[test]
fn builder_rows_can_be_added_and_removed() {
    let (_host, _guard) = host_with(&[books_v4()]);
    let url = format!("{V4_ROOT}Books");
    let view = render(spec(&url), None, None);
    let rows = |v: &ViewReply| {
        form(v, "build")
            .iter()
            .filter(|f| f.name.starts_with("prop."))
            .count()
    };
    assert_eq!(rows(&view), 3);
    let more = act("add-row", spec(&url), view.state.clone(), vec![], None);
    assert_eq!(rows(&more), 4);
    let fewer = act("remove-row", spec(&url), more.state.clone(), vec![], None);
    let fewer = act("remove-row", spec(&url), fewer.state.clone(), vec![], None);
    assert_eq!(rows(&fewer), 2);
    assert!(more.update_request.is_none());
}

#[test]
fn without_metadata_a_plain_form_still_works() {
    let (_host, _guard) = host_with(&[]);
    let url = "http://localhost:4010/sap/opu/odata/sap/ZRT_OTHER_SRV/Things?x=1";
    let view = render(spec(url), None, None);
    let n = notices(&view);
    assert!(
        n[0].1
            .contains("Send a request to http://localhost:4010/sap/opu/odata/sap/ZRT_OTHER_SRV/$metadata first"),
        "{n:?}"
    );
    assert!(
        !view
            .view
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Form { id, .. } if id == "build"))
    );
    let names: Vec<&str> = form(&view, "apply")
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "filter", "select", "expand", "orderby", "top", "skip", "count", "version"
        ]
    );
    assert_eq!(
        field(&view, "apply", "version").value,
        "V2",
        "guessed from /sap/opu/odata/"
    );
    let input = submit(
        &view,
        "apply",
        &[
            ("filter", "Name eq 'a b'"),
            ("select", "Name, ID"),
            ("orderby", "Name desc"),
            ("count", "true"),
        ],
    );
    let reply = act("apply", spec(url), view.state.clone(), input, None);
    assert_eq!(
        reply.update_request.unwrap().url,
        format!(
            "{url}&$filter=Name%20eq%20'a%20b'&$select=Name,ID&$orderby=Name%20desc&$inlinecount=allpages"
        )
    );

    // Version chosen in the form wins and is remembered.
    let input = submit(&view, "apply", &[("count", "true"), ("version", "V4")]);
    let reply = act("apply", spec(url), view.state.clone(), input, None);
    let updated = reply.update_request.unwrap();
    assert_eq!(updated.url, format!("{url}&$count=true"));
    let again = render(updated, reply.state.clone(), None);
    assert_eq!(field(&again, "apply", "version").value, "V4");
}

#[test]
fn unknown_root_and_non_entity_sets() {
    let (_host, _guard) = host_with(&[books_v4()]);
    // A template without a value: no root, a notice, still a form.
    let view = render(spec("{{baseUrl}}/Books?$top=1"), None, context(&[]));
    assert_eq!(notices(&view)[0].0, NoteLevel::Warning);
    assert_eq!(field(&view, "apply", "top").value, "1");
    // The service root itself, and something that is not an entity set.
    let view = render(spec(V4_ROOT), None, None);
    assert!(notices(&view)[0].1.contains("Books, Authors"));
    let view = render(spec(&format!("{V4_ROOT}Nope(1)")), None, None);
    assert!(notices(&view)[0].1.contains("Nope is not an entity set"));
    // Path rows and disabled rows survive an apply.
    let mut s = spec(&format!("{V4_ROOT}Books(:id)?$top=1"));
    s.params = vec![
        Param {
            name: "$top".into(),
            value: "1".into(),
            kind: ParamKind::Query,
            enabled: true,
        },
        Param {
            name: "off".into(),
            value: "".into(),
            kind: ParamKind::Query,
            enabled: false,
        },
        Param {
            name: "id".into(),
            value: "7".into(),
            kind: ParamKind::Path,
            enabled: true,
        },
    ];
    let view = render(s.clone(), None, None);
    let input = submit(&view, "apply", &[("top", "2")]);
    let reply = act("apply", s, view.state.clone(), input, None);
    let updated = reply.update_request.unwrap();
    assert_eq!(updated.url, format!("{V4_ROOT}Books(:id)?$top=2"));
    let rows: Vec<(&str, &str, bool)> = updated
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str(), p.enabled))
        .collect();
    assert_eq!(
        rows,
        vec![("$top", "2", true), ("off", "", false), ("id", "7", true)]
    );
}
