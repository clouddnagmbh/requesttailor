// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strand D, importer `sap-hub`: detection and import of the synthetic Hub-shaped files in
//! `testdata/hub/`; the result is read back with `rt-format` as the host does.

use rt_format::{FileSet, read_collection};
use rt_plugin_odata::OData;
use rt_plugin_sdk::api::{Call, DetectReply, ImportReply, PluginFile};
use rt_plugin_sdk::model::{
    ApiKeyPlacement, Auth, Body, Collection, Item, KeyValue, ParamKind, RawKind, RequestSpec,
};
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{LossEffect, LossKind, text_file};

fn fixture(path: &str) -> PluginFile {
    let full = format!("{}/testdata/{path}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&full).unwrap();
    text_file(path.rsplit('/').next().unwrap(), &text)
}

fn detect(files: Vec<PluginFile>) -> bool {
    let _guard = MockHost::new().install();
    let reply: DetectReply = testing::call(
        &OData,
        &Call::Detect {
            importer: "sap-hub".into(),
            files,
        },
    )
    .unwrap();
    reply.matches
}

fn import(files: Vec<PluginFile>) -> Result<ImportReply, String> {
    let _guard = MockHost::new().install();
    testing::call(
        &OData,
        &Call::Import {
            importer: "sap-hub".into(),
            files,
        },
    )
}

/// The files as the host checks them (`rt_app::plugins::plugin_import`).
fn read(reply: &ImportReply) -> Collection {
    let mut set = FileSet::default();
    for (path, text) in &reply.files {
        assert!(
            !path.starts_with('/') && !path.split('/').any(|p| p.is_empty() || p == ".."),
            "{path}"
        );
        assert!(
            set.files
                .insert(path.clone(), text.as_bytes().to_vec())
                .is_none(),
            "{path} twice"
        );
    }
    let doc = read_collection(&set).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    // Writing back what was read is byte-identical (the files are in the format's layout).
    assert_eq!(doc.write(), set);
    doc.model().clone()
}

fn requests(items: &[Item], folder: &str, out: &mut Vec<(String, RequestSpec)>) {
    for item in items {
        match item {
            Item::Request(r) => out.push((folder.to_owned(), r.spec.clone())),
            Item::Folder(f) => requests(&f.items, &f.name, out),
        }
    }
}

fn all_requests(c: &Collection) -> Vec<(String, RequestSpec)> {
    let mut out = Vec::new();
    requests(&c.items, "", &mut out);
    out
}

fn find<'a>(list: &'a [(String, RequestSpec)], name: &str) -> &'a RequestSpec {
    &list
        .iter()
        .find(|(_, s)| s.name == name)
        .unwrap_or_else(|| panic!("no request {name}"))
        .1
}

fn env_value(c: &Collection, env: &str, var: &str) -> Option<String> {
    let e = c
        .environment(env)
        .unwrap_or_else(|| panic!("no environment {env}"));
    let v = e.variables.iter().find(|v| v.name == var)?;
    assert!(!v.secret || v.value.is_none(), "secret {var} with a value");
    Some(if v.secret {
        "<secret>".to_owned()
    } else {
        v.value.clone().unwrap_or_default()
    })
}

#[test]
fn detects_hub_files_only() {
    assert!(detect(vec![fixture("hub/API_RT_PRODUCT_SRV.json")]));
    assert!(detect(vec![fixture("hub/api_rt_salesorder_v4.json")]));
    assert!(detect(vec![fixture("hub/API_RT_PRODUCT_SRV.edmx")]));

    // A plain $metadata is the `edmx` importer's, a plain OpenAPI the built-in importer's.
    assert!(!detect(vec![fixture("s4/ZRT_SALES_SRV.edmx")]));
    let plain = r#"{"openapi": "3.0.0", "info": {"title": "x", "version": "1"},
        "servers": [{"url": "https://api.example.com"}], "paths": {}}"#;
    assert!(!detect(vec![text_file("plain.json", plain)]));
    // Swagger 2 with x-sap keys is not OpenAPI 3.
    let swagger = r#"{"swagger": "2.0", "x-sap-api-type": "ODATA", "paths": {}}"#;
    assert!(!detect(vec![text_file("old.json", swagger)]));
    // Only via the sandbox server, no x-sap keys: detected.
    let sandbox = r#"{"openapi": "3.0.1", "servers": [{"url": "https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_X"}], "paths": {}}"#;
    assert!(detect(vec![text_file("s.json", sandbox)]));
    // Not JSON, binary, two files, no files.
    assert!(!detect(vec![text_file("x.json", "not json")]));
    assert!(!detect(vec![PluginFile {
        name: "b.json".into(),
        base64: "//79".into()
    }]));
    assert!(!detect(vec![
        fixture("hub/API_RT_PRODUCT_SRV.json"),
        fixture("hub/api_rt_salesorder_v4.json")
    ]));
    assert!(!detect(Vec::new()));
}

