// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand I: the budget `catalogue_index`: reading and searching an index with 1 000
//! entries (release build, median of 9, under 20 ms); the SHA-256 of a 10 MiB module is reported
//! (held at 250 ms). `node scripts/perf/run.mjs --only catalogue` runs `catalogue_index_is_measured`
//! in release mode with `RT_PERF_OUT=<dir>` and reads `<dir>/catalogue.json`.

use std::time::Instant;

use rt_app::catalogue::{MAX_ENTRIES, matches, parse_index, sha256_hex};

fn index() -> Vec<u8> {
    let plugins: Vec<_> = (0..MAX_ENTRIES)
        .map(|i| {
            serde_json::json!({
                "id": format!("plugin-{i}"),
                "name": format!("Plugin number {i}"),
                "version": "1.2.3",
                "api": 1,
                "description": "A synthetic plugin that does something useful with requests and responses.",
                "license": "Apache-2.0",
                "homepage": "https://example.test/plugins",
                "wasm": {
                    "url": format!("https://example.test/plugins/plugin-{i}.wasm"),
                    "sha256": format!("{:064x}", i),
                    "size": 1_000_000 + i,
                },
                "permissions": { "network": ["api.example.test", "*.example.test"], "storage": true, "variables": true },
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({ "version": 1, "name": "Perf", "plugins": plugins }))
        .unwrap()
}

fn median_ms(mut runs: Vec<f64>) -> f64 {
    runs.sort_by(f64::total_cmp);
    runs[runs.len() / 2]
}

#[test]
fn catalogue_index_is_measured() {
    let bytes = index();
    assert!(bytes.len() < 1024 * 1024, "the index fits the 1 MiB limit");
    let mut runs = Vec::new();
    for _ in 0..9 {
        let started = Instant::now();
        let parsed = parse_index(&bytes).unwrap();
        let hits = parsed
            .plugins
            .iter()
            .filter(|e| matches(e, "number 99"))
            .count();
        runs.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(hits >= 10, "the search works ({hits})");
    }
    let catalogue_index = median_ms(runs.clone());

    let module = vec![0x5au8; 10 * 1024 * 1024];
    let mut sums = Vec::new();
    for _ in 0..9 {
        let started = Instant::now();
        let sum = sha256_hex(&module);
        sums.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(sum.len(), 64);
    }
    let sha = median_ms(sums);
    println!(
        "catalogue_index: {catalogue_index:.2} ms for 1000 entries; sha256 of 10 MiB {sha:.1} ms"
    );
    if !cfg!(debug_assertions) {
        assert!(
            catalogue_index <= 20.0,
            "index took {catalogue_index:.1} ms"
        );
        // Software SHA-256 (sha2 without the asm feature, which would not build everywhere):
        // about 100 ms for 10 MiB here; modules are far smaller. Reported, held at 250 ms.
        assert!(sha <= 250.0, "sha256 of 10 MiB took {sha:.1} ms");
    }
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let out = serde_json::json!({
            "catalogue_index": catalogue_index,
            "runs": runs,
            "sha256_10_mib_ms": sha,
            "bytes": bytes.len(),
        });
        std::fs::write(
            std::path::Path::new(&dir).join("catalogue.json"),
            serde_json::to_string_pretty(&out).unwrap(),
        )
        .unwrap();
    }
}
