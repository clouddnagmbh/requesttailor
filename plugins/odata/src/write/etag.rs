// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Remembered ETags, per entity URL (plugin storage `service::storage_key("etag", url)`,
//! the URL as [`util::entity_url`] writes it, value the ETag as a JSON string). An index per
//! service root (`etag-index:<root>`) keeps at most [`CAP`] entries; the oldest go first.

use rt_plugin_sdk::api::{PluginRequest, PluginResponse};
use rt_plugin_sdk::host;
use rt_plugin_sdk::{Result, header};
use serde_json::Value;

use super::util::{self, MASK};
use crate::edm::{Model, Version};
use crate::service::{self, Target};

/// Remembered ETags per service root.
pub const CAP: usize = 500;

fn key(url: &str) -> String {
    service::storage_key("etag", &util::entity_url(url))
}

fn index_key(root: &str) -> String {
    service::storage_key("etag-index", root)
}

/// The remembered ETag of an entity URL.
pub fn lookup(url: &str) -> Result<Option<String>> {
    Ok(host::storage_get(key(url))?.and_then(|v| v.as_str().map(str::to_owned)))
}

/// Remembers `(entity url, etag)` pairs of one service root.
pub fn remember(root: &str, entries: &[(String, String)]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut index: Vec<String> = host::storage_get(index_key(root))?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    for (url, etag) in entries {
        let url = util::entity_url(url);
        host::storage_set(key(&url), Value::String(etag.clone()))?;
        index.retain(|u| *u != url);
        index.push(url);
    }
    while index.len() > CAP {
        let oldest = index.remove(0);
        host::storage_delete(key(&oldest))?;
    }
    host::storage_set(index_key(root), serde_json::to_value(index)?)
}

/// Forgets the ETag of a deleted entity.
pub fn forget(root: &str, url: &str) -> Result<()> {
    let url = util::entity_url(url);
    host::storage_delete(key(&url))?;
    if let Some(v) = host::storage_get(index_key(root))? {
        let mut index: Vec<String> = serde_json::from_value(v).unwrap_or_default();
        index.retain(|u| *u != url);
        host::storage_set(index_key(root), serde_json::to_value(index)?)?;
    }
    Ok(())
}

/// The URL of the entity a bound action is called on: `…/Set(key)/NS.Action` → `…/Set(key)`.
pub fn action_entity(target: &Target) -> Option<String> {
    let (entity, last) = target.resource.rsplit_once('/')?;
    let (_, predicate) = util::split_segment(entity.rsplit('/').next()?);
    let is_action = last.contains('.') || last.starts_with("draft");
    (predicate.is_some() && is_action && !last.contains('('))
        .then(|| format!("{}{entity}", target.service_root))
}

/// Whether the last segment addresses one entity (`Set(key)`).
pub fn is_entity(target: &Target) -> bool {
    let last = target.resource.rsplit('/').next().unwrap_or_default();
    util::split_segment(last).1.is_some()
}

fn absolute(target: &Target, location: &str) -> String {
    if location.contains("://") {
        location.to_owned()
    } else if location.starts_with('/') {
        let root = &target.service_root;
        let origin_end = root
            .find("://")
            .and_then(|i| root[i + 3..].find('/').map(|j| i + 3 + j))
            .unwrap_or(root.len());
        format!("{}{location}", &root[..origin_end])
    } else {
        format!("{}{location}", target.service_root)
    }
}

/// Literal of a key value in a V4 URL, by its EDM type.
fn key_literal(edm_type: &str, value: &Value, version: Version) -> Option<String> {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Null | Value::Array(_) | Value::Object(_) => return None,
        other => other.to_string(),
    };
    Some(match edm_type {
        "Edm.String" => format!("'{}'", text.replace('\'', "''")),
        "Edm.Guid" if version == Version::V2 => format!("guid'{text}'"),
        _ => text,
    })
}

