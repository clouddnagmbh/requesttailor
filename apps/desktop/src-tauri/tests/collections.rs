// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 14, strand A: several collections in the desktop core. A root with three collections
//! (one nested, a `node_modules` decoy), adding and removing roots, the active collection,
//! tabs of two collections with the same path, saving all, search over all, the runner keeping
//! its collection, secrets keyed the same whether a collection is opened alone or through its
//! parent folder, new collections, and the set remembered across starts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rt_app::collections::NewCollection;
use rt_app::{AppError, SecretKey, SecretStore, SendInput, TreeNode};
use rt_desktop::Core;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
        self.0.lock().unwrap().insert(key.clone(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-collections-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(&dir).unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A collection in `dir` named `name` with a request `ping.yml` (same path in every
/// collection) and an environment `Local` with a secret `token`.
fn collection(dir: &Path, name: &str, base_url: &str) {
    write(
        &dir.join("opencollection.yml"),
        &format!("opencollection: \"1.0.0\"\ninfo:\n  name: {name}\n"),
    );
    write(
        &dir.join("ping.yml"),
        &format!(
            "info:\n  name: Ping {name}\n  type: http\n  seq: 1\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/echo?from={name}\"\n"
        ),
    );
    write(
        &dir.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: baseUrl\n    value: {base_url}\n  - secret: true\n    name: token\n"
        ),
    );
}

/// Root with Billing, Orders and Orders' nested Legacy, plus a decoy in `node_modules`.
fn sample_root(tag: &str, base_url: &str) -> TempDir {
    let root = TempDir::new(tag);
    collection(&root.0.join("billing"), "Billing", base_url);
    collection(&root.0.join("services/orders"), "Orders", base_url);
    collection(
        &root.0.join("services/orders/legacy"),
        "Legacy orders",
        base_url,
    );
    collection(
        &root.0.join("node_modules/some-lib/fixtures"),
        "Decoy",
        base_url,
    );
    write(&root.0.join("README.md"), "several collections\n");
    root
}

fn id(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn requests(node: &TreeNode) -> Vec<String> {
    match node {
        TreeNode::Request { path, .. } => vec![path.clone()],
        TreeNode::Folder { children, .. } => children.iter().flat_map(requests).collect(),
    }
}

fn input(env: &str) -> SendInput {
    SendInput {
        environment: Some(env.into()),
        vars: Vec::new(),
        secrets: Vec::new(),
    }
}

#[test]
fn a_root_with_three_collections_shows_three_and_activates_the_first() {
    let root = sample_root("three", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    let info = core.open(&root.0).unwrap();
    let set = core.collection_set();
    assert_eq!(set.roots.len(), 1);
    assert_eq!(set.roots[0].id, id(&root.0));
    assert!(!set.roots[0].truncated);
    let names: Vec<_> = set.collections.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Billing", "Orders", "Legacy orders"]);
    let dirs: Vec<_> = set.collections.iter().map(|c| c.dir.as_str()).collect();
    assert_eq!(
        dirs,
        ["billing", "services/orders", "services/orders/legacy"]
    );
    assert!(set.collections.iter().all(|c| c.root == set.roots[0].id));
    assert_eq!(set.roots[0].collections.len(), 3);
    // The first collection is active; ids are the canonical paths of the collections.
    assert_eq!(info.id, id(&root.0.join("billing")));
    assert_eq!(set.active.as_deref(), Some(info.id.as_str()));
    assert_eq!(core.collection_id().as_deref(), Some(info.id.as_str()));
    // The nested collection is not part of the outer one.
    let orders = id(&root.0.join("services/orders"));
    assert_eq!(requests(&core.tree_of(&orders).unwrap()), ["ping.yml"]);
    // Not a Git repository.
    assert!(set.collections.iter().all(|c| c.repo.is_none()));
}

#[test]
fn trees_of_collections_that_are_not_active_load_on_demand() {
    let root = sample_root("lazy", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    assert_eq!(
        core.loaded_collections(),
        1,
        "only the active collection is read"
    );
    let legacy = id(&root.0.join("services/orders/legacy"));
    assert_eq!(requests(&core.tree_of(&legacy).unwrap()), ["ping.yml"]);
    assert_eq!(core.loaded_collections(), 2);
    assert!(core.tree_of("/not/in/the/set").is_err());
}

#[test]
fn add_collection_appends_a_root_once_and_remove_takes_it_out() {
    let root = sample_root("add", "http://127.0.0.1:9");
    let other = TempDir::new("add-other");
    collection(&other.0, "Payments", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();

    let set = core.add_root(&other.0).unwrap();
    assert_eq!(set.roots.len(), 2);
    assert_eq!(set.collections.len(), 4);
    assert_eq!(set.collections[3].name, "Payments");
    assert_eq!(set.collections[3].dir, "");
    // The added collection becomes active.
    assert_eq!(set.active.as_deref(), Some(id(&other.0).as_str()));

    // The same folder again, or a collection already in the set: nothing new.
    assert_eq!(core.add_root(&other.0).unwrap().collections.len(), 4);
    assert_eq!(
        core.add_root(&root.0.join("billing"))
            .unwrap()
            .collections
            .len(),
        4
    );

    // A root with unsaved changes stays.
    core.activate(&id(&other.0)).unwrap();
    let mut spec = core.get_request("ping.yml").unwrap();
    spec.url = "http://changed".into();
    core.update_request("ping.yml", spec).unwrap();
    assert!(matches!(
        core.remove_root(&id(&other.0)),
        Err(AppError::Shell(_))
    ));
    core.save_all().unwrap();
    let set = core.remove_root(&id(&other.0)).unwrap();
    assert_eq!(set.roots.len(), 1);
    assert_eq!(set.collections.len(), 3);
    // The active collection was removed: the first one left is active.
    assert_eq!(
        set.active.as_deref(),
        Some(id(&root.0.join("billing")).as_str())
    );
    assert_eq!(core.collection_id(), set.active);
}

#[test]
fn the_same_path_in_two_collections_keeps_two_states_and_save_all_writes_both() {
    let root = sample_root("same-path", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    let billing = id(&root.0.join("billing"));
    let orders = id(&root.0.join("services/orders"));

    let mut a = core.get_request("ping.yml").unwrap();
    a.url = "http://billing.example/ping".into();
    core.update_request("ping.yml", a).unwrap();
    core.activate(&orders).unwrap();
    let mut b = core.get_request("ping.yml").unwrap();
    assert_eq!(b.name, "Ping Orders");
    b.url = "http://orders.example/ping".into();
    core.update_request("ping.yml", b).unwrap();

    assert_eq!(core.dirty_of(&billing).unwrap(), ["ping.yml"]);
    assert_eq!(core.dirty_of(&orders).unwrap(), ["ping.yml"]);
    let set = core.collection_set();
    assert!(set.collections.iter().filter(|c| c.dirty == 1).count() == 2);

    let saved = core.save_all().unwrap();
    let ids: Vec<_> = saved.iter().map(|c| c.collection.as_str()).collect();
    assert_eq!(ids, [billing.as_str(), orders.as_str()]);
    let on_disk = |dir: &str| std::fs::read_to_string(root.0.join(dir).join("ping.yml")).unwrap();
    assert!(on_disk("billing").contains("billing.example"));
    assert!(on_disk("services/orders").contains("orders.example"));
    assert!(core.dirty_of(&billing).unwrap().is_empty());
    assert!(core.dirty_of(&orders).unwrap().is_empty());
}

#[test]
fn search_finds_in_every_collection_names_first() {
    let root = sample_root("search", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    let hits = core.search_all("ping", 50).unwrap();
    let mut collections: Vec<_> = hits.iter().map(|h| h.collection.clone()).collect();
    collections.dedup();
    assert_eq!(collections.len(), 3, "{hits:?}");
    assert!(
        hits.iter()
            .take(3)
            .all(|h| h.hit.field == rt_app::SearchField::Name)
    );
    let only_orders = core.search_all("from=Orders", 50).unwrap();
    assert_eq!(only_orders.len(), 1);
    assert_eq!(
        only_orders[0].collection,
        id(&root.0.join("services/orders"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_keeps_its_collection_while_another_becomes_active() {
    let addr = rt_testserver::spawn_local().await;
    let root = sample_root("runner", &format!("http://{addr}"));
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    let orders = id(&root.0.join("services/orders"));
    core.activate(&orders).unwrap();
    let (snapshot, run_collection) = core.run_snapshot(input("Local")).await.unwrap();
    assert_eq!(run_collection, orders);

    // The user switches to Billing while the run goes on.
    core.activate(&id(&root.0.join("billing"))).unwrap();

    let plan = rt_core::runner::RunPlan {
        requests: snapshot.plan(None),
        data: Vec::new(),
        iterations: 1,
        delay_ms: 0,
        bail: false,
    };
    let transport = rt_core::transport::native::NativeTransport::new().unwrap();
    let mut urls = Vec::new();
    let summary = snapshot
        .run(
            &plan,
            &transport,
            &rt_core::send::SendOptions {
                iteration: None,
                body_limit: 1 << 20,
            },
            &rt_core::services::Services::default(),
            &mut |event| {
                if let rt_core::runner::RunEvent::Step { step } = event {
                    urls.push(step.report.request.map(|r| r.url).unwrap_or_default());
                }
            },
            &|| false,
        )
        .await;
    assert_eq!(summary.requests, 1);
    assert_eq!(urls.len(), 1);
    assert!(urls[0].contains("from=Orders"), "{urls:?}");
    assert_eq!(
        core.collection_id().as_deref(),
        Some(id(&root.0.join("billing")).as_str())
    );
}

#[test]
fn secrets_are_the_same_alone_and_through_the_parent_folder() {
    let root = sample_root("secrets", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    let orders_dir = root.0.join("services/orders");
    let alone = core.open(&orders_dir).unwrap();
    assert_eq!(alone.id, id(&orders_dir));
    core.secret_set("Local", "token", "orders-secret").unwrap();

    core.open(&root.0).unwrap();
    assert_eq!(
        core.secret_get("Local", "token").unwrap(),
        None,
        "Billing is active"
    );
    let info = core.activate(&id(&orders_dir)).unwrap();
    assert_eq!(info.id, alone.id);
    assert_eq!(
        core.secret_get("Local", "token").unwrap().as_deref(),
        Some("orders-secret")
    );
}

#[test]
fn new_collections_are_unsaved_until_saved_and_never_overwrite() {
    let root = sample_root("create", "http://127.0.0.1:9");
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    let root_id = id(&root.0);
    let info = core
        .create_collection(&NewCollection {
            name: "Inventory".into(),
            root: Some(root_id.clone()),
            dir: "inventory".into(),
            base_url: Some("http://localhost:8080".into()),
            environment: Some("Local".into()),
        })
        .unwrap();
    assert_eq!(info.name, "Inventory");
    assert_eq!(core.collection_id().as_deref(), Some(info.id.as_str()));
    assert!(
        !root.0.join("inventory/opencollection.yml").exists(),
        "unsaved"
    );
    assert_eq!(core.environments().unwrap()[0].name, "Local");
    let set = core.collection_set();
    let entry = set.collections.iter().find(|c| c.id == info.id).unwrap();
    assert_eq!(entry.dir, "inventory");
    assert!(entry.dirty > 0);
    assert_eq!(set.roots[0].collections.len(), 4);

    core.save_all().unwrap();
    assert!(root.0.join("inventory/opencollection.yml").is_file());
    assert!(root.0.join("inventory/environments/Local.yml").is_file());
    assert_eq!(info.id, id(&root.0.join("inventory")));

    // A folder with files in it, or outside the root, is refused.
    let refused = |dir: &str| {
        core.create_collection(&NewCollection {
            name: "X".into(),
            root: Some(root_id.clone()),
            dir: dir.into(),
            base_url: None,
            environment: None,
        })
    };
    assert!(matches!(refused("billing"), Err(AppError::Shell(_))));
    assert!(matches!(refused("../escape"), Err(AppError::Shell(_))));
    assert!(matches!(refused("node_modules/x"), Err(AppError::Shell(_))));
}

#[test]
fn reload_rereads_every_collection_of_the_repository() {
    let root = sample_root("reload", "http://127.0.0.1:9");
    std::fs::create_dir_all(root.0.join(".git")).unwrap();
    let core = Core::new(MemorySecrets::default()).unwrap();
    core.open(&root.0).unwrap();
    let orders = id(&root.0.join("services/orders"));
    core.tree_of(&orders).unwrap();
    let set = core.collection_set();
    assert!(
        set.collections
            .iter()
            .all(|c| c.repo.as_deref() == Some(root.0.to_str().unwrap()))
    );

    // Files change underneath (a checkout): both collections of the repository are read again.
    write(
        &root.0.join("services/orders/second.yml"),
        "info:\n  name: Second\n  type: http\n  seq: 2\nhttp:\n  method: GET\n  url: http://x\n",
    );
    write(
        &root.0.join("billing/second.yml"),
        "info:\n  name: Second\n  type: http\n  seq: 2\nhttp:\n  method: GET\n  url: http://x\n",
    );
    core.reload().unwrap();
    assert_eq!(requests(&core.tree().unwrap()), ["ping.yml", "second.yml"]);
    assert_eq!(
        requests(&core.tree_of(&orders).unwrap()),
        ["ping.yml", "second.yml"]
    );
}

#[test]
fn the_set_is_remembered_and_restored_with_its_active_collection() {
    let data = TempDir::new("data");
    let root = sample_root("remember", "http://127.0.0.1:9");
    let other = TempDir::new("remember-other");
    collection(&other.0, "Payments", "http://127.0.0.1:9");
    {
        let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
        core.open(&root.0).unwrap();
        core.add_root(&other.0).unwrap();
        core.activate(&id(&root.0.join("services/orders"))).unwrap();
    }
    let core = Core::with_data_dir(MemorySecrets::default(), Some(data.0.clone())).unwrap();
    let recent = core.recent.list();
    assert_eq!(
        recent[0].info.id,
        id(&other.0),
        "the last folder added is the most recent"
    );
    let info = rt_desktop::recent::open_recent_in(&core, &recent[0].info.id)
        .unwrap()
        .unwrap();
    let set = core.collection_set();
    assert_eq!(set.roots.len(), 2);
    assert_eq!(set.collections.len(), 4);
    assert_eq!(info.id, id(&root.0.join("services/orders")));

    // A recent folder outside the remembered set opens alone.
    let alone = TempDir::new("remember-alone");
    collection(&alone.0, "Alone", "http://127.0.0.1:9");
    core.open(&alone.0).unwrap();
    core.open(&root.0).unwrap();
    assert_eq!(core.collection_set().roots.len(), 1);
}