#[test]
fn openapi_v2_becomes_requests_folders_and_environments() {
    let reply = import(vec![fixture("hub/API_RT_PRODUCT_SRV.json")]).unwrap();
    assert_eq!(
        reply.collection_name,
        "Product Master (RequestTailor test API)"
    );
    let c = read(&reply);

    // Collection: APIKey header for the sandbox, Basic auth for the own systems.
    assert_eq!(
        c.defaults.headers,
        vec![KeyValue::new("APIKey", "{{apiKey}}")]
    );
    assert_eq!(
        c.defaults.auth,
        Auth::Basic {
            username: "{{user}}".into(),
            password: "{{password}}".into()
        }
    );

    // Folders by tag, in the order of the top-level tags.
    let folders: Vec<&str> = c
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Folder(f) => Some(f.name.as_str()),
            Item::Request(_) => None,
        })
        .collect();
    assert_eq!(
        folders,
        ["Product", "Product Description", "Batch Requests"]
    );

    let list = all_requests(&c);
    assert_eq!(list.len(), 7);
    assert_eq!(reply.report.requests, 7);
    assert_eq!(reply.report.folders, 3);
    assert_eq!(reply.report.environments, 4);

    // GET collection: optional system query options are disabled params, not in the URL.
    let get = find(&list, "Get entities from A_RtProduct");
    assert_eq!(get.method, "GET");
    assert_eq!(get.url, "{{baseUrl}}/A_RtProduct");
    let top = get.params.iter().find(|p| p.name == "$top").unwrap();
    assert_eq!(
        (top.kind, top.enabled, top.value.as_str()),
        (ParamKind::Query, false, "50")
    );
    assert!(
        get.params
            .iter()
            .any(|p| p.name == "$orderby" && !p.enabled)
    );

    // Key access: `{Product}` → `:Product` with the example as path param.
    let by_key = find(&list, "Get entity from A_RtProduct by key");
    assert_eq!(by_key.url, "{{baseUrl}}/A_RtProduct(':Product')");
    let key = by_key
        .params
        .iter()
        .find(|p| p.kind == ParamKind::Path)
        .unwrap();
    assert_eq!(
        (key.name.as_str(), key.value.as_str()),
        ("Product", "RT-1000")
    );

    // Required query parameter goes into the URL.
    let desc = find(&list, "Get entities from related to_Description");
    assert_eq!(
        desc.url,
        "{{baseUrl}}/A_RtProduct(':Product')/to_Description?sap-language=EN"
    );

    // Bodies: generated skeleton for POST, explicit example for PATCH; Content-Type header.
    let post = find(&list, "Add new entity to A_RtProduct");
    assert_eq!(
        post.headers[0],
        KeyValue::new("Content-Type", "application/json")
    );
    let Body::Raw {
        kind: RawKind::Json,
        data,
    } = &post.body
    else {
        panic!("json body expected: {:?}", post.body);
    };
    let body: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(body["Product"], "RT-1000");
    assert_eq!(body["IsMarkedForDeletion"], false);
    assert_eq!(body["to_Description"]["results"][0]["Language"], "string");
    let patch = find(&list, "Update entity in A_RtProduct");
    let Body::Raw { data, .. } = &patch.body else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(data).unwrap(),
        serde_json::json!({"ProductType": "FERT", "BaseUnit": "PC"})
    );
    // Optional header parameter: disabled header.
    assert!(
        patch
            .headers
            .iter()
            .any(|h| h.name == "If-Match" && !h.enabled)
    );
    assert_eq!(
        find(&list, "Delete entity from A_RtProduct").method,
        "DELETE"
    );
    // $batch: multipart body without example is not imported (reported below).
    let batch = find(&list, "Send a group of requests");
    assert_eq!((batch.method.as_str(), &batch.body), ("POST", &Body::None));
    assert!(list.iter().all(|(_, s)| s.auth == Auth::Inherit));

    // Environments: DEV/QAS/PRD template and the sandbox.
    // (Environments come back sorted by file name.)
    let names: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["API Hub Sandbox", "DEV", "PRD", "QAS"]);
    assert_eq!(
        env_value(&c, "DEV", "baseUrl").as_deref(),
        Some("{{baseHost}}/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(env_value(&c, "DEV", "baseHost").as_deref(), Some(""));
    assert_eq!(env_value(&c, "QAS", "sapLanguage").as_deref(), Some("EN"));
    assert_eq!(
        env_value(&c, "PRD", "password").as_deref(),
        Some("<secret>")
    );
    assert_eq!(
        env_value(&c, "PRD", "rtProduction").as_deref(),
        Some("true")
    );
    assert_eq!(env_value(&c, "DEV", "rtProduction"), None);
    assert_eq!(
        env_value(&c, "API Hub Sandbox", "baseUrl").as_deref(),
        Some("https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(
        env_value(&c, "API Hub Sandbox", "apiKey").as_deref(),
        Some("<secret>")
    );
    assert_eq!(
        env_value(&c, "API Hub Sandbox", "sapSandbox").as_deref(),
        Some("true")
    );

    // Loss report: collection entries first, nothing silent.
    let losses = &reply.report.losses;
    let has = |kind: LossKind, effect: LossEffect, part: &str| {
        losses
            .iter()
            .any(|l| l.kind == kind && l.effect == effect && l.detail.contains(part))
    };
    assert!(has(
        LossKind::Body,
        LossEffect::Approximated,
        "generated from the schema"
    ));
    assert!(has(LossKind::Body, LossEffect::Dropped, "multipart/mixed"));
    assert!(has(
        LossKind::Documentation,
        LossEffect::Dropped,
        "x-sap-ext-overview"
    ));
    assert!(has(
        LossKind::Documentation,
        LossEffect::Dropped,
        "x-sap-software-min-version"
    ));
    assert!(has(
        LossKind::Documentation,
        LossEffect::Dropped,
        "responses, schemas"
    ));
    assert!(has(
        LossKind::Variable,
        LossEffect::Approximated,
        "set baseHost"
    ));
    assert!(!losses.iter().any(|l| l.detail.contains("x-sap-shortText")));
    let first_other = losses
        .iter()
        .position(|l| l.location != "collection")
        .unwrap();
    assert!(
        losses[first_other..]
            .iter()
            .all(|l| l.location != "collection")
    );
    let batch_loss = losses
        .iter()
        .find(|l| l.detail.contains("multipart/mixed"))
        .unwrap();
    assert_eq!(
        batch_loss.location,
        "Batch Requests / Send a group of requests"
    );
}

#[test]
fn openapi_v4_with_examples_and_other_security() {
    let reply = import(vec![fixture("hub/api_rt_salesorder_v4.json")]).unwrap();
    let c = read(&reply);
    let list = all_requests(&c);
    // No tags: everything at the root; name from operationId when there is no summary.
    assert!(list.iter().all(|(folder, _)| folder.is_empty()));
    let read_one = find(&list, "readSalesOrder");
    assert_eq!(read_one.url, "{{baseUrl}}/SalesOrder/:SalesOrder");
    assert_eq!(read_one.params[0].value, "1");
    let create = find(&list, "Creates a sales order.");
    let Body::Raw { data, .. } = &create.body else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(data).unwrap(),
        serde_json::json!({"SalesOrderType": "OR", "SoldToParty": "RT-CUST-1"})
    );
    assert_eq!(
        env_value(&c, "DEV", "baseUrl").as_deref(),
        Some("{{baseHost}}/sap/opu/odata4/sap/api_rt_salesorder/srvd_a2x/sap/rt_salesorder/0001")
    );
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.kind == LossKind::Auth && l.detail.contains("`OAuth2` (oauth2)"))
    );
    assert!(
        !reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("Body"))
    );
}

