// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Putting the pages together: the items of the array at the merge path of every later page are
//! appended to the array of the first page; every other field of the first page stays as it
//! was (a root array is concatenated).
//!
//! Plain paths (`$`, `$.items`, `$.d.results`, `$['a b'][0].c`: names, quoted names and
//! indexes) merge **as text**: the array of the first page is cut open at its closing bracket
//! and the inner text of each later array is appended, so nothing is parsed into a tree and the
//! peak memory stays near the size of the result (a 100 MiB answer does not need 500 MiB of
//! JSON values). Any other JSONPath (a wildcard, a filter, a recursive descent) uses the first
//! array it finds and merges through a parsed tree, which costs several times the size.

use std::borrow::Cow;
use std::collections::HashMap;

use serde_json::Value;
use serde_json::value::RawValue;
use serde_json_path::JsonPath;
use serde_json_path::PathElement;

/// One step of a plain path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Seg {
    Name(String),
    Index(usize),
}

/// The steps of a plain path, or `None` if the path uses anything beyond names and indexes.
pub(super) fn plain_path(path: &str) -> Option<Vec<Seg>> {
    let rest = path.trim().strip_prefix('$')?;
    let mut segs = Vec::new();
    let mut chars = rest.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '.' => {
                let start = i + 1;
                let mut end = start;
                while let Some(&(j, d)) = chars.peek() {
                    if d == '.' || d == '[' {
                        break;
                    }
                    end = j + d.len_utf8();
                    chars.next();
                }
                let name = &rest[start..end];
                if name.is_empty() || name.contains(['*', ' ']) {
                    return None;
                }
                segs.push(Seg::Name(name.to_owned()));
            }
            '[' => {
                let close = rest[i..].find(']')? + i;
                let inner = &rest[i + 1..close];
                if let Some(q) = inner.chars().next().filter(|q| *q == '\'' || *q == '"') {
                    if inner.len() < 2 || !inner.ends_with(q) {
                        return None;
                    }
                    let name = &inner[1..inner.len() - 1];
                    if name.contains([q, '\\']) {
                        return None;
                    }
                    segs.push(Seg::Name(name.to_owned()));
                } else {
                    segs.push(Seg::Index(inner.parse().ok()?));
                }
                // Skip to just after the `]`.
                while chars.peek().is_some_and(|&(j, _)| j <= close) {
                    chars.next();
                }
            }
            _ => return None,
        }
    }
    Some(segs)
}

/// The raw text of the array at `path` in `text`, if there is an array.
fn locate<'a>(text: &'a str, path: &[Seg]) -> Option<&'a str> {
    let mut cur: &'a RawValue = serde_json::from_str(text).ok()?;
    for seg in path {
        let raw: &'a str = cur.get();
        cur = match seg {
            Seg::Name(name) => {
                let map: HashMap<Cow<'a, str>, &'a RawValue> = serde_json::from_str(raw).ok()?;
                *map.get(name.as_str())?
            }
            Seg::Index(i) => {
                let list: Vec<&'a RawValue> = serde_json::from_str(raw).ok()?;
                *list.get(*i)?
            }
        };
    }
    let raw = cur.get();
    (raw.starts_with('[') && raw.ends_with(']')).then_some(raw)
}

/// The text between the brackets of an array's raw text, trimmed.
fn inner(array: &str) -> &str {
    array[1..array.len() - 1].trim()
}

enum Mode {
    /// Plain path: `head` is the first page up to the closing bracket of the array and grows.
    Text {
        path: Vec<Seg>,
        head: String,
        tail: String,
        has_items: bool,
    },
    /// Any other path: a parsed tree, `location` is where the array is.
    Tree {
        path: JsonPath,
        location: Vec<Seg>,
        value: Value,
    },
}

pub(super) struct Merger {
    mode: Mode,
}

fn tree_location(path: &JsonPath, value: &Value) -> Option<Vec<Seg>> {
    path.query_located(value)
        .all()
        .into_iter()
        .find(|n| n.node().is_array())
        .map(|n| {
            n.location()
                .iter()
                .map(|e| match e {
                    PathElement::Name(n) => Seg::Name((*n).to_owned()),
                    PathElement::Index(i) => Seg::Index(*i),
                })
                .collect()
        })
}

fn array_at<'v>(value: &'v mut Value, location: &[Seg]) -> Option<&'v mut Vec<Value>> {
    let mut cur = value;
    for seg in location {
        cur = match seg {
            Seg::Name(n) => cur.get_mut(n.as_str())?,
            Seg::Index(i) => cur.get_mut(*i)?,
        };
    }
    cur.as_array_mut()
}

