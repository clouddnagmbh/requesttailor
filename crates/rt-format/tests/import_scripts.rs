// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 7: imported Postman and Insomnia scripts land in the model at the right level and
//! phase, and the loss report names exactly the APIs the shims do not support.

use rt_core::model::{Item, Script, ScriptPhase};
use rt_format::import::{ImportFormat, LossEffect, LossKind, import};
use rt_format::read_collection;

fn script(phase: ScriptPhase, code: &str) -> Script {
    Script {
        phase,
        code: code.to_owned(),
    }
}

fn first_request(items: &[Item]) -> &rt_core::model::RequestSpec {
    match &items[0] {
        Item::Request(r) => &r.spec,
        Item::Folder(f) => first_request(&f.items),
    }
}

#[test]
fn postman_scripts_at_every_level() {
    let collection = r#"{
      "info": { "name": "S", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "event": [{ "listen": "prerequest", "script": { "exec": ["pm.variables.set('a', 1);", "// two"] } }],
      "item": [{
        "name": "F",
        "event": [
          { "listen": "test", "script": { "exec": ["pm.test('f', () => {});"] } },
          { "listen": "prerequest", "script": { "exec": [""] } }
        ],
        "item": [{
          "name": "R",
          "event": [
            { "listen": "prerequest", "script": { "exec": "pm.vault.get('k');" } },
            { "listen": "test", "script": { "exec": ["const x = require('xml2js');", "pm.expect(1).to.equal(1);"] } },
            { "listen": "test", "disabled": true, "script": { "exec": ["pm.test('off', () => {});"] } },
            { "listen": "beforeQuery", "script": { "exec": ["noop();"] } }
          ],
          "request": { "method": "GET", "url": "https://example.com" }
        }]
      }]
    }"#;
    let out = import(Some(ImportFormat::Postman), "s.json", collection.as_bytes()).unwrap();
    let doc = read_collection(&out.files).unwrap();
    let model = doc.model();
    assert_eq!(
        model.defaults.scripts,
        [script(
            ScriptPhase::BeforeRequest,
            "pm.variables.set('a', 1);\n// two"
        )]
    );
    let Item::Folder(folder) = &model.items[0] else {
        panic!("folder expected")
    };
    // An empty script is no script.
    assert_eq!(
        folder.defaults.scripts,
        [script(ScriptPhase::Tests, "pm.test('f', () => {});")]
    );
    assert_eq!(
        first_request(&model.items).scripts,
        [
            script(ScriptPhase::BeforeRequest, "pm.vault.get('k');"),
            script(
                ScriptPhase::Tests,
                "const x = require('xml2js');\npm.expect(1).to.equal(1);"
            ),
        ]
    );

    let losses: Vec<(&str, LossKind, LossEffect, &str)> = out
        .report
        .losses
        .iter()
        .map(|l| (l.location.as_str(), l.kind, l.effect, l.detail.as_str()))
        .collect();
    assert_eq!(losses.len(), 4, "{losses:#?}");
    let has = |kind, effect, text: &str| {
        losses
            .iter()
            .any(|l| l.0 == "F / R" && l.1 == kind && l.2 == effect && l.3.contains(text))
    };
    assert!(has(
        LossKind::Script,
        LossEffect::Approximated,
        "uses `pm.vault`"
    ));
    assert!(has(
        LossKind::Test,
        LossEffect::Approximated,
        "uses `require('xml2js')`"
    ));
    assert!(has(
        LossKind::Test,
        LossEffect::Dropped,
        "disabled test script (1 line) not imported"
    ));
    assert!(has(
        LossKind::Script,
        LossEffect::Dropped,
        "`beforeQuery` event"
    ));
}

#[test]
fn insomnia_scripts_of_folders_and_requests() {
    let export = r#"{
      "_type": "export", "__export_format": 4,
      "resources": [
        { "_id": "wrk_1", "_type": "workspace", "name": "W" },
        { "_id": "fld_1", "_type": "request_group", "parentId": "wrk_1", "name": "G",
          "preRequestScript": "insomnia.environment.set('a', '1');" },
        { "_id": "req_1", "_type": "request", "parentId": "fld_1", "name": "R",
          "method": "GET", "url": "https://example.com",
          "afterResponseScript": "insomnia.test('ok', () => insomnia.expect(insomnia.response.code).to.equal(200));\nconst c = insomnia.parentFolders.get('G');" }
      ]
    }"#;
    let out = import(Some(ImportFormat::Insomnia), "i.json", export.as_bytes()).unwrap();
    let doc = read_collection(&out.files).unwrap();
    let Item::Folder(folder) = &doc.model().items[0] else {
        panic!("folder expected")
    };
    assert_eq!(
        folder.defaults.scripts,
        [script(
            ScriptPhase::BeforeRequest,
            "insomnia.environment.set('a', '1');"
        )]
    );
    assert_eq!(
        first_request(&folder.items).scripts[0].phase,
        ScriptPhase::AfterResponse
    );
    let script_losses: Vec<&str> = out
        .report
        .losses
        .iter()
        .filter(|l| matches!(l.kind, LossKind::Script | LossKind::Test))
        .map(|l| l.detail.as_str())
        .collect();
    assert_eq!(script_losses.len(), 1, "{script_losses:?}");
    assert!(script_losses[0].contains("`insomnia.parentFolders`"));
}

#[test]
fn the_gate_collection_imports_without_loss() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/import/postman/scripts-gate.postman_collection.json"
    );
    let data = std::fs::read(path).unwrap();
    let out = import(None, "scripts-gate.postman_collection.json", &data).unwrap();
    assert_eq!(out.report.format, ImportFormat::Postman);
    assert!(out.report.is_complete(), "{:#?}", out.report.losses);
    let doc = read_collection(&out.files).unwrap();
    let model = doc.model();
    assert_eq!(model.defaults.scripts.len(), 2);
    let Item::Folder(orders) = &model.items[0] else {
        panic!("folder expected")
    };
    assert_eq!(orders.defaults.scripts.len(), 1);
    let create = first_request(&orders.items);
    assert_eq!(
        create.scripts.iter().map(|s| s.phase).collect::<Vec<_>>(),
        [ScriptPhase::BeforeRequest, ScriptPhase::Tests]
    );
    assert!(create.scripts[0].code.contains("require(\"crypto-js\")"));
}
