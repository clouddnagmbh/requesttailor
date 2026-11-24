// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Collection lint (wave 18, strand F): each of the four rules with matches, non-matches and
//! edge cases (dynamic script access reports nothing, loopback hosts, secrets without use, the
//! same file name), the health score, settings and ignoring. Synthetic collections only.

use rt_app::Workspace;
use rt_app::lint::{self, LintFinding, LintLevel, LintReport, LintRule, LintSettings};
use rt_format::{COLLECTION_FILE, FileSet};

const CANARY: &str = "RT-CANARY-lint-secret-7";

fn workspace(files: &[(&str, &str)]) -> Workspace {
    let mut set = FileSet::default();
    set.files.insert(
        COLLECTION_FILE.to_owned(),
        b"opencollection: \"1.0.0\"\ninfo:\n  name: shop\n".to_vec(),
    );
    for (path, text) in files {
        set.files.insert((*path).to_owned(), text.as_bytes().to_vec());
    }
    Workspace::open(set).expect("fixture collection")
}

/// A GET request file; `extra` is appended (runtime, more http fields).
fn request(name: &str, url: &str, extra: &str) -> String {
    format!(
        "info:\n  name: {name}\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{url}\"\n  auth: inherit\n{extra}"
    )
}

const WITH_ASSERTION: &str = "\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n";

fn run(ws: &Workspace) -> LintReport {
    lint::run(ws)
}

fn of(report: &LintReport, rule: LintRule) -> Vec<&LintFinding> {
    report.findings.iter().filter(|f| f.rule == rule).collect()
}

fn paths(report: &LintReport, rule: LintRule) -> Vec<&str> {
    of(report, rule).iter().map(|f| f.path.as_str()).collect()
}

// ---- no-tests ----

#[test]
fn no_tests_finds_requests_without_test_assertion_or_snapshot() {
    let ws = workspace(&[
        ("bare.yml", &request("Bare", "{{baseUrl}}/a", "")),
        ("asserted.yml", &request("Asserted", "{{baseUrl}}/b", WITH_ASSERTION)),
        (
            "tested.yml",
            &request(
                "Tested",
                "{{baseUrl}}/c",
                "\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test(\"ok\", () => {});\n",
            ),
        ),
        (
            "snap.yml",
            &request(
                "Snap",
                "{{baseUrl}}/d",
                "\nruntime:\n  assertions:\n    - expression: res\n      operator: matchesSnapshot\n      value: snap.snap.json\n",
            ),
        ),
        (
            "disabled.yml",
            &request(
                "Disabled",
                "{{baseUrl}}/e",
                "\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n      enabled: false\n",
            ),
        ),
    ]);
    let report = run(&ws);
    let mut found = paths(&report, LintRule::NoTests);
    found.sort();
    assert_eq!(found, vec!["bare.yml", "disabled.yml"]);
    let f = of(&report, LintRule::NoTests)[0];
    assert_eq!(f.level, LintLevel::Warn);
}

#[test]
fn a_tests_script_of_a_folder_covers_the_requests_below() {
    let ws = workspace(&[
        (
            "users/folder.yml",
            "info:\n  name: users\n  type: folder\nrequest:\n  scripts:\n    - type: tests\n      code: |-\n        test(\"all\", () => {});\n",
        ),
        ("users/get.yml", &request("Get", "{{baseUrl}}/u", "")),
        ("other.yml", &request("Other", "{{baseUrl}}/o", "")),
    ]);
    let report = run(&ws);
    assert_eq!(paths(&report, LintRule::NoTests), vec!["other.yml"]);
}

// ---- hard-coded-host ----