impl Merger {
    /// Starts with the first page. `Err` says why the path finds no array on it.
    pub(super) fn new(first: &str, merge_path: &str) -> Result<Self, String> {
        if let Some(path) = plain_path(merge_path) {
            let array = locate(first, &path).ok_or_else(|| {
                format!("the merge path {merge_path} finds no array on the first page")
            })?;
            let start = array.as_ptr() as usize - first.as_ptr() as usize;
            let close = start + array.len() - 1;
            return Ok(Self {
                mode: Mode::Text {
                    path,
                    head: first[..close].to_owned(),
                    tail: first[close..].to_owned(),
                    has_items: !inner(array).is_empty(),
                },
            });
        }
        let compiled = JsonPath::parse(merge_path)
            .map_err(|e| format!("the merge path {merge_path} is not JSONPath: {e}"))?;
        let value: Value =
            serde_json::from_str(first).map_err(|e| format!("the first page is not JSON: {e}"))?;
        let location = tree_location(&compiled, &value).ok_or_else(|| {
            format!("the merge path {merge_path} finds no array on the first page")
        })?;
        Ok(Self {
            mode: Mode::Tree {
                path: compiled,
                location,
                value,
            },
        })
    }

    /// Appends the items of `page`. `Err` says why the page does not fit (not JSON, no array).
    pub(super) fn add(&mut self, page: &str) -> Result<(), String> {
        match &mut self.mode {
            Mode::Text {
                path,
                head,
                has_items,
                ..
            } => {
                let array = locate(page, path)
                    .ok_or_else(|| "the merge path finds no array on this page".to_owned())?;
                let items = inner(array);
                if !items.is_empty() {
                    if *has_items {
                        head.push(',');
                    }
                    head.push_str(items);
                    *has_items = true;
                }
                Ok(())
            }
            Mode::Tree {
                path,
                location,
                value,
            } => {
                let mut page: Value =
                    serde_json::from_str(page).map_err(|e| format!("a page is not JSON: {e}"))?;
                let at = tree_location(path, &page)
                    .ok_or_else(|| "the merge path finds no array on this page".to_owned())?;
                let items = array_at(&mut page, &at)
                    .map(std::mem::take)
                    .ok_or_else(|| "the merge path finds no array on this page".to_owned())?;
                array_at(value, location)
                    .ok_or_else(|| "the merged array vanished".to_owned())?
                    .extend(items);
                Ok(())
            }
        }
    }

    /// The merged body.
    pub(super) fn finish(self) -> String {
        match self.mode {
            Mode::Text { mut head, tail, .. } => {
                head.push_str(&tail);
                head
            }
            Mode::Tree { value, .. } => value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths() {
        assert_eq!(plain_path("$"), Some(vec![]));
        assert_eq!(plain_path("$.items"), Some(vec![Seg::Name("items".into())]));
        assert_eq!(
            plain_path("$.d.results"),
            Some(vec![Seg::Name("d".into()), Seg::Name("results".into())])
        );
        assert_eq!(
            plain_path("$['a b'][0].c"),
            Some(vec![
                Seg::Name("a b".into()),
                Seg::Index(0),
                Seg::Name("c".into())
            ])
        );
        assert_eq!(plain_path("$.items[*]"), None);
        assert_eq!(plain_path("$..items"), None);
        assert_eq!(plain_path("$.items[?@.a]"), None);
        assert_eq!(plain_path("items"), None);
    }

    #[test]
    fn text_merge_keeps_the_first_page_and_appends() {
        let mut m = Merger::new(r#"{"total":3,"items":[1,2],"next":"x"}"#, "$.items").unwrap();
        m.add(r#"{"items":[3,4],"next":"y"}"#).unwrap();
        m.add(r#"{"items":[],"next":null}"#).unwrap();
        m.add(r#"{ "items" : [ 5 ] }"#).unwrap();
        assert_eq!(m.finish(), r#"{"total":3,"items":[1,2,3,4,5],"next":"x"}"#);
    }

    #[test]
    fn an_empty_first_array_takes_the_items_without_a_comma() {
        let mut m = Merger::new(r#"{"items":[ ]}"#, "$.items").unwrap();
        m.add(r#"{"items":[1]}"#).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&m.finish()).unwrap()["items"],
            serde_json::json!([1])
        );
    }

    #[test]
    fn a_root_array_is_concatenated() {
        let mut m = Merger::new("[1, 2]", "$").unwrap();
        m.add("[3]").unwrap();
        assert_eq!(m.finish(), "[1, 2,3]");
    }

    #[test]
    fn a_wildcard_path_goes_through_the_tree() {
        let mut m = Merger::new(r#"{"a":{"list":[1]},"b":{"list":[9]}}"#, "$.*.list").unwrap();
        m.add(r#"{"a":{"list":[2]}}"#).unwrap();
        let v: Value = serde_json::from_str(&m.finish()).unwrap();
        assert_eq!(v["a"]["list"], serde_json::json!([1, 2]));
        assert_eq!(v["b"]["list"], serde_json::json!([9]));
    }

    #[test]
    fn a_missing_array_says_so() {
        assert!(Merger::new(r#"{"items":1}"#, "$.items").is_err());
        let mut m = Merger::new(r#"{"items":[1]}"#, "$.items").unwrap();
        assert!(m.add(r#"{"other":[1]}"#).is_err());
        assert!(m.add("not json").is_err());
    }
}
