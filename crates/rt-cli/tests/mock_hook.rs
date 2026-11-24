// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18: `rt mock --hook`. The builds of this repository register no hook, so a name is an
//! error; with a registered hook (a distribution, here a test) it decides per request.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rt_cli::mock::{Answer, HookAction, Mock, MockHook, MockHookEntry, MockRequest, serve};

struct Fixed(HookAction, AtomicUsize);

impl MockHook for Fixed {
    fn on_request(&self, _: &MockRequest<'_>) -> HookAction {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.clone()
    }
}

fn empty_mock() -> Mock {
    let shop = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/shop");
    rt_cli::mock::load(&rt_cli::mock::Args {
        collection: shop,
        ..Default::default()
    })
    .expect("mock")
}

async fn start(hook: Arc<dyn MockHook>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(
        listener,
        Arc::new(empty_mock()),
        Arc::new(vec![hook]),
    ));
    addr
}

fn get(addr: std::net::SocketAddr) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"GET /no-such-route HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn an_unknown_hook_is_an_error() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = rt_cli::run(["mock", "--hook", "faults=x.json", "."], &mut out, &mut err);
    assert_eq!(code, rt_cli::EXIT_USAGE);
    assert!(String::from_utf8_lossy(&err).contains("unknown hook"));
}

#[test]
fn a_hook_can_answer_delay_and_drop() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let respond = Arc::new(Fixed(
            HookAction::Respond(Answer {
                status: 503,
                reason: None,
                headers: Vec::new(),
                body: b"down".to_vec(),
                matched: None,
            }),
            AtomicUsize::new(0),
        ));
        let addr = start(respond.clone()).await;
        let text = tokio::task::spawn_blocking(move || get(addr))
            .await
            .unwrap();
        assert!(text.starts_with("HTTP/1.1 503"), "{text}");
        assert!(text.ends_with("down"));
        assert_eq!(respond.1.load(Ordering::SeqCst), 1);

        let delay = Arc::new(Fixed(
            HookAction::Delay(Duration::from_millis(300)),
            AtomicUsize::new(0),
        ));
        let addr = start(delay).await;
        let t = Instant::now();
        let text = tokio::task::spawn_blocking(move || get(addr))
            .await
            .unwrap();
        assert!(t.elapsed() >= Duration::from_millis(300));
        assert!(text.starts_with("HTTP/1.1 404"), "{text}");

        let abort = Arc::new(Fixed(
            HookAction::Abort(rt_cli::mock::AbortKind::Reset),
            AtomicUsize::new(0),
        ));
        let addr = start(abort).await;
        let text = tokio::task::spawn_blocking(move || get(addr))
            .await
            .unwrap();
        assert!(text.is_empty(), "{text}");
    });
}

#[test]
fn registered_hooks_are_listed_in_help() {
    let entry = MockHookEntry {
        name: "faults",
        summary: "inject faults",
        create: |_| Err("nope".into()),
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = rt_cli::mock::run_with(
        rt_cli::mock::Args {
            help: true,
            ..Default::default()
        },
        &[entry],
        &mut out,
        &mut err,
    );
    assert_eq!(code, rt_cli::EXIT_OK);
    assert!(String::from_utf8_lossy(&out).contains("faults"));
}
