// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests for `Scope::get` and `Scope::interpolate`. Pure: no I/O, so they mean the
//! same on native and wasm32.

use super::*;

fn layer(source: VarSource, vars: &[(&str, &str)]) -> VarLayer {
    VarLayer {
        source,
        vars: vars
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned(), false))
            .collect(),
    }
}

fn scope(layers: Vec<VarLayer>) -> Scope {
    Scope { layers }
}

fn env(name: &str) -> VarSource {
    VarSource::Environment { name: name.into() }
}

fn folder(path: &str) -> VarSource {
    VarSource::Folder { path: path.into() }
}

fn names(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// All layers in precedence order, each defining `x` as its own label.
fn all_layers() -> Vec<VarLayer> {
    vec![
        layer(VarSource::Global, &[("x", "global")]),
        layer(VarSource::Collection, &[("x", "collection")]),
        layer(env("dev"), &[("x", "environment")]),
        layer(folder("outer"), &[("x", "folder-outer")]),
        layer(folder("outer/inner"), &[("x", "folder-inner")]),
        layer(VarSource::Request, &[("x", "request")]),
        layer(VarSource::Runtime, &[("x", "runtime")]),
    ]
}

#[test]
fn precedence_table_highest_layer_wins_and_each_removal_falls_back() {
    let full = all_layers();
    let labels: Vec<String> = full.iter().map(|l| l.vars[0].1.clone()).collect();

    // Everything present: runtime wins.
    let s = scope(full.clone());
    assert_eq!(s.get("x").map(|(v, _, _)| v), Some("runtime"));
    assert_eq!(s.interpolate("{{x}}").value, "runtime");

    // Remove each layer in turn: the highest remaining layer wins.
    for removed in 0..full.len() {
        let mut layers = full.clone();
        layers.remove(removed);
        let expected = if removed == full.len() - 1 {
            &labels[full.len() - 2]
        } else {
            &labels[full.len() - 1]
        };
        let s = scope(layers.clone());
        assert_eq!(s.interpolate("{{x}}").value, *expected, "removed {removed}");
    }

    // Peel layers from the top: each lower layer wins once everything above is gone.
    for keep in (1..=full.len()).rev() {
        let s = scope(full[..keep].to_vec());
        let i = s.interpolate("{{x}}");
        assert_eq!(i.value, labels[keep - 1]);
        assert_eq!(i.used[0].source, full[keep - 1].source);
    }
}

#[test]
fn get_returns_value_source_and_secret_flag() {
    let mut s = scope(all_layers());
    s.layers[2].vars.push(("token".into(), "t0p".into(), true));
    assert_eq!(s.get("token"), Some(("t0p", &env("dev"), true)));
    assert_eq!(s.get("x"), Some(("runtime", &VarSource::Runtime, false)));
    assert_eq!(s.get("missing"), None);
    assert_eq!(Scope::default().get("x"), None);
}

#[test]
fn later_definition_in_the_same_layer_wins() {
    let s = scope(vec![layer(
        VarSource::Request,
        &[("a", "first"), ("a", "second")],
    )]);
    assert_eq!(s.get("a").map(|(v, _, _)| v), Some("second"));
}

#[test]
fn provenance_is_recorded_once_per_variable() {
    let mut s = scope(vec![
        layer(VarSource::Collection, &[("host", "example.com")]),
        layer(folder("users"), &[("path", "users")]),
    ]);
    s.layers[0]
        .vars
        .push(("token".into(), "s3cret".into(), true));
    let i = s.interpolate("https://{{host}}/{{path}}/{{host}}?t={{token}}");
    assert_eq!(i.value, "https://example.com/users/example.com?t=s3cret");
    assert_eq!(
        i.used,
        vec![
            VarUse {
                name: "host".into(),
                source: VarSource::Collection,
                secret: false
            },
            VarUse {
                name: "path".into(),
                source: folder("users"),
                secret: false
            },
            VarUse {
                name: "token".into(),
                source: VarSource::Collection,
                secret: true
            },
        ]
    );
    assert!(i.unresolved.is_empty());
}

#[test]
fn values_are_resolved_recursively_with_provenance_for_every_level() {
    let s = scope(vec![
        layer(VarSource::Global, &[("c", "deep")]),
        layer(env("dev"), &[("b", "{{c}}-mid")]),
        layer(VarSource::Request, &[("a", "<{{b}}>")]),
    ]);
    let i = s.interpolate("{{a}}!");
    assert_eq!(i.value, "<deep-mid>!");
    let used: Vec<(&str, &VarSource)> = i
        .used
        .iter()
        .map(|u| (u.name.as_str(), &u.source))
        .collect();
    assert_eq!(
        used,
        vec![
            ("a", &VarSource::Request),
            ("b", &env("dev")),
            ("c", &VarSource::Global)
        ]
    );
}

#[test]
fn recursion_uses_the_scope_not_the_layer_of_the_value() {
    // A low-layer value referencing a name that a higher layer overrides gets the override.
    let s = scope(vec![
        layer(
            VarSource::Collection,
            &[("url", "{{host}}/api"), ("host", "low")],
        ),
        layer(VarSource::Runtime, &[("host", "high")]),
    ]);
    assert_eq!(s.interpolate("{{url}}").value, "high/api");
}

#[test]
fn repeated_nested_placeholders_expand_every_occurrence() {
    let s = scope(vec![layer(
        VarSource::Request,
        &[
            ("r", "{{r2}} {{r2}}"),
            ("r2", "{{r3}}{{r3}}{{r3}}"),
            ("r3", "x"),
        ],
    )]);
    assert_eq!(s.interpolate("{{r}}").value, "xxx xxx");
}

#[test]
fn self_cycle_leaves_placeholder_and_reports_it() {
    let s = scope(vec![layer(VarSource::Request, &[("r", "{{r}}")])]);
    let i = s.interpolate("a {{r}} b");
    assert_eq!(i.value, "a {{r}} b");
    assert_eq!(names(&i.unresolved), ["r"]);
    assert!(i.used.is_empty());
}

#[test]
fn longer_cycles_leave_the_outer_placeholder_verbatim() {
    let s = scope(vec![layer(
        VarSource::Request,
        &[
            ("r1", "{{r2}}"),
            ("r2", "x{{r3}}"),
            ("r3", "{{r1}}"),
            ("ok", "fine"),
        ],
    )]);
    let i = s.interpolate("{{ok}} {{r1}} {{r2}}");
    assert_eq!(i.value, "fine {{r1}} {{r2}}");
    assert_eq!(names(&i.unresolved), ["r1", "r2"]);
    assert_eq!(i.used.len(), 1);
    assert_eq!(i.used[0].name, "ok");
}

#[test]
fn unresolved_names_stay_verbatim_and_are_reported_once_in_order() {
    let s = scope(vec![layer(VarSource::Request, &[("a", "x{{inner}}")])]);
    let i = s.interpolate("{{missing}}/{{a}}/{{missing}}/{{other}}");
    assert_eq!(i.value, "{{missing}}/x{{inner}}/{{missing}}/{{other}}");
    assert_eq!(names(&i.unresolved), ["missing", "inner", "other"]);
    assert_eq!(i.used.len(), 1);
}

#[test]
fn whitespace_inside_braces_is_part_of_the_name_like_bruno() {
    let s = scope(vec![layer(VarSource::Request, &[("name", "v")])]);
    let i = s.interpolate("{{ name }}|{{name}}");
    assert_eq!(i.value, "{{ name }}|v");
    assert_eq!(names(&i.unresolved), [" name "]);
}

#[test]
fn process_env_and_dynamic_variables_are_not_resolved() {
    let s = scope(vec![layer(VarSource::Request, &[("a", "1")])]);
    let i = s.interpolate("{{process.env.HOME}} {{$randomInt}} {{?Prompt}} {{a}}");
    assert_eq!(i.value, "{{process.env.HOME}} {{$randomInt}} {{?Prompt}} 1");
    assert_eq!(
        names(&i.unresolved),
        ["process.env.HOME", "$randomInt", "?Prompt"]
    );
}

#[test]
fn text_that_is_not_a_placeholder_is_untouched() {
    let s = scope(vec![layer(VarSource::Request, &[("a", "1")])]);
    for t in [
        "",
        "plain",
        "{{}}",
        "{a}",
        "{{a}",
        "{{a",
        "}}{{",
        "{ {a}}",
        "{\"json\": {\"a\": 1}}",
    ] {
        let i = s.interpolate(t);
        assert_eq!(i.value, t, "template {t:?}");
        assert!(i.unresolved.is_empty(), "template {t:?}");
        assert!(i.used.is_empty(), "template {t:?}");
    }
}

#[test]
fn placeholders_next_to_braces_and_unicode() {
    let s = scope(vec![layer(VarSource::Request, &[("a", "ä"), ("ü", "u")])]);
    assert_eq!(s.interpolate("{{{a}} {{ü}}}").value, "{{{a}} u}");
    assert_eq!(s.interpolate("{\"k\":\"{{a}}\"}").value, "{\"k\":\"ä\"}");
    assert_eq!(s.interpolate("{{a}}{{a}}").value, "ää");
}

#[test]
fn empty_values_resolve_to_empty_strings() {
    let s = scope(vec![layer(VarSource::Request, &[("empty", "")])]);
    let i = s.interpolate("[{{empty}}]");
    assert_eq!(i.value, "[]");
    assert!(i.unresolved.is_empty());
    assert_eq!(i.used.len(), 1);
}
