// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The `edmx` importer: detection, and a collection `rt-format` reads without issues (the host
//! checks a plugin import the same way), with the requests of `requests_for`, an environment
//! with `baseUrl` and a loss report.

use rt_plugin_odata::OData;
use rt_plugin_odata::metadata::{FALLBACK_BASE_URL, parse, requests_for};
use rt_plugin_sdk::api::{Call, DetectReply, ImportReply, Permissions};
use rt_plugin_sdk::model::{Auth, Collection, Item, RequestSpec};
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{ImportFormat, LossEffect, text_file};

const S4_V2: &str = include_str!("../testdata/s4/ZRT_SALES_SRV.edmx");
const S4_V4: &str = include_str!("../testdata/s4/zrt_salesorder_0001.edmx");
const CAP_V4: &str = include_str!("../testdata/cap-bookshop/admin-v4.edmx");
const CAP_V2: &str = include_str!("../testdata/cap-bookshop/admin-v2.edmx");

fn detect(files: Vec<rt_plugin_sdk::api::PluginFile>) -> bool {
    let reply: DetectReply = testing::call(
        &OData,
        &Call::Detect {
            importer: "edmx".into(),
            files,
        },
    )
    .unwrap();
    reply.matches
}

fn import(files: Vec<rt_plugin_sdk::api::PluginFile>) -> ImportReply {
    testing::call(
        &OData,
        &Call::Import {
            importer: "edmx".into(),
            files,
        },
    )
    .unwrap()
}

/// Reads the reply as the host does and returns the collection.
fn read(reply: &ImportReply) -> Collection {
    let files = rt_format::FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    };
    assert_eq!(files.files.len(), reply.files.len(), "paths are unique");
    let doc = rt_format::read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    doc.model().clone()
}

/// Every request of the collection with its folder name.
fn requests(collection: &Collection) -> Vec<(String, RequestSpec)> {
    let mut out = Vec::new();
    for item in &collection.items {
        match item {
            Item::Request(r) => out.push((String::new(), r.spec.clone())),
            Item::Folder(f) => {
                for item in &f.items {
                    match item {
                        Item::Request(r) => out.push((f.name.clone(), r.spec.clone())),
                        other => panic!("nested folder {other:?}"),
                    }
                }
            }
        }
    }
    out
}

#[test]
fn detects_edmx_only() {
    assert!(detect(vec![text_file("ZRT_SALES_SRV.edmx", S4_V2)]));
    assert!(detect(vec![text_file("metadata.xml", CAP_V4)]));
    assert!(detect(vec![
        text_file("readme.txt", "x"),
        text_file("$metadata.xml", CAP_V2)
    ]));
    assert!(!detect(vec![text_file("metadata.json", CAP_V4)]));
    assert!(!detect(vec![text_file("pom.xml", "<project/>")]));
    assert!(!detect(vec![text_file("broken.edmx", "<edmx:Edmx")]));
    assert!(!detect(vec![]));
}

#[test]
fn every_fixture_imports_as_a_valid_collection() {
    for (name, xml) in [
        ("ZRT_SALES_SRV.edmx", S4_V2),
        ("zrt_salesorder_0001.edmx", S4_V4),
        ("admin-v4.edmx", CAP_V4),
        ("admin-v2.xml", CAP_V2),
    ] {
        let reply = import(vec![text_file(name, xml)]);
        let collection = read(&reply);
        let model = parse(xml, FALLBACK_BASE_URL).unwrap();
        let expected = requests_for(&model);
        assert_eq!(reply.collection_name, model.namespace, "{name}");
        assert_eq!(collection.name, model.namespace, "{name}");
        assert_eq!(reply.report.format, ImportFormat::Plugin);
        assert_eq!(reply.report.requests as usize, expected.len(), "{name}");
        assert_eq!(
            reply.report.folders as usize,
            model.entity_sets.len(),
            "{name}"
        );
        assert_eq!(reply.report.environments, 1);

        // The files say exactly what requests_for says.
        let got = requests(&collection);
        assert_eq!(got.len(), expected.len(), "{name}");
        for want in &expected {
            let (_, spec) = got
                .iter()
                .find(|(folder, spec)| *folder == want.folder && spec.name == want.name)
                .unwrap_or_else(|| panic!("{name}: {} / {} missing", want.folder, want.name));
            assert_eq!(spec.method, want.spec.method, "{}", want.name);
            assert_eq!(spec.url, want.spec.url, "{}", want.name);
            assert_eq!(spec.params, want.spec.params, "{}", want.name);
            assert_eq!(spec.headers, want.spec.headers, "{}", want.name);
            assert_eq!(spec.body, want.spec.body, "{}", want.name);
            assert_eq!(spec.auth, Auth::Inherit, "{}", want.name);
        }

        // One environment with baseUrl; the fixtures do not say where they live.
        assert_eq!(collection.environments.len(), 1);
        let env = &collection.environments[0];
        assert_eq!(env.name, "Default");
        let base = env.variables.iter().find(|v| v.name == "baseUrl").unwrap();
        assert_eq!(base.value.as_deref(), Some(FALLBACK_BASE_URL));
    }
}

