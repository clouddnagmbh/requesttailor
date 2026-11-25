// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `security_passive` (wave 19, strand R): every passive security rule, with
//! the checks of wave 18, over one response (headers, TLS details, a 100 KB JSON body with three
//! tokens, one of them signed with a guessable secret) in under 2 ms (`perf/budgets.toml`).
//! `scripts/perf/security.mjs` runs this in a release build with `RT_PERF_OUT=<dir>` and reads
//! `<dir>/security.json`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use rt_app::inspect::{self, ResponseView};
use rt_core::transport::TlsInfo;
use web_time::Instant;

const ITERATIONS: usize = 200;
const RUNS: usize = 9;

fn token(secret: &str, sub: &str) -> String {
    let part = |v: serde_json::Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let input = format!(
        "{}.{}",
        part(serde_json::json!({"alg": "HS256"})),
        part(serde_json::json!({"sub": sub}))
    );
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(input.as_bytes());
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

#[test]
fn security_passive_of_one_response_is_measured() {
    let weak = token("changeme", "a");
    let strong = token("q7Vn3-very-long-and-random-shared-secret-9fK2xLw0", "b");
    let items: Vec<String> = (0..2200)
        .map(|i| format!("{{\"id\":{i},\"name\":\"item {i}\",\"tags\":[\"a\",\"b\"]}}"))
        .collect();
    let body = format!(
        "{{\"access_token\":\"{weak}\",\"id_token\":\"{strong}\",\"refresh_token\":\"{}\",\"items\":[{}]}}",
        token("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", "c"),
        items.join(",")
    );
    assert!(body.len() > 90_000, "{}", body.len());
    let headers: Vec<(String, String)> = [
        ("content-type", "application/json"),
        ("strict-transport-security", "max-age=31536000"),
        ("x-content-type-options", "nosniff"),
        ("cache-control", "no-store"),
        ("server", "nginx"),
        ("set-cookie", "sid=abc; Secure; HttpOnly; SameSite=Lax"),
        ("access-control-allow-origin", "https://app.example.com"),
    ]
    .iter()
    .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
    .collect();
    let sent = vec![
        ("Origin".to_owned(), "https://app.example.com".to_owned()),
        ("Authorization".to_owned(), format!("Bearer {strong}")),
    ];
    let tls = TlsInfo {
        protocol: Some("TLSv1.3".into()),
        not_after: Some(rt_core::jwt::now_secs() + 200 * 86_400),
        key_algorithm: Some("EC".into()),
        key_bits: Some(256),
        ..TlsInfo::default()
    };
    let view = ResponseView::new(
        "https://api.example.com/items",
        200,
        &sent,
        &headers,
        body.as_bytes(),
        body.len() as u64,
    )
    .with_tls(Some(&tls));

    let mut runs = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let mut found = 0;
        for _ in 0..ITERATIONS {
            found += std::hint::black_box(inspect::check_all(&view, &[])).len();
        }
        runs.push(start.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64);
        assert!(found > 0, "the weak token is found");
    }
    runs.sort_by(f64::total_cmp);
    let median = runs[RUNS / 2];
    println!("security_passive: median {median:.3} ms per response over {RUNS} runs");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "security_passive": median, "runs": runs });
        std::fs::write(
            std::path::Path::new(&dir).join("security.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
