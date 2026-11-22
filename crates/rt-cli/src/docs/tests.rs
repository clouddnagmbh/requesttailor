// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt docs` against the fixture collection `testdata/shop` and in-memory collections.

use std::collections::HashSet;
use std::path::Path;

use rt_format::{FileSet, read_collection};

use super::*;

fn shop() -> rt_format::CollectionDoc {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/shop");
    crate::load_collection(&dir).unwrap()
}

fn in_memory(files: &[(&str, &str)]) -> rt_format::CollectionDoc {
    read_collection(&FileSet {
        files: files
            .iter()
            .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
            .collect(),
    })
    .unwrap()
}

fn attr<'h>(tag: &'h str, name: &str) -> Option<&'h str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[at..].find('"')? + at;
    Some(&tag[at..end])
}

/// The website's rules (scripts/check-website.mjs) for a generated page: nothing runs,
/// nothing is loaded from elsewhere, links stay on the page.
pub(super) fn site_rule_violations(html: &str, css: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let lower = html.to_ascii_lowercase();
    if !lower.starts_with("<!doctype html>") {
        errors.push("no doctype".to_owned());
    }
    for needle in [
        "<html lang=\"en\">",
        "<meta charset=\"utf-8\">",
        "<meta name=\"viewport\"",
        "<main id=\"main\">",
        "href=\"#main\"",
    ] {
        if !html.contains(needle) {
            errors.push(format!("missing {needle}"));
        }
    }
    match html
        .split("<meta http-equiv=\"Content-Security-Policy\" content=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
    {
        Some(csp) if csp.starts_with("default-src 'none'") => {
            if csp.contains("http") || csp.contains('*') {
                errors.push(format!("CSP allows other servers: {csp}"));
            }
        }
        _ => errors.push("no CSP with default-src 'none'".to_owned()),
    }
    if lower.matches("<h1").count() != 1 {
        errors.push("not exactly one h1".to_owned());
    }
    for tag in [
        "script", "iframe", "frame", "object", "embed", "form", "base", "style", "img",
    ] {
        if lower.contains(&format!("<{tag} ")) || lower.contains(&format!("<{tag}>")) {
            errors.push(format!("<{tag}> found"));
        }
    }
    if lower.contains(" style=") {
        errors.push("inline style attribute".to_owned());
    }
    // Inline event handlers: ` on<letters>=` inside a tag.
    for tag in html
        .split('<')
        .skip(1)
        .map(|t| t.split('>').next().unwrap_or(""))
    {
        if tag.split_whitespace().skip(1).any(|a| {
            a.len() > 3
                && a[..2].eq_ignore_ascii_case("on")
                && a.contains('=')
                && a[2..a.find('=').unwrap()]
                    .chars()
                    .all(|c| c.is_ascii_alphabetic())
        }) {
            errors.push(format!("event handler in <{tag}>"));
        }
    }
    let ids: HashSet<&str> = html
        .match_indices(" id=\"")
        .filter_map(|(i, _)| html[i + 5..].split('"').next())
        .collect();
    for tag in html
        .split('<')
        .skip(1)
        .map(|t| t.split('>').next().unwrap_or(""))
    {
        let name = tag.split_whitespace().next().unwrap_or("");
        for key in ["href", "src", "srcset", "poster", "action", "data"] {
            let Some(url) = attr(tag, key) else { continue };
            match (name, url) {
                ("link", "style.css") => {}
                ("a", u) if u.starts_with('#') => {
                    if !ids.contains(&u[1..]) {
                        errors.push(format!("link {u} has no target"));
                    }
                }
                _ => errors.push(format!("<{name} {key}=\"{url}\"> leaves the page")),
            }
        }
    }
    let css_lower = css.to_ascii_lowercase();
    if css_lower.contains("@import") || css_lower.contains("url(") {
        errors.push("the stylesheet loads something".to_owned());
    }
    errors
}

#[test]
fn documents_folders_requests_and_examples() {
    let r = render(&shop(), Some("Local")).unwrap();
    let html = &r.html;
    assert_eq!(r.requests, 5);
    assert!(html.contains("<h1>Shop</h1>"), "{html}");
    assert!(html.contains("A small shop API with saved examples."));
    assert!(html.contains("<h2 id=\"f-users\">Users</h2>"));
    assert!(html.contains("Everything about users."));
    // Method, name, the URL with the environment's variables, description and docs.
    assert!(html.contains(
        "<h3 id=\"r-users-get-user-yml\"><span class=\"method get\">GET</span> Get user</h3>"
    ));
    assert!(html.contains("<code>http://localhost:4010/api/v1/users/:id</code>"));
    assert!(html.contains("Reads one user by id."));
    assert!(html.contains("Returns the user with the given id, or 404."));
    // Parameters, headers, body.
    assert!(
        html.contains("<tr><td><code>id</code></td><td>path</td><td><code>42</code></td></tr>")
    );
    assert!(
        html.contains(
            "<tr><td><code>Accept</code></td><td><code>application/json</code></td></tr>"
        )
    );
    assert!(html.contains("<h4>Body <span class=\"tag\">JSON</span></h4>"));
    assert!(html.contains("{&quot;name&quot;: &quot;Grace&quot;}"));
    // Auth: the type and the variables, inherited or not.
    assert!(html.contains("Bearer token <code>{{token}}</code>, from the collection"));
    assert!(html.contains(
        "Basic, user <code>{{adminUser}}</code>, password <code>{{adminPassword}}</code>"
    ));
    // Examples with status, headers and bodies.
    assert!(html.contains("<summary>Found <span class=\"status s2\">200 OK</span></summary>"));
    assert!(
        html.contains(
            "<summary>Not found <span class=\"status s4\">404 Not Found</span></summary>"
        )
    );
    assert!(html.contains("<code>X-Request-Id</code>"));
    assert!(html.contains("&quot;Ada&quot;"));
    assert!(html.contains("&lt;order id=&quot;A-1&quot;/&gt;"));
    // Table of contents links to every request.
    assert!(html.contains("<a href=\"#r-orders-get-order-yml\">"));
}

#[test]
fn without_an_environment_values_stay_as_written() {
    let html = render(&shop(), None).unwrap().html;
    assert!(
        html.contains("<code>{{baseUrl}}/v1/users/:id</code>"),
        "{html}"
    );
    assert!(html.contains("No environment"));
}

#[test]
fn secret_variables_stay_placeholders() {
    let html = render(&shop(), Some("Local")).unwrap().html;
    // The environment table shows secrets as {{name}}, non-secrets with their value.
    assert!(html.contains(
        "<tr><td><code>token</code></td><td><code>{{token}}</code> <span class=\"tag\">secret</span></td></tr>"
    ));
    assert!(html.contains("<tr><td><code>adminUser</code></td><td><code>admin</code></td></tr>"));
    // Auth is never interpolated, even with the environment.
    assert!(!html.contains("user <code>admin</code>"));
}

#[test]
fn values_that_look_like_credentials_are_masked() {
    // Built at run time, so the repository's own secret scans do not see a credential.
    let token = format!("{}{}", "ghp_", "Zy9Xw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe9Dc8");
    let request = format!(
        "info:\n  name: Leaky\n  type: http\nhttp:\n  method: POST\n  url: https://api.example.com/x?access_token={token}\n  headers:\n    - name: Authorization\n      value: Bearer {token}\n    - name: X-Api-Key\n      value: plain-key-value-1234\n  body:\n    type: json\n    data: |-\n      {{\"user\": \"ada\", \"token\": \"{token}\"}}\n  auth:\n    type: basic\n    username: ada\n    password: correct-horse-battery\n"
    );
    let env = format!(
        "name: Dev\nvariables:\n  - name: apiToken\n    value: {token}\n  - name: host\n    value: api.example.com\n"
    );
    let doc = in_memory(&[
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: Leaks\n",
        ),
        ("leaky.yml", &request),
        ("environments/Dev.yml", &env),
    ]);
    let r = render(&doc, Some("Dev")).unwrap();
    assert!(!r.html.contains(&token), "{}", r.html);
    assert!(!r.html.contains("plain-key-value-1234"));
    assert!(!r.html.contains("correct-horse-battery"));
    assert!(r.html.contains("api.example.com"));
    assert!(
        r.html.contains("&quot;user&quot;: &quot;ada&quot;"),
        "{}",
        r.html
    );
    assert!(r.masked >= 5, "masked {}", r.masked);
}

