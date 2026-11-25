// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `journal_overhead` (wave 19, strand C): what the journal adds to one writing send is
//! at most 1 ms (it copies a few fields and hands them to a background writer), and reading
//! 5000 entries takes under 50 ms. The measured test runs in a release build with
//! `RT_PERF_OUT=<dir>` and writes `<dir>/journal.json`
//! (`{"journal_overhead": <ms>, "read_5000_ms": <ms>, ...}`); `node scripts/perf/run.mjs --only
//! journal` drives it.

mod journal_support;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use journal_support::{NoFiles, TempDir, Wire, answer, block_on, collection, snapshot};
use rt_app::journal::{
    FileStorage, Journal, JournalQuery, JournalSettings, forget, journal_for, refresh,
};
use rt_core::observe::{Exchange, ExchangeObserver, ObserverSet};
use rt_core::prepare::PrepareInput;
use rt_core::send::{SendOptions, send_with};
use rt_core::services::Services;

struct Keep(Mutex<Option<Exchange>>);

impl ExchangeObserver for Keep {
    fn observe(&self, exchange: &Exchange) {
        *self.0.lock().unwrap() = Some(exchange.clone());
    }
}

fn one_exchange(name: &str) -> Exchange {
    let files = collection(name, &[("create", "POST", "{{base}}/orders?key={{token}}")]);
    let snap = snapshot(&files, "dev");
    let keep = Arc::new(Keep(Mutex::new(None)));
    let observers = ObserverSet::new().with(keep.clone());
    let wire = Wire::new(|_| {
        answer(
            201,
            &[("Location", "http://dev.test/orders/1"), ("ETag", "\"v1\"")],
            "{\"id\":1,\"name\":\"widget\"}",
        )
    });
    let input = PrepareInput {
        environment: Some("dev"),
        globals: Vec::new(),
        runtime: Vec::new(),
    };
    block_on(send_with(
        &snap.collection,
        "create.yml",
        &input,
        &NoFiles,
        &wire,
        &SendOptions::default(),
        &Services {
            observers: Some(&observers),
            ..Services::none()
        },
    ));
    keep.0.lock().unwrap().take().expect("an exchange")
}

#[test]
fn journal_overhead_is_measured() {
    let name = "journal-perf";
    let exchange = one_exchange(name);
    let dir = TempDir::new("perf");
    let journal = journal_for(name, || Arc::new(FileStorage::new(&dir.0, "/perf")));
    journal
        .set_settings(JournalSettings {
            enabled: true,
            id_path: Some("$.id".into()),
        })
        .unwrap();
    refresh();
    // 10 000 entries into the channel, each timed on its own.
    let mut times = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let started = Instant::now();
        rt_core::journal::record(&exchange, rt_core::guard::Effect::Write);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p99 = times[times.len() * 99 / 100];
    journal.flush();
    let recorded = journal.entries(&JournalQuery::default()).total;
    assert_eq!(recorded, 5_000, "the ring holds 5000 of the 10 000");
    forget(name);
    // Reading 5000 entries from the file.
    let started = Instant::now();
    let again = Journal::open(name, Arc::new(FileStorage::new(&dir.0, "/perf")));
    let read_ms = started.elapsed().as_secs_f64() * 1000.0;
    let page = again.entries(&JournalQuery {
        limit: 5_000,
        ..Default::default()
    });
    assert_eq!(page.entries.len(), 5_000);
    let release = !cfg!(debug_assertions);
    if release {
        assert!(median <= 1.0, "the journal adds {median} ms per send");
        assert!(read_ms <= 50.0, "reading 5000 entries took {read_ms} ms");
    }
    if let Some(out) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({
            "journal_overhead": median,
            "p99_ms": p99,
            "read_5000_ms": read_ms,
            "entries": 10_000,
        });
        std::fs::write(
            std::path::Path::new(&out).join("journal.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