#[test]
fn hard_coded_hosts_in_urls_are_found_and_variables_are_not() {
    let ws = workspace(&[
        ("fixed.yml", &request("Fixed", "https://api.example.com/x?key=public", WITH_ASSERTION)),
        ("var.yml", &request("Var", "{{baseUrl}}/x", WITH_ASSERTION)),
        ("host-var.yml", &request("HostVar", "https://{{host}}/x", WITH_ASSERTION)),
        ("scheme-var.yml", &request("SchemeVar", "{{scheme}}://api.example.com/x", WITH_ASSERTION)),
        ("part.yml", &request("Part", "https://api.{{stage}}.example.com/x", WITH_ASSERTION)),
        ("bare-host.yml", &request("BareHost", "example.com/x", WITH_ASSERTION)),
    ]);
    let report = run(&ws);
    let found = of(&report, LintRule::HardCodedHost);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, "fixed.yml");
    assert_eq!(found[0].level, LintLevel::Warn);
    assert_eq!(found[0].location.as_deref(), Some("url"));
    assert!(found[0].message.contains("api.example.com"));
}

#[test]
fn loopback_hosts_are_info_whatever_the_level() {
    let ws = workspace(&[
        ("a.yml", &request("A", "http://localhost:3000/x", WITH_ASSERTION)),
        ("b.yml", &request("B", "http://127.0.0.1:8080/x", WITH_ASSERTION)),
        ("c.yml", &request("C", "http://[::1]:8080/x", WITH_ASSERTION)),
        ("d.yml", &request("D", "http://api.localhost/x", WITH_ASSERTION)),
        ("e.yml", &request("E", "http://10.0.0.5/x", WITH_ASSERTION)),
    ]);
    let report = run(&ws);
    let found = of(&report, LintRule::HardCodedHost);
    let mut by_path: Vec<(&str, LintLevel)> = found.iter().map(|f| (f.path.as_str(), f.level)).collect();
    by_path.sort();
    assert_eq!(
        by_path,
        vec![
            ("a.yml", LintLevel::Info),
            ("b.yml", LintLevel::Info),
            ("c.yml", LintLevel::Info),
            ("d.yml", LintLevel::Info),
            ("e.yml", LintLevel::Warn),
        ]
    );
    // Even at `error`, loopback stays info.
    let mut settings = lint::settings(&ws);
    settings.rules.insert("lint/hard-coded-host".into(), LintLevel::Error);
    let strict = lint::lint_with(&ws, &settings);
    let level = |p: &str| of(&strict, LintRule::HardCodedHost).iter().find(|f| f.path == p).unwrap().level;
    assert_eq!(level("a.yml"), LintLevel::Info);
    assert_eq!(level("e.yml"), LintLevel::Error);
}

#[test]
fn token_urls_of_oauth_are_checked_on_requests_folders_and_the_collection() {
    let oauth = |url: &str| {
        format!(
            "  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: \"{url}\"\n    clientId: id\n    clientSecret: \"{{{{secret}}}}\"\n"
        )
    };
    let request_with_oauth = format!(
        "info:\n  name: Tok\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/t\"\n{}",
        oauth("https://login.example.com/token")
    );
    let ws = workspace(&[("tok.yml", &request_with_oauth)]);
    let report = run(&ws);
    let found = of(&report, LintRule::HardCodedHost);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, "tok.yml");
    assert_eq!(found[0].location.as_deref(), Some("auth.access_token_url"));
    // The same with a variable gives nothing.
    let ok = request_with_oauth.replace("https://login.example.com/token", "{{tokenUrl}}");
    assert!(of(&run(&workspace(&[("tok.yml", &ok)])), LintRule::HardCodedHost).is_empty());
}

#[test]
fn the_message_names_the_host_only_never_the_rest_of_the_url() {
    let url = format!("https://user:{CANARY}@api.example.com/x?token={CANARY}");
    let ws = workspace(&[("x.yml", &request("X", &url, WITH_ASSERTION))]);
    let report = run(&ws);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(CANARY), "{json}");
    assert_eq!(of(&report, LintRule::HardCodedHost).len(), 1);
}

// ---- orphan-variable ----

const COLLECTION_VARS: &str = "opencollection: \"1.0.0\"\ninfo:\n  name: shop\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.test\n    - name: unusedCollection\n      value: x\n    - name: off\n      value: x\n      enabled: false\n";