#[test]
fn openapi_without_sandbox_has_no_api_key() {
    let spec = r#"{"openapi": "3.0.0", "x-sap-api-type": "REST",
        "info": {"title": "Own REST API", "version": "1"},
        "servers": [{"url": "https://{host}:{port}/sap/bc/rest/zrt"}],
        "paths": {"/items": {"get": {"summary": "List items"}}}}"#;
    let reply = import(vec![text_file("own.json", spec)]).unwrap();
    let c = read(&reply);
    assert!(c.defaults.headers.is_empty());
    let names: Vec<&str> = c.environments.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["DEV", "PRD", "QAS"]);
    assert_eq!(
        env_value(&c, "DEV", "baseUrl").as_deref(),
        Some("{{baseHost}}/sap/bc/rest/zrt")
    );
    assert_eq!(
        find(&all_requests(&c), "List items").url,
        "{{baseUrl}}/items"
    );
}

#[test]
fn names_that_collide_get_unique_files() {
    let spec = r#"{"openapi": "3.0.0", "x-sap-api-type": "ODATA",
        "info": {"title": "T", "version": "1"},
        "paths": {"/a": {"get": {"summary": "Read"}, "post": {"summary": "read"}},
                  "/b": {"get": {"summary": "Read"}}}}"#;
    let reply = import(vec![text_file("t.json", spec)]).unwrap();
    let c = read(&reply);
    assert_eq!(all_requests(&c).len(), 3);
    let paths: Vec<&str> = reply.files.iter().map(|(p, _)| p.as_str()).collect();
    assert!(
        paths.contains(&"read.yml")
            && paths.contains(&"read-2.yml")
            && paths.contains(&"read-3.yml")
    );
    assert!(
        reply
            .report
            .losses
            .iter()
            .any(|l| l.detail.contains("no servers"))
    );
}

