// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests of the plugin catalogue without a server (wave 18, strand I): the index and
//! its limits, sources the user added (nothing is requested before an action, no built-in
//! index), Git sources through the host's raw-file address and, when that fails, a clone, the
//! SHA-256 before an install (a mismatch refuses), a changed release with the same version,
//! the rights read from the module's own manifest (a difference warns), an update with more
//! rights, and the state kept in the plugin folder. The wire is a fake transport that records
//! every request.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context as TaskContext, Poll};

use bytes::Bytes;
use rt_app::catalogue::disk::DiskCatalogue;
use rt_app::catalogue::{
    CatalogueEntry, CatalogueWasm, INDEX_VERSION, InstalledState, MAX_ENTRIES, check_updates,
    fetch_index, fetch_wasm, locate, matches, parse_index, plan_install, resolve_wasm_url,
    sha256_hex, verify_installed,
};
use rt_app::plugins::Permissions;
use rt_core::transport::{
    BodyStream, HttpRequest, HttpVersion, Response, ResponseHead, Timing, Transport, TransportError,
};
use rt_plugin::host::dir::PluginDir;
use serde_json::json;

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = TaskContext::from_waker(std::task::Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

struct Once(Option<Bytes>);

impl futures_core::Stream for Once {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

/// The fake host: answers the addresses it was given, 404 for everything else, and remembers
/// what was asked.
#[derive(Default)]
struct Wire {
    files: Mutex<Vec<(String, Vec<u8>)>>,
    asked: Mutex<Vec<String>>,
}

impl Wire {
    fn serve(&self, url: &str, body: impl Into<Vec<u8>>) {
        let mut files = self.files.lock().unwrap();
        files.retain(|(u, _)| u != url);
        files.push((url.to_owned(), body.into()));
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Transport for Wire {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        self.asked.lock().unwrap().push(request.url.clone());
        let body = self
            .files
            .lock()
            .unwrap()
            .iter()
            .find(|(u, _)| *u == request.url)
            .map(|(_, b)| b.clone());
        let (status, body) = match body {
            Some(b) => (200, b),
            None => (404, b"not found".to_vec()),
        };
        let stream: BodyStream = Box::pin(Once(Some(Bytes::from(body))));
        Ok(Response {
            head: ResponseHead {
                status,
                version: HttpVersion::Http11,
                headers: Vec::new(),
                url: request.url,
                redirects: Vec::new(),
                timing: Timing::default(),
            },
            body: stream,
        })
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rt-plugin/tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// The same plugin and version with different bytes: a valid trailing custom section.
fn republished(mut wasm: Vec<u8>) -> Vec<u8> {
    wasm.extend_from_slice(&[0x00, 0x02, 0x01, b'x']);
    wasm
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-app-catalogue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn entry_json(
    id: &str,
    version: &str,
    wasm: &[u8],
    url: &str,
    permissions: serde_json::Value,
) -> serde_json::Value {
    json!({
        "id": id, "name": format!("{id} plugin"), "version": version, "api": 1,
        "description": "A synthetic plugin for tests", "license": "Apache-2.0",
        "wasm": { "url": url, "sha256": sha256_hex(wasm), "size": wasm.len() },
        "permissions": permissions,
    })
}

fn index_bytes(entries: Vec<serde_json::Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "version": 1, "name": "Test index", "plugins": entries })).unwrap()
}

fn entry(id: &str, version: &str, wasm: &[u8], permissions: Permissions) -> CatalogueEntry {
    CatalogueEntry {
        id: id.into(),
        name: id.into(),
        version: version.into(),
        api: 1,
        description: String::new(),
        license: None,
        homepage: None,
        wasm: CatalogueWasm {
            url: "x.wasm".into(),
            sha256: sha256_hex(wasm),
            size: wasm.len() as u64,
        },
        permissions,
    }
}

const INDEX: &str = "https://plugins.example.test/catalogue/plugins.json";

// ---- the index ----

#[test]
fn a_valid_index_is_read_and_searched() {
    let wasm = fixture("demo.wasm");
    let bytes = index_bytes(vec![entry_json(
        "demo",
        "1.0.0",
        &wasm,
        "demo.wasm",
        json!({"storage": true}),
    )]);
    let index = parse_index(&bytes).unwrap();
    assert_eq!(index.version, INDEX_VERSION);
    assert_eq!(index.name, "Test index");
    assert_eq!(index.plugins[0].id, "demo");
    assert!(matches(&index.plugins[0], "DEMO synthetic"));
    assert!(!matches(&index.plugins[0], "odata"));
    assert_eq!(
        resolve_wasm_url(INDEX, &index.plugins[0]).unwrap(),
        "https://plugins.example.test/catalogue/demo.wasm",
        "relative to the index"
    );
}

#[test]
fn the_limits_of_an_index_hold() {
    let wasm = fixture("demo.wasm");
    let one = entry_json("demo", "1.0.0", &wasm, "demo.wasm", json!({}));
    // Too many entries.
    let many: Vec<_> = (0..=MAX_ENTRIES)
        .map(|i| entry_json(&format!("p{i}"), "1.0.0", &wasm, "a.wasm", json!({})))
        .collect();
    assert!(
        parse_index(&index_bytes(many))
            .unwrap_err()
            .to_string()
            .contains("at most 1000")
    );
    // Exactly the limit is fine.
    let ok: Vec<_> = (0..MAX_ENTRIES)
        .map(|i| entry_json(&format!("p{i}"), "1.0.0", &wasm, "a.wasm", json!({})))
        .collect();
    assert_eq!(
        parse_index(&index_bytes(ok)).unwrap().plugins.len(),
        MAX_ENTRIES
    );
    // Too large a file.
    let mut big = index_bytes(vec![one.clone()]);
    big.resize(1024 * 1024 + 1, b' ');
    assert!(
        parse_index(&big)
            .unwrap_err()
            .to_string()
            .contains("larger than 1 MiB")
    );
    // A module over 32 MiB.
    let mut huge = one.clone();
    huge["wasm"]["size"] = json!(33 * 1024 * 1024);
    assert!(
        parse_index(&index_bytes(vec![huge]))
            .unwrap_err()
            .to_string()
            .contains("32 MiB")
    );
    // Wrong version, bad checksum, bad or doubled ids.
    let wrong = serde_json::to_vec(&json!({"version": 2, "name": "x", "plugins": []})).unwrap();
    assert!(parse_index(&wrong).is_err());
    let mut bad = one.clone();
    bad["wasm"]["sha256"] = json!("abc");
    assert!(parse_index(&index_bytes(vec![bad])).is_err());
    let mut id = one.clone();
    id["id"] = json!("../evil");
    assert!(parse_index(&index_bytes(vec![id])).is_err());
    assert!(
        parse_index(&index_bytes(vec![one.clone(), one]))
            .unwrap_err()
            .to_string()
            .contains("twice")
    );
    assert!(parse_index(b"<html>").is_err());
}

// ---- sources: nothing without an action ----

#[test]
fn adding_a_source_asks_nobody_and_there_is_no_built_in_index() {
    let data = temp("sources");
    let disk = DiskCatalogue::new(PluginDir::in_data_dir(&data));
    assert!(disk.sources().is_empty(), "no built-in index");
    let added = disk
        .add_source("https://plugins.example.test/index/plugins.json")
        .unwrap();
    assert_eq!(added.commit, None);
    disk.add_source("https://plugins.example.test/index/plugins.json")
        .unwrap();
    assert_eq!(disk.sources().len(), 1, "twice is once");
    // Another instance reads the same file.
    let again = DiskCatalogue::new(PluginDir::in_data_dir(&data));
    assert_eq!(again.sources(), disk.sources());
    disk.remove_source("https://plugins.example.test/index/plugins.json")
        .unwrap();
    assert!(disk.sources().is_empty());
    assert!(
        disk.remove_source("https://nowhere.test/plugins.json")
            .is_err()
    );
    // What a source may be.
    for bad in [
        "http://plugins.example.test/plugins.json",
        "ftp://x.test/plugins.json",
        "https://user:pw@x.test/plugins.json",
        "not a url",
        "https://github.com/onlyone",
    ] {
        assert!(disk.add_source(bad).is_err(), "{bad} must be refused");
    }
    assert!(
        disk.add_source("http://127.0.0.1:48629/plugins.json")
            .is_ok(),
        "loopback may use http"
    );
    assert!(locate("https://github.com/org/repo").is_ok());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_git_source_is_read_through_the_hosts_raw_address() {
    let github = locate("https://github.com/org/repo#v1.2").unwrap();
    assert_eq!(
        github.index_urls,
        ["https://raw.githubusercontent.com/org/repo/v1.2/plugins.json"]
    );
    let gitlab = locate("https://gitlab.com/org/repo").unwrap();
    assert_eq!(
        gitlab.index_urls,
        ["https://gitlab.com/org/repo/-/raw/HEAD/plugins.json"]
    );
    let sha = locate("https://codeberg.org/org/repo.git#3fa1c9e5d2").unwrap();
    assert_eq!(sha.commit.as_deref(), Some("3fa1c9e"));
    assert_eq!(
        sha.index_urls[0],
        "https://codeberg.org/org/repo/raw/commit/3fa1c9e5d2/plugins.json"
    );
    // A link to the file is the file.
    let direct = locate("https://x.test/a/plugins.json").unwrap();
    assert!(direct.git.is_none());
    assert_eq!(direct.index_urls, ["https://x.test/a/plugins.json"]);
}

#[test]
fn a_git_source_is_cloned_when_no_raw_address_answers() {
    let wasm = fixture("demo.wasm");
    let bytes = index_bytes(vec![entry_json(
        "demo",
        "1.0.0",
        &wasm,
        "https://m.test/demo.wasm",
        json!({}),
    )]);
    let wire = Wire::default();
    // The raw address answers: no clone.
    wire.serve(
        "https://codeberg.org/org/repo/raw/branch/main/plugins.json",
        bytes.clone(),
    );
    let calls = Mutex::new(0);
    let clone = |_: &rt_app::catalogue::GitSource| {
        *calls.lock().unwrap() += 1;
        Err::<(Vec<u8>, String), String>("not used".into())
    };
    let got = block_on(fetch_index(
        &wire,
        "https://codeberg.org/org/repo",
        Some(&clone),
    ))
    .unwrap();
    assert_eq!(got.index.index.plugins.len(), 1);
    assert_eq!(*calls.lock().unwrap(), 0);

    // Nothing answers: the clone is used, and its commit is shown.
    let wire = Wire::default();
    let clone = |g: &rt_app::catalogue::GitSource| {
        assert_eq!(g.url, "https://git.example.test/org/repo");
        Ok::<_, String>((
            bytes.clone(),
            "3fa1c9e0000000000000000000000000000000aa".to_owned(),
        ))
    };
    let got = block_on(fetch_index(
        &wire,
        "https://git.example.test/org/repo",
        Some(&clone),
    ))
    .unwrap();
    assert_eq!(got.index.source.commit.as_deref(), Some("3fa1c9e"));
    assert_eq!(got.index.source.name.as_deref(), Some("Test index"));
    // Without a clone (the browser) the failure says which addresses were tried.
    let e = block_on(fetch_index(
        &wire,
        "https://git.example.test/org/repo",
        None,
    ))
    .unwrap_err();
    assert!(e.to_string().contains("HTTP 404"), "{e}");
}

#[test]
fn nothing_is_requested_until_asked() {
    let wire = Wire::default();
    let data = temp("quiet");
    let disk = DiskCatalogue::new(PluginDir::in_data_dir(&data));
    disk.add_source(INDEX).unwrap();
    let _ = (
        disk.sources(),
        disk.installed_sources(),
        locate(INDEX).unwrap(),
    );
    assert!(
        wire.asked().is_empty(),
        "adding a source, listing, locating: no request"
    );
    wire.serve(INDEX, index_bytes(vec![]));
    block_on(fetch_index(&wire, INDEX, None)).unwrap();
    assert_eq!(wire.asked(), [INDEX], "one request, for the refresh");
    let _ = std::fs::remove_dir_all(&data);
}

// ---- installing ----

#[test]
fn the_checksum_is_verified_and_a_mismatch_refuses() {
    let wasm = fixture("demo.wasm");
    let good = entry(
        "demo",
        "1.0.0",
        &wasm,
        Permissions {
            storage: true,
            ..Default::default()
        },
    );
    let plan = plan_install(INDEX, "https://m.test/demo.wasm", &good, &wasm, None).unwrap();
    assert_eq!(plan.sha256, sha256_hex(&wasm));
    assert!(!plan.update && !plan.more_permissions && !plan.permissions_differ);
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);

    let tampered = republished(wasm.clone());
    let e = plan_install(INDEX, "u", &good, &tampered, None)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("checksum does not match") && e.contains("Nothing was installed"),
        "{e}"
    );
    // A check on the bytes before anything is read from them: garbage fails on the sum first.
    assert!(
        plan_install(INDEX, "u", &good, b"<html>oops</html>", None)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
}

#[test]
fn rights_come_from_the_modules_manifest_and_a_difference_warns() {
    let wasm = fixture("demo.wasm"); // asks for storage only
    // The index claims network access and no storage.
    let claimed = Permissions {
        network: vec!["*".into()],
        ..Default::default()
    };
    let plan = plan_install(
        INDEX,
        "u",
        &entry("demo", "1.0.0", &wasm, claimed),
        &wasm,
        None,
    )
    .unwrap();
    assert_eq!(
        plan.permissions,
        Permissions {
            storage: true,
            ..Default::default()
        }
    );
    assert!(plan.permissions_differ);
    assert!(
        plan.warnings.iter().any(|w| w.contains("other rights")),
        "{:?}",
        plan.warnings
    );
    // Another version in the index than in the module warns too.
    let plan = plan_install(
        INDEX,
        "u",
        &entry("demo", "9.9.9", &wasm, plan.permissions),
        &wasm,
        None,
    )
    .unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.contains("version 1.0.0")),
        "{:?}",
        plan.warnings
    );
    // The id must be the module's.
    let e = plan_install(
        INDEX,
        "u",
        &entry("other", "1.0.0", &wasm, Permissions::default()),
        &wasm,
        None,
    );
    assert!(
        e.unwrap_err()
            .to_string()
            .contains("the index says \"other\"")
    );
}

#[test]
fn a_changed_release_with_the_same_version_is_refused() {
    let wasm = fixture("demo.wasm");
    let republished = republished(wasm.clone());
    let installed = InstalledState {
        version: "1.0.0".into(),
        granted: Permissions {
            storage: true,
            ..Default::default()
        },
        sha256: Some(sha256_hex(&wasm)),
    };
    // The index now lists other bytes under the same version.
    let changed = entry(
        "demo",
        "1.0.0",
        &republished,
        Permissions {
            storage: true,
            ..Default::default()
        },
    );
    let e = plan_install(INDEX, "u", &changed, &republished, Some(&installed))
        .unwrap_err()
        .to_string();
    assert!(e.contains("the index changed a published release"), "{e}");
    // The same bytes are no change; and a plugin installed from a file has no sum to compare.
    let same = entry(
        "demo",
        "1.0.0",
        &wasm,
        Permissions {
            storage: true,
            ..Default::default()
        },
    );
    assert!(plan_install(INDEX, "u", &same, &wasm, Some(&installed)).is_ok());
    let from_file = InstalledState {
        sha256: None,
        ..installed
    };
    assert!(plan_install(INDEX, "u", &changed, &republished, Some(&from_file)).is_ok());
}

#[test]
fn an_update_with_more_rights_asks_again() {
    let v1 = fixture("demo.wasm");
    let v2 = fixture("demo-v2.wasm"); // adds network access
    let installed = InstalledState {
        version: "1.0.0".into(),
        granted: Permissions {
            storage: true,
            ..Default::default()
        },
        sha256: Some(sha256_hex(&v1)),
    };
    let rights = Permissions {
        network: vec!["{request}".into()],
        storage: true,
        variables: false,
    };
    let plan = plan_install(
        INDEX,
        "u",
        &entry("demo", "2.0.0", &v2, rights.clone()),
        &v2,
        Some(&installed),
    )
    .unwrap();
    assert!(plan.update && plan.more_permissions);
    assert_eq!(plan.permissions, rights);
    // The same rights again: an update without a question.
    let same = InstalledState {
        granted: rights.clone(),
        ..installed.clone()
    };
    let plan = plan_install(
        INDEX,
        "u",
        &entry("demo", "2.0.0", &v2, rights),
        &v2,
        Some(&same),
    )
    .unwrap();
    assert!(plan.update && !plan.more_permissions);
    // An older module than the installed one warns.
    let newer = InstalledState {
        version: "3.0.0".into(),
        ..installed
    };
    let plan = plan_install(
        INDEX,
        "u",
        &entry(
            "demo",
            "1.0.0",
            &v1,
            Permissions {
                storage: true,
                ..Default::default()
            },
        ),
        &v1,
        Some(&newer),
    )
    .unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("older")));
}