#[test]
fn folders_and_root_requests_keep_their_order() {
    let reply = import(vec![text_file("s.edmx", S4_V2)]);
    let collection = read(&reply);
    let order: Vec<String> = collection
        .items
        .iter()
        .map(|i| match i {
            Item::Folder(f) => format!("folder {}", f.name),
            Item::Request(r) => format!("request {}", r.spec.name),
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "folder SalesOrderSet",
            "folder SalesOrderItemSet",
            "folder CustomerSet",
            "request ConfirmSalesOrder",
            "request GetOpenOrderCount",
        ]
    );
    let paths: Vec<&str> = reply.files.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"SalesOrderSet/folder.yml"));
    assert!(paths.contains(&"SalesOrderSet/list-salesorderset.yml"));
    assert!(paths.contains(&"confirmsalesorder.yml"));
    assert!(paths.contains(&"environments/Default.yml"));
}

#[test]
fn the_self_link_gives_base_url_and_fills_the_cache() {
    let root = "https://gw.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/";
    let xml = S4_V2.replace(
        "<EntityContainer ",
        &format!(
            "<atom:link rel=\"self\" href=\"{root}$metadata\" xmlns:atom=\"http://www.w3.org/2005/Atom\"/>\n<EntityContainer "
        ),
    );
    let host = MockHost::new().granted(Permissions {
        storage: true,
        ..Permissions::default()
    });
    let _guard = host.install();
    let reply = import(vec![text_file("ZRT_SALES_SRV.edmx", &xml)]);
    let collection = read(&reply);
    let base = &collection.environments[0].variables[0];
    assert_eq!(base.value.as_deref(), Some(root));
    let cached = rt_plugin_odata::metadata::cached(root).unwrap().unwrap();
    assert_eq!(cached.entity_sets.len(), 3);
}

#[test]
fn the_report_lists_what_requests_cannot_carry() {
    let xml = r#"<edmx:Edmx Version="4.0" xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx">
  <edmx:DataServices><Schema Namespace="X" xmlns="http://docs.oasis-open.org/odata/ns/edm">
    <EntityType Name="Doc" HasStream="true"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32" Nullable="false"/></EntityType>
    <EntityType Name="Lonely"><Key><PropertyRef Name="ID"/></Key><Property Name="ID" Type="Edm.Int32"/></EntityType>
    <Action Name="orphan"/>
    <Action Name="poke" IsBound="true"><Parameter Name="in" Type="X.Lonely"/></Action>
    <EntityContainer Name="C">
      <EntitySet Name="Docs" EntityType="X.Doc"/>
      <Singleton Name="Me" Type="X.Doc"/>
    </EntityContainer>
  </Schema></edmx:DataServices></edmx:Edmx>"#;
    let reply = import(vec![text_file("x.edmx", xml), text_file("y.edmx", xml)]);
    read(&reply);
    let losses: Vec<(&str, LossEffect)> = reply
        .report
        .losses
        .iter()
        .map(|l| (l.location.as_str(), l.effect))
        .collect();
    assert_eq!(
        losses,
        vec![
            ("Docs", LossEffect::Approximated),
            ("operation orphan", LossEffect::Dropped),
            ("operation poke", LossEffect::Dropped),
            ("singleton Me", LossEffect::Dropped),
            ("y.edmx", LossEffect::Dropped),
        ]
    );
    assert_eq!(reply.report.requests, 5);
}

#[test]
fn not_edmx_is_an_error() {
    let e = testing::call::<ImportReply>(
        &OData,
        &Call::Import {
            importer: "edmx".into(),
            files: vec![text_file("a.xml", "<a/>")],
        },
    )
    .unwrap_err();
    assert!(e.contains("no EDMX"), "{e}");
}