#[test]
fn import_refuses_other_files() {
    assert!(import(vec![fixture("s4/ZRT_SALES_SRV.edmx")]).is_err());
    assert!(import(Vec::new()).is_err());
}

#[test]
fn api_key_auth_in_the_request_is_written() {
    // The writer's other auth types read back as written (used by the EDMX path).
    use rt_plugin_odata::landscape::files::{CollectionFiles, RequestFile};
    let spec = RequestSpec {
        name: "k".into(),
        method: "GET".into(),
        url: "{{baseUrl}}/x?a=1".into(),
        auth: Auth::ApiKey {
            key: "APIKey".into(),
            value: "{{apiKey}}".into(),
            placement: ApiKeyPlacement::Header,
        },
        body: Body::FormUrlEncoded {
            fields: vec![KeyValue::new("a", "b: c")],
        },
        ..RequestSpec::default()
    };
    let files = CollectionFiles {
        name: "Ä \"quoted\" name".into(),
        docs: Some("line one\n\nline three".into()),
        requests: vec![RequestFile {
            folder: vec!["a/b".into(), "inner".into()],
            spec: spec.clone(),
            docs: Some("doc".into()),
        }],
        ..CollectionFiles::default()
    };
    let (out, counts) = files.write();
    assert_eq!((counts.requests, counts.folders), (1, 2));
    let reply = ImportReply {
        collection_name: "x".into(),
        files: out,
        report: rt_plugin_sdk::LossReport {
            format: rt_plugin_sdk::ImportFormat::Plugin,
            requests: 1,
            folders: 2,
            environments: 0,
            losses: Vec::new(),
        },
    };
    let c = read(&reply);
    assert_eq!(c.name, "Ä \"quoted\" name");
    let list = all_requests(&c);
    assert_eq!(list[0].0, "inner");
    assert_eq!(
        list[0].1,
        RequestSpec {
            seq: Some(1.0),
            ..spec
        }
    );
}

#[test]
fn edmx_from_the_sandbox_uses_the_metadata_requests() {
    let reply = import(vec![fixture("hub/API_RT_PRODUCT_SRV.edmx")]).unwrap();
    let c = read(&reply);
    assert_eq!(reply.collection_name, "API_RT_PRODUCT_SRV");
    let list = all_requests(&c);
    assert!(!list.is_empty());
    assert!(
        list.iter().all(|(_, s)| s.url.starts_with("{{baseUrl}}")),
        "{list:?}"
    );
    assert!(list.iter().any(|(_, s)| s.url.contains("A_RtProduct")));
    assert_eq!(
        c.defaults.headers,
        vec![KeyValue::new("APIKey", "{{apiKey}}")]
    );
    assert_eq!(
        env_value(&c, "API Hub Sandbox", "baseUrl").as_deref(),
        Some("https://sandbox.api.sap.com/s4hanacloud/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(
        env_value(&c, "PRD", "baseUrl").as_deref(),
        Some("{{baseHost}}/sap/opu/odata/sap/API_RT_PRODUCT_SRV")
    );
    assert_eq!(reply.report.requests as usize, list.len());
}