#[test]
fn the_page_follows_the_website_rules() {
    for env in [None, Some("Local")] {
        let html = render(&shop(), env).unwrap().html;
        assert_eq!(site_rule_violations(&html, STYLE), Vec::<String>::new());
    }
    // The checker itself sees violations.
    let bad = "<!doctype html><html lang=\"en\"><script src=\"https://x\"></script><a href=\"https://x\" onclick=\"y\">";
    let errors = site_rule_violations(bad, "@import url(https://x);");
    assert!(errors.iter().any(|e| e.contains("<script>")), "{errors:?}");
    assert!(
        errors.iter().any(|e| e.contains("event handler")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("leaves the page")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.contains("stylesheet")),
        "{errors:?}"
    );
}

#[test]
fn names_and_texts_are_escaped() {
    let doc = in_memory(&[
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: \"<b>Bold</b>\"\n",
        ),
        (
            "x.yml",
            "info:\n  name: \"<script>alert(1)</script>\"\n  type: http\nhttp:\n  method: GET\n  url: \"/a?b=<c>\"\ndocs: \"<img src=x onerror=alert(1)>\"\n",
        ),
    ]);
    let html = render(&doc, None).unwrap().html;
    assert!(html.contains("<h1>&lt;b&gt;Bold&lt;/b&gt;</h1>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert_eq!(site_rule_violations(&html, STYLE), Vec::<String>::new());
}

#[test]
fn unknown_environment_is_an_error() {
    assert!(render(&shop(), Some("Prod")).is_err());
}

#[test]
fn parses_arguments() {
    let parse_args = |args: &[&str]| parse(&mut lexopt::Parser::from_args(args.iter().copied()));
    let a = parse_args(&["coll", "--out", "site", "-e", "Local"])
        .unwrap()
        .unwrap();
    assert_eq!(a.collection, std::path::PathBuf::from("coll"));
    assert_eq!(a.out, Some(std::path::PathBuf::from("site")));
    assert_eq!(a.environment.as_deref(), Some("Local"));
    let a = parse_args(&["-o", "site"]).unwrap().unwrap();
    assert_eq!(a.collection, std::path::PathBuf::from("."));
    assert!(parse_args(&[]).unwrap_err().contains("--out"));
    assert!(parse_args(&["--help"]).unwrap().unwrap().help);
}

#[test]
fn writes_index_and_stylesheet() {
    let out = std::env::temp_dir().join(format!("rt-docs-unit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let args = Args {
        collection: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/shop/users"),
        out: Some(out.clone()),
        environment: Some("Local".into()),
        help: false,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(run(args, &mut stdout, &mut stderr), crate::EXIT_OK);
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(stdout.contains("(5 requests)"), "{stdout}");
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("<h1>Shop</h1>"));
    assert_eq!(
        std::fs::read_to_string(out.join("style.css")).unwrap(),
        STYLE
    );
    let _ = std::fs::remove_dir_all(&out);
}
