// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection tree: collection, folders, requests, environments.
//! Format-neutral: only `rt-format` knows how this maps to files.

use serde::{Deserialize, Serialize};

use super::auth::CollectionConfig;
use super::env::{Environment, Variable};
use super::request::{Auth, KeyValue, RequestSpec, Settings};

/// Defaults that a collection or folder passes down to everything below it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RequestDefaults {
    pub headers: Vec<KeyValue>,
    /// `Auth::Inherit` on a folder defers to its parent; on the collection it means no auth.
    pub auth: Auth,
    pub vars: Vec<Variable>,
    pub settings: Settings,
    /// Scripts of the collection or folder (wave 7). They run for every request below,
    /// outermost first, before the request's own scripts of the same phase.
    #[serde(default)]
    pub scripts: Vec<super::Script>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    pub seq: Option<f64>,
    /// Path of the folder relative to the collection root, `/`-separated, e.g. `users/admin`.
    pub path: String,
    pub defaults: RequestDefaults,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEntry {
    /// Path of the request file relative to the collection root, `/`-separated,
    /// e.g. `users/get-user.yml`. This is the request's identity.
    pub path: String,
    pub spec: RequestSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    Folder(Folder),
    Request(RequestEntry),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub defaults: RequestDefaults,
    pub items: Vec<Item>,
    pub environments: Vec<Environment>,
    /// OpenCollection `config` (wave 5): client certificates.
    #[serde(default)]
    pub config: CollectionConfig,
}

/// A request together with the chain of defaults above it, outermost first.
/// This is what variable resolution and auth inheritance work on.
#[derive(Debug, Clone)]
pub struct RequestContext<'a> {
    pub collection: &'a Collection,
    /// Folders from the collection root down to the request's parent.
    pub folders: Vec<&'a Folder>,
    pub request: &'a RequestEntry,
}

impl Collection {
    /// Finds a request by its path relative to the collection root.
    pub fn request(&self, path: &str) -> Option<RequestContext<'_>> {
        fn walk<'a>(
            items: &'a [Item],
            path: &str,
            chain: &mut Vec<&'a Folder>,
        ) -> Option<&'a RequestEntry> {
            for item in items {
                match item {
                    Item::Request(r) if r.path == path => return Some(r),
                    Item::Folder(f) => {
                        chain.push(f);
                        if let Some(r) = walk(&f.items, path, chain) {
                            return Some(r);
                        }
                        chain.pop();
                    }
                    Item::Request(_) => {}
                }
            }
            None
        }
        let mut folders = Vec::new();
        let request = walk(&self.items, path, &mut folders)?;
        Some(RequestContext {
            collection: self,
            folders,
            request,
        })
    }

    pub fn environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }
}