fn with_collection_file(text: &str, files: &[(&str, &str)]) -> Workspace {
    let mut set = FileSet::default();
    set.files.insert(COLLECTION_FILE.to_owned(), text.as_bytes().to_vec());
    for (path, body) in files {
        set.files.insert((*path).to_owned(), body.as_bytes().to_vec());
    }
    Workspace::open(set).expect("fixture collection")
}

fn names(report: &LintReport) -> Vec<String> {
    let mut v: Vec<String> = of(report, LintRule::OrphanVariable)
        .iter()
        .map(|f| f.message.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn unused_variables_of_collection_folder_and_environment_are_found() {
    let ws = with_collection_file(
        COLLECTION_VARS,
        &[
            ("get.yml", &request("Get", "{{baseUrl}}/x", WITH_ASSERTION)),
            (
                "users/folder.yml",
                "info:\n  name: users\n  type: folder\nrequest:\n  variables:\n    - name: unusedFolder\n      value: 1\n",
            ),
            ("users/list.yml", &request("List", "{{baseUrl}}/u", WITH_ASSERTION)),
            (
                "environments/Dev.yml",
                "name: Dev\nvariables:\n  - name: unusedEnv\n    value: 1\n  - name: baseUrl\n    value: https://dev.test\n  - secret: true\n    name: unusedSecret\n",
            ),
        ],
    );
    let report = run(&ws);
    let found = of(&report, LintRule::OrphanVariable);
    let by_name = |n: &str| found.iter().find(|f| f.message.contains(n));
    assert!(by_name("unusedCollection").is_some(), "{found:?}");
    assert!(by_name("unusedFolder").is_some());
    assert!(by_name("unusedEnv").is_some());
    assert!(by_name("unusedSecret").is_some());
    assert!(by_name("`off`").is_none(), "disabled variables are not defined");
    assert!(by_name("baseUrl").is_none());
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(by_name("unusedFolder").unwrap().path, "users/");
    assert_eq!(by_name("unusedEnv").unwrap().path, "environments/Dev.yml");
    assert_eq!(by_name("unusedCollection").unwrap().path, "");
    assert!(found.iter().all(|f| f.level == LintLevel::Info));
}

fn orphan_ws(script: &str) -> Workspace {
    let request = format!(
        "info:\n  name: S\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/s\"\n  auth: inherit\n\nruntime:\n  scripts:\n    - type: after-response\n      code: |-\n        {script}\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n"
    );
    with_collection_file(
        "opencollection: \"1.0.0\"\ninfo:\n  name: shop\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.test\n    - name: token\n      value: x\n",
        &[("s.yml", &request)],
    )
}

#[test]
fn literal_script_access_counts_as_use() {
    for script in [
        "const t = rt.vars.get(\"token\");",
        "rt.vars.set('token', 1);",
        "const t = bru.getVar(\"token\");",
        "const t = bru.getEnvVar('token');",
        "const t = pm.environment.get(\"token\");",
        "const t = pm.collectionVariables.get(\"token\");",
        "const t = rt.vars.replace(\"Bearer {{token}}\");",
    ] {
        let report = run(&orphan_ws(script));
        assert!(names(&report).is_empty(), "{script}: {:?}", names(&report));
    }
}

#[test]
fn a_script_without_access_leaves_the_variable_orphaned() {
    let report = run(&orphan_ws("console.log(\"hi\");"));
    assert_eq!(names(&report).len(), 1, "{:?}", names(&report));
    assert!(names(&report)[0].contains("token"));
}

#[test]
fn dynamic_script_access_reports_nothing() {
    for script in [
        "const t = rt.vars.get(name);",
        "const t = rt.vars.get(\"to\" + \"ken\");",
        "const t = rt.vars.get(`${prefix}ken`);",
        "const all = rt.vars.toObject();",
        "const v = rt.vars; v.get(\"x\");",
        "const { vars } = rt;",
        "const b = bru; b.getVar(k);",
        "const t = bru.getVar(key);",
        "const t = pm.variables.get(key);",
        "for (const k of Object.keys(rt.vars.toObject())) {}",
        "const t = rt[\"vars\"].get(\"token\");",
    ] {
        let report = run(&orphan_ws(script));
        assert!(names(&report).is_empty(), "{script}: {:?}", names(&report));
    }
}

#[test]
fn placeholders_in_any_field_assertions_actions_and_body_files_count() {
    let ws = with_collection_file(
        "opencollection: \"1.0.0\"\ninfo:\n  name: shop\nrequest:\n  variables:\n    - name: inHeader\n      value: 1\n    - name: inBody\n      value: 1\n    - name: inFile\n      value: 1\n    - name: inAssertion\n      value: 1\n    - name: setByAction\n      value: 1\n    - name: inEnv\n      value: 1\n    - name: inAuth\n      value: 1\n    - name: lonely\n      value: 1\n",
        &[
            (
                "r.yml",
                "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: \"http://localhost/r\"\n  headers:\n    - name: X\n      value: \"{{inHeader}}\"\n  body:\n    type: json\n    data: |-\n      {\"a\": \"{{inBody}}\"}\n  auth:\n    type: bearer\n    token: \"{{inAuth}}\"\n\nruntime:\n  assertions:\n    - expression: inAssertion\n      operator: eq\n      value: \"1\"\n  actions:\n    - type: set-variable\n      phase: post-response\n      selector:\n        expression: res.body.id\n        method: jsonq\n      variable:\n        name: setByAction\n        scope: runtime\n",
            ),
            ("payload.json", "{\"x\": \"{{inFile}}\"}"),
            ("environments/E.yml", "name: E\nvariables:\n  - name: other\n    value: \"{{inEnv}}\"\n"),
        ],
    );
    let report = run(&ws);
    let orphans = names(&report);
    assert!(orphans.iter().any(|m| m.contains("lonely")), "{orphans:?}");
    for used in ["inHeader", "inBody", "inFile", "inAssertion", "setByAction", "inEnv", "inAuth"] {
        assert!(!orphans.iter().any(|m| m.contains(&format!("`{used}`"))), "{used}: {orphans:?}");
    }
}

// ---- duplicate-name ----

#[test]
fn duplicate_names_in_one_folder_are_errors_and_other_folders_are_fine() {
    let ws = workspace(&[
        ("a/one.yml", &request("Same", "{{b}}/1", WITH_ASSERTION)),
        ("a/two.yml", &request("Same", "{{b}}/2", WITH_ASSERTION)),
        ("a/three.yml", &request("Different", "{{b}}/3", WITH_ASSERTION)),
        ("b/one.yml", &request("Same", "{{b}}/4", WITH_ASSERTION)),
        ("x/folder.yml", "info:\n  name: Dup\n  type: folder\n"),
        ("x/r.yml", &request("R", "{{b}}/5", WITH_ASSERTION)),
        ("y/folder.yml", "info:\n  name: Dup\n  type: folder\n"),
        ("y/r.yml", &request("R2", "{{b}}/6", WITH_ASSERTION)),
    ]);
    let report = run(&ws);
    let mut found = paths(&report, LintRule::DuplicateName);
    found.sort();
    assert_eq!(found, vec!["a/one.yml", "a/two.yml", "x/", "y/"]);
    assert!(of(&report, LintRule::DuplicateName).iter().all(|f| f.level == LintLevel::Error));
}

#[test]
fn file_names_that_differ_only_in_case_collide() {
    let ws = workspace(&[
        ("List.yml", &request("List one", "{{b}}/1", WITH_ASSERTION)),
        ("list.yml", &request("List two", "{{b}}/2", WITH_ASSERTION)),
        ("Other.yml", &request("Other", "{{b}}/3", WITH_ASSERTION)),
    ]);
    let report = run(&ws);
    let mut found = paths(&report, LintRule::DuplicateName);
    found.sort();
    assert_eq!(found, vec!["List.yml", "list.yml"]);
    assert_eq!(of(&report, LintRule::DuplicateName)[0].location.as_deref(), Some("file name"));
}

// ---- the score ----

fn clean() -> Workspace {
    with_collection_file(
        "opencollection: \"1.0.0\"\ninfo:\n  name: shop\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.test\n",
        &[
            ("a.yml", &request("A", "{{baseUrl}}/a", WITH_ASSERTION)),
            ("b.yml", &request("B", "{{baseUrl}}/b", WITH_ASSERTION)),
        ],
    )
}

#[test]
fn a_clean_collection_scores_100_and_has_no_findings() {
    let report = run(&clean());
    assert_eq!(report.score.score, 100);
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert_eq!(report.collection, "shop");
    assert_eq!(report.folders[0].folder, "");
    assert_eq!(report.folders[0].score, report.score);
    assert_eq!(report.score.rules.len(), 4);
}

#[test]
fn the_score_is_the_weighted_mean_of_the_satisfaction_per_rule() {
    // Tests 40, hosts 25, variables 15, names 20. Four requests: one without a test (3/4),
    // one hard-coded host (3/4), one unused variable of two (1/2), no duplicates (4/4).
    let ws = with_collection_file(
        "opencollection: \"1.0.0\"\ninfo:\n  name: shop\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.test\n    - name: unused\n      value: 1\n",
        &[
            ("a.yml", &request("A", "{{baseUrl}}/a", WITH_ASSERTION)),
            ("b.yml", &request("B", "{{baseUrl}}/b", WITH_ASSERTION)),
            ("c.yml", &request("C", "https://fixed.example.com/c", WITH_ASSERTION)),
            ("d.yml", &request("D", "{{baseUrl}}/d", "")),
        ],
    );
    let report = run(&ws);
    let score = |r: LintRule| report.score.rules.iter().find(|s| s.rule == r).map(|s| (s.ok, s.total)).unwrap();
    assert_eq!(score(LintRule::NoTests), (3, 4));
    assert_eq!(score(LintRule::HardCodedHost), (3, 4));
    assert_eq!(score(LintRule::OrphanVariable), (1, 2));
    assert_eq!(score(LintRule::DuplicateName), (4, 4));
    // (40*0.75 + 25*0.75 + 15*0.5 + 20*1.0) / 100 = 0.76
    assert_eq!(report.score.score, 76);
    // Reproducible.
    assert_eq!(run(&ws), report);
}

#[test]
fn folders_get_their_own_score() {
    let ws = workspace(&[
        ("good/a.yml", &request("A", "{{b}}/a", WITH_ASSERTION)),
        ("bad/b.yml", &request("B", "{{b}}/b", "")),
    ]);
    let report = run(&ws);
    let folder = |f: &str| report.folders.iter().find(|x| x.folder == f).map(|x| x.score.score).unwrap();
    assert_eq!(folder("good"), 100);
    assert!(folder("bad") < 100);
    assert!(folder("") < 100 && folder("") > folder("bad"));
}

#[test]
fn a_rule_without_anything_to_check_does_not_count() {
    // No variables at all: orphan-variable has total 0 and no weight in the mean.
    let ws = workspace(&[("a.yml", &request("A", "{{b}}/a", WITH_ASSERTION))]);
    let report = run(&ws);
    let orphan = report.score.rules.iter().find(|s| s.rule == LintRule::OrphanVariable).unwrap();
    assert_eq!(orphan.total, 0);
    assert_eq!(report.score.score, 100);
    assert_eq!(run(&workspace(&[])).score.score, 100);
}

// ---- settings and ignoring ----

#[test]
fn settings_change_levels_switch_rules_off_and_survive_a_save() {
    let mut ws = workspace(&[("a.yml", &request("A", "https://x.example.com/a", ""))]);
    assert_eq!(of(&run(&ws), LintRule::NoTests)[0].level, LintLevel::Warn);
    let mut settings = lint::settings(&ws);
    assert_eq!(settings, LintSettings::default());
    settings.rules.insert("lint/no-tests".into(), LintLevel::Error);
    settings.rules.insert("lint/hard-coded-host".into(), LintLevel::Off);
    settings.min_score = Some(90);
    lint::set_settings(&mut ws, &settings).unwrap();
    assert!(ws.dirty().iter().any(|p| p == COLLECTION_FILE), "{:?}", ws.dirty());
    assert_eq!(lint::settings(&ws), settings);
    let report = run(&ws);
    assert_eq!(of(&report, LintRule::NoTests)[0].level, LintLevel::Error);
    assert!(of(&report, LintRule::HardCodedHost).is_empty());
    // Off rules have nothing to check and no weight.
    let hosts = report.score.rules.iter().find(|s| s.rule == LintRule::HardCodedHost).unwrap();
    assert_eq!(hosts.total, 0);
    // Saved and reopened.
    let saved = ws.save().unwrap();
    let file = saved.iter().find(|c| c.path == COLLECTION_FILE).expect("collection file");
    assert!(file.text.contains("lint/no-tests: error"), "{}", file.text);
    let mut files = FileSet::default();
    for change in saved {
        files.files.insert(change.path, change.text.into_bytes());
    }
    let again = Workspace::open(files).unwrap();
    assert_eq!(lint::settings(&again), settings);
}

#[test]
fn ignoring_a_finding_leaves_it_out_and_counts_it() {
    let mut ws = workspace(&[
        ("legacy/a.yml", &request("A", "{{b}}/a", "")),
        ("keep/b.yml", &request("B", "{{b}}/b", "")),
        ("keep/c.yml", &request("C", "{{b}}/c", "")),
    ]);
    assert_eq!(of(&run(&ws), LintRule::NoTests).len(), 3);
    lint::ignore(&mut ws, "keep/c.yml", LintRule::NoTests).unwrap();
    let settings = lint::settings(&ws);
    assert_eq!(settings.ignore, vec!["lint/no-tests:keep/c.yml"]);
    // Twice is once.
    lint::ignore(&mut ws, "keep/c.yml", LintRule::NoTests).unwrap();
    assert_eq!(lint::settings(&ws).ignore.len(), 1);
    let report = run(&ws);
    assert_eq!(paths(&report, LintRule::NoTests).len(), 2);
    assert_eq!(report.ignored, 1);
    // A path pattern leaves out every rule below it.
    let mut settings = lint::settings(&ws);
    settings.ignore.push("legacy/*".into());
    lint::set_settings(&mut ws, &settings).unwrap();
    let report = run(&ws);
    assert_eq!(paths(&report, LintRule::NoTests), vec!["keep/b.yml"]);
    assert_eq!(report.ignored, 2);
    // Ignored requests are not checked: the folder score looks at what is left.
    let legacy = report.folders.iter().find(|f| f.folder == "legacy");
    assert!(legacy.is_none_or(|f| f.score.score == 100));
}

#[test]
fn a_rule_scoped_ignore_does_not_hide_other_rules() {
    let mut ws = workspace(&[("a.yml", &request("A", "https://x.example.com/a", ""))]);
    lint::ignore(&mut ws, "a.yml", LintRule::NoTests).unwrap();
    let report = run(&ws);
    assert!(of(&report, LintRule::NoTests).is_empty());
    assert_eq!(of(&report, LintRule::HardCodedHost).len(), 1);
}

#[test]
fn the_lint_does_not_change_the_workspace() {
    let ws = clean();
    let before = ws.dirty();
    let _ = run(&ws);
    assert_eq!(ws.dirty(), before);
}

#[test]
fn findings_come_in_a_fixed_order() {
    let ws = workspace(&[
        ("z.yml", &request("Z", "https://z.example.com", "")),
        ("a.yml", &request("A", "https://a.example.com", "")),
    ]);
    let report = run(&ws);
    let order: Vec<(&str, LintRule)> = report.findings.iter().map(|f| (f.path.as_str(), f.rule)).collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}
