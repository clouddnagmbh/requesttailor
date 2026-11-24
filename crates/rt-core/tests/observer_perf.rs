// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `observer_overhead` (wave 17): three exchange observers that do nothing
//! add at most 1 ms to a send against `rt-testserver` (`perf/budgets.toml`).
//!
//! One sample is a pair of sends of the same request, one without observers and one with three
//! (alternating, so drift hits both alike); the overhead is the median of the differences over
//! 500 pairs. `scripts/perf/run.mjs --only observe` runs the measuring test in a release build
//! with `RT_PERF_OUT=<dir>`; it writes `<dir>/observe.json` (`{"observer_overhead": <ms>, ...}`).

use std::sync::Arc;
use std::time::Instant;

use rt_core::model::{Collection, Item, RequestEntry, RequestSpec};
use rt_core::observe::{Exchange, ExchangeObserver, ObserverSet};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{SendOptions, send_with};
use rt_core::services::Services;
use rt_core::transport::native::NativeTransport;

const PAIRS: usize = 500;

struct Nothing;

impl ExchangeObserver for Nothing {
    fn observe(&self, _: &Exchange) {}
}

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

fn collection(url: String) -> Collection {
    Collection {
        name: "Perf".into(),
        items: vec![Item::Request(RequestEntry {
            path: "get.yml".into(),
            spec: RequestSpec {
                name: "get".into(),
                method: "GET".into(),
                url,
                ..Default::default()
            },
        })],
        ..Default::default()
    }
}

struct World<'a> {
    collection: &'a Collection,
    input: &'a PrepareInput<'a>,
    options: &'a SendOptions,
    transport: &'a NativeTransport,
}

impl World<'_> {
    /// One send; the time in milliseconds.
    async fn send(&self, observers: Option<&ObserverSet>) -> f64 {
        let services = Services {
            observers,
            ..Services::none()
        };
        let started = Instant::now();
        let report = send_with(
            self.collection,
            "get.yml",
            self.input,
            &NoFiles,
            self.transport,
            self.options,
            &services,
        )
        .await;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert!(report.error.is_none(), "{:?}", report.error);
        elapsed
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn observer_overhead_is_measured() {
    let addr = rt_testserver::spawn_local().await;
    let collection = collection(format!("http://{addr}/echo/perf"));
    let transport = NativeTransport::new().unwrap();
    let observers = ObserverSet::of(vec![
        Arc::new(Nothing),
        Arc::new(Nothing),
        Arc::new(Nothing),
    ]);
    let input = PrepareInput {
        environment: None,
        globals: Vec::new(),
        runtime: Vec::new(),
    };
    let options = SendOptions::default();
    let world = World {
        collection: &collection,
        input: &input,
        options: &options,
        transport: &transport,
    };
    // Warm up connections and code paths.
    for _ in 0..20 {
        world.send(None).await;
        world.send(Some(&observers)).await;
    }
    let mut diffs = Vec::with_capacity(PAIRS);
    let (mut without, mut with) = (Vec::new(), Vec::new());
    for i in 0..PAIRS {
        let (a, b) = if i % 2 == 0 {
            let a = world.send(None).await;
            (a, world.send(Some(&observers)).await)
        } else {
            let b = world.send(Some(&observers)).await;
            (world.send(None).await, b)
        };
        diffs.push(b - a);
        without.push(a);
        with.push(b);
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let overhead = median(&mut diffs);
    println!(
        "observer_overhead: median {overhead:.3} ms over {PAIRS} pairs (send {:.2} ms without, {:.2} ms with)",
        median(&mut without),
        median(&mut with)
    );
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({
            "observer_overhead": overhead.max(0.0),
            "pairs": PAIRS,
            "send_without_ms": median(&mut without),
            "send_with_ms": median(&mut with),
        });
        std::fs::write(
            std::path::Path::new(&dir).join("observe.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