/// `root` + `Set(key)` of an entity from a response, by the model's keys.
fn computed_id(
    model: &Model,
    set_name: &str,
    entity: &serde_json::Map<String, Value>,
) -> Option<String> {
    let set = model.entity_set(set_name)?;
    let ty = model.entity_type(&set.entity_type)?;
    let mut parts = Vec::new();
    for key in &ty.key {
        let prop = ty.properties.iter().find(|p| p.name == *key)?;
        parts.push((
            key.as_str(),
            key_literal(&prop.r#type, entity.get(key)?, model.version)?,
        ));
    }
    let predicate = match parts.as_slice() {
        [(_, single)] => single.clone(),
        _ => parts
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(","),
    };
    Some(format!("{}{set_name}({predicate})", model.service_root))
}

/// The entity set named by a V4 `@odata.context` (`$metadata#Books/$entity`,
/// `…$metadata#Books(title)`).
fn context_set(context: &str) -> Option<&str> {
    let after = context.split_once('#')?.1;
    let name = after.split(['/', '(']).next()?;
    (!name.is_empty()).then_some(name)
}

/// What a response tells about ETags: `(entity url, etag)` pairs.
pub fn from_response(
    request: &PluginRequest,
    response: &PluginResponse,
    target: &Target,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let method = util::effective_method(request);
    let clean = |e: &str| (!e.is_empty() && !e.contains(MASK)).then(|| e.to_owned());
    let header_etag = header(&response.headers, "ETag").and_then(clean);
    let location = header(&response.headers, "Location").map(|l| absolute(target, l));
    let addresses_entity =
        is_entity(target) && matches!(method.as_str(), "GET" | "PATCH" | "PUT" | "MERGE");
    if let Some(etag) = &header_etag {
        if let Some(loc) = &location {
            out.push((loc.clone(), etag.clone()));
        } else if addresses_entity {
            out.push((request.url.clone(), etag.clone()));
        }
    }

    let json = response
        .body
        .text
        .as_deref()
        .filter(|_| !response.truncated)
        .filter(|_| header(&response.headers, "Content-Type").is_none_or(|c| c.contains("json")))
        .and_then(|t| serde_json::from_str::<Value>(t).ok());
    let Some(json) = json else {
        return out;
    };
    let mut model: Option<Option<Model>> = None;
    let mut model_of = || {
        model
            .get_or_insert_with(|| util::model(&target.service_root))
            .clone()
    };

    if let Some(d) = json.get("d") {
        // V2: `d` is the entity or `{ results: [...] }`.
        let entries: Vec<&Value> = match d.get("results").and_then(Value::as_array) {
            Some(rows) => rows.iter().collect(),
            None => vec![d],
        };
        for entry in entries {
            let meta = entry.get("__metadata");
            let uri = meta.and_then(|m| m.get("uri")).and_then(Value::as_str);
            let etag = meta
                .and_then(|m| m.get("etag"))
                .and_then(Value::as_str)
                .and_then(clean);
            if let (Some(uri), Some(etag)) = (uri, etag) {
                out.push((absolute(target, uri), etag));
            }
        }
    } else if let Some(object) = json.as_object() {
        let context = object.get("@odata.context").and_then(Value::as_str);
        let set = context.and_then(context_set);
        let single = |entry: &serde_json::Map<String, Value>,
                      model: &mut dyn FnMut() -> Option<Model>| {
            let etag = entry
                .get("@odata.etag")
                .and_then(Value::as_str)
                .and_then(clean)?;
            let id = entry
                .get("@odata.id")
                .and_then(Value::as_str)
                .map(|id| absolute(target, id))
                .or_else(|| computed_id(&model()?, set?, entry))?;
            Some((id, etag))
        };
        match object.get("value").and_then(Value::as_array) {
            Some(rows) if context.is_some() => {
                for row in rows.iter().filter_map(Value::as_object) {
                    out.extend(single(row, &mut model_of));
                }
            }
            _ => match single(object, &mut model_of) {
                Some(pair) => out.push(pair),
                None => {
                    if let Some(etag) = object
                        .get("@odata.etag")
                        .and_then(Value::as_str)
                        .and_then(clean)
                        && addresses_entity
                    {
                        out.push((request.url.clone(), etag));
                    }
                }
            },
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|(url, _)| seen.insert(util::entity_url(url)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(url: &str) -> Target {
        service::split(url, &[]).unwrap()
    }

    #[test]
    fn action_entities() {
        let t = target(
            "http://h/odata/v4/admin/Books(ID=1,IsActiveEntity=true)/AdminService.draftEdit",
        );
        assert_eq!(
            action_entity(&t).as_deref(),
            Some("http://h/odata/v4/admin/Books(ID=1,IsActiveEntity=true)")
        );
        assert_eq!(
            action_entity(&target("http://h/odata/v4/admin/Books(1)/author")),
            None
        );
        assert!(is_entity(&target("http://h/odata/v4/admin/Books(1)")));
        assert!(!is_entity(&target("http://h/odata/v4/admin/Books")));
        assert_eq!(context_set("$metadata#Books/$entity"), Some("Books"));
        assert_eq!(
            context_set("http://h/x/$metadata#Books(title,ID)"),
            Some("Books")
        );
    }
}
