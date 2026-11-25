// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The downloads of the catalogue over the app's `Transport`. Nothing here runs by itself:
//! every function is called for a user action (refresh, install, `rt plugin`).

use std::time::Duration;

use rt_core::transport::{HttpRequest, SendOptions, Transport, collect};

use super::index::{GitSource, Located, locate, parse_index, short};
use super::{CatalogueSource, MAX_INDEX_BYTES, MAX_WASM_BYTES, SourceIndex};
use crate::AppError;

/// Reads a Git source by cloning it (native shells only): the index bytes and the commit.
pub type CloneIndex<'a> = &'a dyn Fn(&GitSource) -> Result<(Vec<u8>, String), String>;

/// A refreshed source and the address its index was read from (relative module addresses are
/// resolved against it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub index: SourceIndex,
    pub index_url: String,
}

fn err(message: impl Into<String>) -> AppError {
    AppError::Plugin(message.into())
}

async fn get<T: Transport>(transport: &T, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let request = HttpRequest {
        method: "GET".into(),
        url: url.to_owned(),
        headers: vec![("Accept".into(), "application/json, */*;q=0.5".into())],
        body: None,
        options: SendOptions {
            timeout: Some(Duration::from_secs(60)),
            ..SendOptions::default()
        },
    };
    let started = web_time::Instant::now();
    let response = transport.send(request).await.map_err(|e| e.to_string())?;
    let done = collect(response, limit, started)
        .await
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&done.head.status) {
        return Err(format!("HTTP {}", done.head.status));
    }
    if done.truncated {
        return Err(format!("larger than {} MiB", limit / (1024 * 1024)));
    }
    Ok(done.body.to_vec())
}

/// Downloads and reads the index of `source`. A Git source is read through the host's raw-file
/// address; if none answers and `clone` is given, the repository is cloned instead.
pub async fn fetch_index<T: Transport>(
    transport: &T,
    source: &str,
    clone: Option<CloneIndex<'_>>,
) -> Result<Fetched, AppError> {
    let located: Located = locate(source)?;
    let mut failures = Vec::new();
    let mut found = None;
    for url in &located.index_urls {
        match get(transport, url, MAX_INDEX_BYTES).await {
            Ok(bytes) => {
                found = Some((bytes, url.clone(), located.commit.clone()));
                break;
            }
            Err(e) => failures.push(format!("{url}: {e}")),
        }
    }
    if found.is_none()
        && let (Some(clone), Some(git)) = (clone, &located.git)
    {
        match clone(git) {
            Ok((bytes, commit)) => {
                let url = located.index_urls.first().cloned().unwrap_or_default();
                found = Some((bytes, url, Some(short(&commit))));
            }
            Err(e) => failures.push(format!("clone {}: {e}", git.url)),
        }
    }
    let Some((bytes, index_url, commit)) = found else {
        return Err(err(format!(
            "cannot read the index of {source}: {}",
            failures.join("; ")
        )));
    };
    let index = parse_index(&bytes)?;
    Ok(Fetched {
        index: SourceIndex {
            source: CatalogueSource {
                url: source.trim().to_owned(),
                commit,
                name: Some(index.name.clone()).filter(|n| !n.is_empty()),
            },
            index,
        },
        index_url,
    })
}

/// Downloads a module (at most [`MAX_WASM_BYTES`]).
pub async fn fetch_wasm<T: Transport>(transport: &T, url: &str) -> Result<Vec<u8>, AppError> {
    let parsed = url::Url::parse(url).map_err(|_| err(format!("{url:?} is not a web address")))?;
    if !super::index::allowed_url(&parsed) {
        return Err(err(format!("{url}: modules come from https:// addresses")));
    }
    get(transport, url, MAX_WASM_BYTES)
        .await
        .map_err(|e| err(format!("cannot download {url}: {e}")))
}
