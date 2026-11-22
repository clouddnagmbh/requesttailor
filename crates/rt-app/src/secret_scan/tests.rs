// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Unit tests of the secret scanner: what it must find, and the look-alikes it must not.
//!
//! Fake credentials are assembled at run time (`fake_*`), so the repository itself stays
//! clean for gitleaks.

use super::*;

fn fake_github() -> String {
    format!("{}{}", "gh", "p_") + "aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3z05"
}

fn fake_aws_key_id() -> String {
    format!("{}{}", "AK", "IA") + "Z7Q3R5T9W2X4Y6V8"
}

fn fake_jwt() -> String {
    // header {"alg":"HS256","typ":"JWT"}, payload {"sub":"1234567890"}, signature.
    [
        format!("{}{}", "ey", "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
        format!("{}{}", "ey", "JzdWIiOiIxMjM0NTY3ODkwIn0"),
        "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c".to_owned(),
    ]
    .join(".")
}

fn fake_slack() -> String {
    format!("{}{}", "xo", "xb-") + "1234567890-0987654321-AbCdEfGhIjKlMnOpQrStUvWx"
}

fn fake_sk() -> String {
    format!("{}{}", "s", "k-") + "proj-Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56"
}

fn fake_private_key_line() -> String {
    format!("-----BEGIN {}-----", ["RSA", "PRIVATE", "KEY"].join(" "))
}

/// `(line, kind)` of each finding in one file.
fn found(text: &str) -> Vec<(u32, String)> {
    scan(&[("r.yml", text)])
        .into_iter()
        .map(|f| (f.line, f.kind))
        .collect()
}

fn one(text: &str) -> SecretFinding {
    let all = scan(&[("requests/r.yml", text)]);
    assert_eq!(all.len(), 1, "{all:#?}");
    all.into_iter().next().unwrap()
}

fn assert_clean(text: &str) {
    let all = scan(&[("r.yml", text)]);
    assert!(all.is_empty(), "{text}\n{all:#?}");
}

// ---------------------------------------------------------------------------------------------
// Positives

#[test]
fn bearer_token_in_auth_block() {
    let text = format!(
        "info:\n  name: Me\nhttp:\n  method: GET\n  url: https://api.example.com/me\n  auth:\n    type: bearer\n    token: {}\n",
        ["3f9a1c77", "e2b54d0c9a8b"].concat()
    );
    let f = one(&text);
    assert_eq!(f.path, "requests/r.yml");
    assert_eq!(f.line, 8);
    assert_eq!(f.kind, "token");
    assert_eq!(f.snippet, "token: ***");
    assert!(f.hint.contains("secret"), "{}", f.hint);
    assert!(f.hint.contains("{{token}}"), "{}", f.hint);
    assert!(f.hint.contains("rt-scan:allow"), "{}", f.hint);
}

#[test]
fn basic_auth_password() {
    let text = "auth:\n  type: basic\n  username: admin\n  password: hunter2\n";
    let f = one(text);
    assert_eq!((f.line, f.kind.as_str()), (4, "password"));
    assert_eq!(f.snippet, "password: ***");
}

#[test]
fn authorization_header_with_bearer_and_basic_values() {
    let text = "headers:\n  - name: Authorization\n    value: Bearer 9c1e2f3a4b5c6d7e8f90\n  - name: Proxy-Authorization\n    value: Basic dXNlcjpzM2NyM3Q=\n";
    assert_eq!(
        found(text),
        [
            (3, "bearer token".to_owned()),
            (5, "basic credentials".to_owned())
        ]
    );
    let all = scan(&[("r.yml", text)]);
    assert_eq!(all[0].snippet, "value: Bearer ***");
    assert_eq!(all[1].snippet, "value: Basic ***");
}

#[test]
fn api_key_headers_and_api_key_auth() {
    let text = "headers:\n  - name: X-API-Key\n    value: 0a1b2c3d4e5f\n  - name: Ocp-Apim-Subscription-Key\n    value: abcdef0123456789\nauth:\n  type: apikey\n  key: x-api-key\n  value: k3y-v4lu3\n  placement: header\n";
    assert_eq!(
        found(text),
        [
            (3, "API key".to_owned()),
            (5, "API key".to_owned()),
            (9, "API key".to_owned())
        ]
    );
    let all = scan(&[("r.yml", text)]);
    assert!(all[0].hint.contains("{{xApiKey}}"), "{}", all[0].hint);
}

#[test]
fn oauth2_client_secret_and_password_flow() {
    let text = "auth:\n  type: oauth2\n  flow: resource_owner_password_credentials\n  accessTokenUrl: https://id.example.com/token\n  credentials:\n    clientId: my-app\n    clientSecret: c0ffee-s3cr3t\n  resourceOwner:\n    username: alice\n    password: wonderland\n";
    assert_eq!(
        found(text),
        [(7, "client secret".to_owned()), (10, "password".to_owned())]
    );
}

#[test]
fn aws_keys() {
    let text = format!(
        "auth:\n  type: awsv4\n  accessKeyId: {}\n  secretAccessKey: {}\n  sessionToken: \"{{{{awsSession}}}}\"\n",
        fake_aws_key_id(),
        ["wJalrXUtnFEMI/K7MDENG", "/bPxRfiCYzzzzKEY"].concat()
    );
    assert_eq!(
        found(&text),
        [
            (3, "AWS access key".to_owned()),
            (4, "AWS secret access key".to_owned())
        ]
    );
    let all = scan(&[("r.yml", &text)]);
    assert!(!all[0].snippet.contains(&fake_aws_key_id()));
}

#[test]
fn private_keys_anywhere() {
    let text = format!(
        "body:\n  type: text\n  data: |-\n    {}\n    MIIEowIBAAKCAQEA\n",
        fake_private_key_line()
    );
    let f = one(&text);
    assert_eq!((f.line, f.kind.as_str()), (4, "private key"));
    assert_eq!(f.snippet, "***");
    assert!(f.hint.contains(".gitignore"), "{}", f.hint);
    // A .pem file next to the requests.
    let pem = format!("{}\nMIIEowIBAAKCAQEA\n", fake_private_key_line());
    assert_eq!(scan(&[("certs/client.key", &pem)])[0].kind, "private key");
}

#[test]
fn known_token_prefixes() {
    let cases = [
        (fake_github(), "GitHub token"),
        (fake_slack(), "Slack token"),
        (fake_sk(), "API key (sk-)"),
        (fake_aws_key_id(), "AWS access key"),
        (fake_jwt(), "JWT"),
        (
            format!("{}{}", "gl", "pat-") + "Ab12Cd34Ef56Gh78Ij90",
            "GitLab token",
        ),
        (
            format!("{}{}", "AI", "za") + "SyA1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q",
            "Google API key",
        ),
        (
            format!("{}{}", "np", "m_") + "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56Qr78",
            "npm token",
        ),
        (
            format!("{}{}", "sk_", "live_") + "4eC39HqLyjWDarjtT1zdp7dc",
            "Stripe key",
        ),
    ];
    for (token, kind) in cases {
        // In a description, where no key says it is a credential.
        let text = format!("docs: \"try it with {token} please\"\n");
        let f = one(&text);
        assert_eq!(f.kind, kind, "{token}");
        assert!(!f.snippet.contains(&token), "{}", f.snippet);
        assert_eq!(f.snippet, "docs: \"try it with *** please\"");
    }
}

#[test]
fn tokens_in_bodies_and_curl_text() {
    let text = format!(
        "body:\n  type: json\n  data: |-\n    {{\n      \"username\": \"alice\",\n      \"password\": \"Tr0ub4dor&3\",\n      \"refresh_token\": \"{}\"\n    }}\ndocs: |-\n  curl -H 'Authorization: Bearer {}' https://x\n",
        fake_jwt(),
        ["7d9f0a1b", "2c3d4e5f6a7b"].concat()
    );
    assert_eq!(
        found(&text),
        [
            (6, "password".to_owned()),
            (7, "JWT".to_owned()),
            (10, "bearer token".to_owned())
        ]
    );
}

#[test]
fn passwords_and_tokens_in_urls() {
    let text = "http:\n  url: https://admin:pa55word@db.example.com/x?api_key=abc123def&page=2\n";
    let f = one(text);
    assert_eq!(f.kind, "password in URL, API key");
    assert_eq!(
        f.snippet,
        "url: https://admin:***@db.example.com/x?api_key=***&page=2"
    );
    let text = "url: \"{{baseUrl}}/search?access_token=ya29.a0AfH6SM\"\n";
    assert_eq!(found(text), [(1, "token".to_owned())]);
}

#[test]
fn variables_with_credential_names_and_values() {
    let text = "name: Dev\nvariables:\n  - name: baseUrl\n    value: https://dev.example.com\n  - name: apiToken\n    value: 5e6f7a8b9c0d\n";
    let f = one(text);
    assert_eq!((f.line, f.kind.as_str()), (6, "token"));
    assert!(
        f.hint.contains("Mark the variable `apiToken` as secret"),
        "{}",
        f.hint
    );
}

#[test]
fn a_secret_variable_with_a_value_in_the_file() {
    let text = "name: Dev\nvariables:\n  - secret: true\n    name: baseUrl\n    value: https://leaked.example.com\n";
    let f = one(text);
    assert_eq!((f.line, f.kind.as_str()), (5, "secret variable value"));
    assert!(f.hint.contains("`baseUrl`"), "{}", f.hint);
}

#[test]
fn bruno_compact_sequences_and_crlf() {
    let text = "variables:\r\n- name: password\r\n  value: letmein\r\n";
    let f = one(text);
    assert_eq!((f.line, f.kind.as_str()), (3, "password"));
    assert_eq!(f.snippet, "value: ***");
}

#[test]
fn several_files_and_line_numbers() {
    let a = &format!(
        "auth:\n  type: bearer\n  token: {}\n",
        ["abcdef", "123456"].concat()
    );
    let b = "x: 1\n\n\npassword: \"s3cr3t\" # typed by hand\n";
    let all = scan(&[("a.yml", a), ("dir/b.yml", b)]);
    let got: Vec<(&str, u32)> = all.iter().map(|f| (f.path.as_str(), f.line)).collect();
    assert_eq!(got, [("a.yml", 3), ("dir/b.yml", 4)]);
    assert_eq!(all[1].snippet, "password: \"***\" # typed by hand");
}

#[test]
fn one_finding_per_line_naming_the_most_specific_kind() {
    let text = format!("auth:\n  type: bearer\n  token: {}\n", fake_github());
    let f = one(&text);
    assert_eq!(f.kind, "GitHub token");
    assert_eq!(f.snippet, "token: ***");
    let text = format!(
        "headers:\n  - name: Authorization\n    value: Bearer {}\n",
        fake_jwt()
    );
    assert_eq!(one(&text).kind, "JWT");
}

#[test]
fn snippets_never_contain_the_value_and_are_cut() {
    let long = format!("docs: \"{} {}\"", "word ".repeat(60), fake_github());
    let f = one(&long);
    assert!(!f.snippet.contains(&fake_github()));
    assert!(f.snippet.chars().count() <= SNIPPET_CHARS + 1);
    assert!(f.snippet.ends_with('…'));
}

#[test]
fn refusal_message_names_the_first_finding() {
    let all = scan(&[("a.yml", "token: abc12345\npassword: hunter2\n")]);
    let m = refusal(&all);
    assert!(
        m.starts_with("Commit refused: 2 credentials in 1 file (a.yml:1 token)."),
        "{m}"
    );
}

// ---------------------------------------------------------------------------------------------
// Not findings

#[test]
fn variables_and_masks_are_fine() {
    assert_clean(
        "auth:\n  type: bearer\n  token: \"{{token}}\"\n\
         headers:\n  - name: Authorization\n    value: Bearer {{accessToken}}\n\
         \x20 - name: Authorization\n    value: Basic {{basic}}\n\
         \x20 - name: X-API-Key\n    value: \"{{ apiKey }}\"\n\
         password: \"***\"\n\
         clientSecret: \"{{process.env.CLIENT_SECRET}}\"\n\
         token: $TOKEN\n\
         token: ${TOKEN}\n\
         secretAccessKey: <your-secret-access-key>\n\
         url: https://{{user}}:{{password}}@example.com/x?token={{t}}\n\
         password: \"\"\n\
         password:\n\
         token: xxxx\n",
    );
}

#[test]
fn look_alikes_are_fine() {
    assert_clean(
        "name: Create token\n\
         docs: Use a Bearer token to authenticate. Bearer authentication is described below.\n\
         tokenUrl: https://id.example.com/oauth/token\n\
         accessTokenUrl: https://id.example.com/token\n\
         tokenPlacement: header\n\
         tokenHeaderPrefix: Bearer\n\
         secretName: prod/db-password\n\
         secretReference: op://Private/api/token\n\
         passwordPolicy: strict\n\
         variables:\n  - secret: true\n    name: password\n\
         \x20 - name: page\n    value: \"2\"\n\
         headers:\n  - name: X-CSRF-Token\n    value: fetch\n\
         \x20 - name: Authorization\n    value: OreApi apikey={{api_key}}, session=\"{{s}}\"\n\
         \x20 - name: Content-Type\n    value: application/json\n\
         params:\n  - name: pageToken\n    value: CAoQAA\n\
         \x20 - name: nextToken\n    value: abc\n\
         body: { type: json }\n\
         task-sk-learn-model-runner-collection: 1\n\
         url: https://example.com/a?key=AIzaShort&format=json\n\
         auth:\n  type: apikey\n  key: X-API-Key\n  placement: header\n",
    );
    // AWS's documented example key, a basic header that is no base64 `user:password`.
    assert_clean(&format!(
        "accessKeyId: AKIA{}\nnote: Basic auth is supported\n",
        "IOSFODNN7EXAMPLE"
    ));
    // Booleans and secret flags.
    assert_clean("secret: true\nsecret: false\npassword: null\n");
}

#[test]
fn allow_markers_suppress_one_line() {
    assert_clean("password: demo # rt-scan:allow\n");
    assert_clean("# rt-scan:allow-next-line\npassword: demo\n");
    // Only that line.
    assert_eq!(
        found("password: demo # rt-scan:allow\npassword: other\n"),
        [(2, "password".to_owned())]
    );
    assert_eq!(
        found("# rt-scan:allow-next-line\npassword: demo\npassword: other\n"),
        [(3, "password".to_owned())]
    );
}

#[test]
fn text_of_skips_binary_and_huge_files() {
    assert_eq!(text_of(b"a: 1\n"), Some("a: 1\n"));
    assert_eq!(text_of(b"\x00\x01PNG"), None);
    assert_eq!(text_of(&[0xff, 0xfe, b'a']), None);
    assert_eq!(text_of(&vec![b'a'; MAX_SCAN_BYTES + 1]), None);
}

#[test]
fn lines_are_parsed_like_yaml() {
    let l = parse_line("  - name: \"X-Key\" # c");
    assert!(l.item);
    assert_eq!(l.col, Some(4));
    assert_eq!(l.key, Some("name"));
    assert_eq!(l.value_text(), Some("X-Key"));
    let l = parse_line("    \"pass\": 'it''s',");
    assert_eq!(l.key, Some("pass"));
    assert_eq!(l.value_text(), Some("it''s"));
    let l = parse_line("url: https://x.example/a#frag");
    assert_eq!(l.value_text(), Some("https://x.example/a#frag"));
    assert_eq!(parse_line("data: |-").value, None);
    assert_eq!(parse_line("# comment").col, None);
    assert_eq!(parse_line("just text").key, None);
}