#[test]
fn an_install_records_where_it_came_from_and_updates_are_found_on_request() {
    let data = temp("install");
    let disk = DiskCatalogue::new(PluginDir::in_data_dir(&data));
    let v1 = fixture("demo.wasm");
    let wire = Wire::default();
    let m1 = "https://plugins.example.test/catalogue/demo.wasm";
    wire.serve(
        INDEX,
        index_bytes(vec![entry_json(
            "demo",
            "1.0.0",
            &v1,
            "demo.wasm",
            json!({"storage": true}),
        )]),
    );
    wire.serve(m1, v1.clone());
    disk.add_source(INDEX).unwrap();

    let fetched = block_on(fetch_index(&wire, INDEX, None)).unwrap();
    let entry = &fetched.index.index.plugins[0];
    let url = resolve_wasm_url(&fetched.index_url, entry).unwrap();
    let bytes = block_on(fetch_wasm(&wire, &url)).unwrap();
    let plan = plan_install(
        INDEX,
        &url,
        entry,
        &bytes,
        disk.installed_state("demo").as_ref(),
    )
    .unwrap();
    disk.install(&bytes, INDEX, &plan.sha256).unwrap();

    let installed = disk.installed_sources();
    assert_eq!(installed.len(), 1);
    assert_eq!(
        (installed[0].plugin.as_str(), installed[0].version.as_str()),
        ("demo", "1.0.0")
    );
    assert_eq!(installed[0].sha256, sha256_hex(&v1));
    assert!(verify_installed(
        &installed[0].sha256,
        &disk.dir().module_bytes("demo").unwrap()
    ));
    assert!(!verify_installed(&installed[0].sha256, b"changed on disk"));
    assert_eq!(disk.dir().load().0.len(), 1, "the plugin loads");

    // The index moves on: the update is found only when asked, from the refreshed index.
    let v2 = fixture("demo-v2.wasm");
    wire.serve(
        INDEX,
        index_bytes(vec![entry_json(
            "demo",
            "2.0.0",
            &v2,
            "demo.wasm",
            json!({}),
        )]),
    );
    let fresh = block_on(fetch_index(&wire, INDEX, None)).unwrap();
    let updates = check_updates(&[fresh.index], &installed);
    assert_eq!(updates.len(), 1);
    assert_eq!(
        (updates[0].installed.as_str(), updates[0].available.as_str()),
        ("1.0.0", "2.0.0")
    );
    assert!(check_updates(&[], &installed).is_empty());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_module_only_comes_from_a_web_address_of_the_allowed_kind() {
    let wire = Wire::default();
    for url in [
        "http://evil.test/a.wasm",
        "file:///etc/passwd",
        "ftp://x/a.wasm",
    ] {
        assert!(block_on(fetch_wasm(&wire, url)).is_err(), "{url}");
    }
    assert!(wire.asked().is_empty(), "refused before any request");
    let wasm = fixture("demo.wasm");
    let mut e = entry("demo", "1.0.0", &wasm, Permissions::default());
    e.wasm.url = "http://evil.test/demo.wasm".into();
    assert!(
        resolve_wasm_url(INDEX, &e).is_err(),
        "an https index cannot point at plain http"
    );
}
