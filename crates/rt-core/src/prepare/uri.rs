// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! URL building: interpolation, path params, percent-encoding.
//!
//! The URL is the truth for the query string, as in Bruno: query params in the request's
//! `params` list only mirror the URL for display and are never appended.
//!
//! Follows Bruno where it matters for imported collections
//! (`packages/bruno-electron/src/ipc/network/interpolate-vars.js`, `.../network/index.js` and
//! `packages/bruno-common/src/utils/url/index.ts`):
//!
//! - A URL without an explicit `scheme://` gets `http://` in front.
//! - Path params replace whole `:name` segments, and `:name` inside OData-style segments such
//!   as `Users(:id)` or `Orders(Key1=:k1)`. A param that is disabled or blank leaves `:name`.
//! - With encoding on, scheme and authority stay as written, path segments and query names and
//!   values are percent-encoded, and `#` counts as data (`%23`), as in Bruno. Unlike Bruno,
//!   existing `%XX` escapes are kept in the query too, so nothing is encoded twice.
//! - Path segments keep RFC 3986 `pchar`s (`:@!$&'()*+,;=`), so OData keys survive unencoded;
//!   query names and values are encoded like `encodeURIComponent`.
//! - With encoding off the URL goes out as written.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::{Interp, PrepareError};
use crate::model::{Param, ParamKind};

/// RFC 3986 `pchar` without `%`: unreserved, sub-delims, `:` and `@`.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=')
    .remove(b':')
    .remove(b'@');

/// What `encodeURIComponent` leaves alone.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'!')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// The URL after interpolation, before encoding.
pub(super) struct Draft {
    /// `scheme://authority`, verbatim.
    origin: String,
    path: String,
    /// Pairs as written in the URL; `None` if the URL has no `?`.
    query: Option<Vec<String>>,
    /// With encoding off only: `#...` as written.
    fragment: String,
    encode: bool,
}

/// Interpolates the URL template and substitutes path params. Query params are ignored:
/// the URL already carries the query.
pub(super) fn draft(
    template: &str,
    params: &[Param],
    encode: bool,
    interp: &mut Interp<'_>,
) -> Draft {
    let mut url = interp.text(template);
    if !has_explicit_scheme(&url) {
        url.insert_str(0, "http://");
    }

    let path_params: Vec<(&str, String)> = params
        .iter()
        .filter(|p| p.enabled && p.kind == ParamKind::Path)
        .map(|p| (p.name.as_str(), interp.text(&p.value)))
        .collect();

    let origin_end = origin_len(&url);
    let rest = &url[origin_end..];
    // With encoding off `#` starts the fragment; with encoding on it is data, like in Bruno.
    let (rest, fragment) = match rest.find('#') {
        Some(i) if !encode => (&rest[..i], rest[i..].to_owned()),
        _ => (rest, String::new()),
    };
    let (path, query) = match rest.find('?') {
        Some(i) => (
            &rest[..i],
            Some(
                rest[i + 1..]
                    .split('&')
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
        ),
        None => (rest, None),
    };
    let path = if path_params.is_empty() {
        path.to_owned()
    } else {
        substitute_path_params(path, &path_params)
    };

    Draft {
        origin: url[..origin_end].to_owned(),
        path,
        query,
        fragment,
        encode,
    }
}

impl Draft {
    /// Encodes, adds an API key query param (replacing one of the same name) and validates.
    pub(super) fn finish(
        self,
        api_key: Option<(String, String)>,
        unresolved: &[String],
    ) -> Result<String, PrepareError> {
        let mut out = self.origin;
        let mut pairs: Vec<String> = Vec::new();
        if self.encode {
            let path: Vec<String> = self
                .path
                .split('/')
                .map(|s| encode(s, PATH_SEGMENT))
                .collect();
            out.push_str(&path.join("/"));
            for pair in self.query.iter().flatten() {
                let encoded = match pair.split_once('=') {
                    Some((n, v)) => format!("{}={}", encode(n, COMPONENT), encode(v, COMPONENT)),
                    None => encode(pair, COMPONENT),
                };
                // Bruno drops pairs without a name.
                if !encoded.starts_with('=') {
                    pairs.push(encoded);
                }
            }
        } else {
            out.push_str(&self.path);
            pairs.extend(self.query.iter().flatten().cloned());
        }
        if let Some((key, value)) = &api_key {
            // Like `URLSearchParams.set`: replaces every pair of that name. Always encoded.
            pairs.retain(|p| {
                let name = p.split_once('=').map_or(p.as_str(), |(n, _)| n);
                percent_encoding::percent_decode_str(name).decode_utf8_lossy() != key.as_str()
            });
            pairs.push(format!(
                "{}={}",
                encode(key, COMPONENT),
                encode(value, COMPONENT)
            ));
        }
        if self.query.is_some() || !pairs.is_empty() {
            out.push('?');
            out.push_str(&pairs.join("&"));
        }
        out.push_str(&self.fragment);

        match url::Url::parse(&out) {
            Ok(_) => Ok(out),
            Err(e) if unresolved.is_empty() => Err(PrepareError::InvalidUrl(format!("{out}: {e}"))),
            Err(e) => Err(PrepareError::InvalidUrl(format!(
                "{out}: {e} (unresolved variables: {})",
                unresolved.join(", ")
            ))),
        }
    }
}

/// Percent-encodes everything in `set`, but keeps existing `%XX` escapes so nothing is encoded
/// twice. A `%` that does not start an escape becomes `%25`.
fn encode(s: &str, set: &'static AsciiSet) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut chunk = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            out.extend(utf8_percent_encode(&s[chunk..i], set));
            out.push_str(&s[i..i + 3]);
            i += 3;
            chunk = i;
        } else {
            i += 1;
        }
    }
    out.extend(utf8_percent_encode(&s[chunk..], set));
    out
}

/// Bruno's `hasExplicitScheme`: `scheme://` with a WHATWG scheme (`localhost:8080` has none).
fn has_explicit_scheme(url: &str) -> bool {
    match url.find("://") {
        Some(end) if end > 0 => {
            let scheme = &url.as_bytes()[..end];
            scheme[0].is_ascii_alphabetic()
                && scheme[1..]
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        }
        _ => false,
    }
}

/// Length of `scheme://authority`. Only called after `has_explicit_scheme` holds.
fn origin_len(url: &str) -> usize {
    let authority = url.find("://").map_or(0, |i| i + 3);
    authority
        + url[authority..]
            .find(['/', '?', '#'])
            .unwrap_or(url.len() - authority)
}

fn substitute_path_params(path: &str, params: &[(&str, String)]) -> String {
    // Bruno: the first param of that name; blank values leave the placeholder.
    let value = |name: &str| {
        params
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
    };
    path.split('/')
        .map(|segment| {
            if let Some(name) = segment.strip_prefix(':') {
                return value(name).unwrap_or(segment).to_owned();
            }
            if is_odata_segment(segment) {
                return substitute_odata(segment, value);
            }
            segment.to_owned()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Bruno's `/^[A-Za-z0-9_.-]+\([^)]*\)$/`.
fn is_odata_segment(segment: &str) -> bool {
    let Some(inner) = segment.strip_suffix(')') else {
        return false;
    };
    let Some((head, args)) = inner.split_once('(') else {
        return false;
    };
    !head.is_empty()
        && head
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
        && !args.contains(')')
}

/// Replaces every `:name` (`[a-zA-Z_]\w*`) that has a value.
fn substitute_odata<'v>(segment: &str, value: impl Fn(&str) -> Option<&'v str>) -> String {
    let bytes = segment.as_bytes();
    let mut out = String::with_capacity(segment.len());
    let mut i = 0;
    while i < bytes.len() {
        let starts_name = bytes[i] == b':'
            && bytes
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_');
        if starts_name {
            let end = i
                + 1
                + bytes[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                    .count();
            if let Some(v) = value(&segment[i + 1..end]) {
                out.push_str(v);
                i = end;
                continue;
            }
        }
        // `:` and ASCII are single bytes; copy everything else char by char.
        let ch_len = segment[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&segment[i..i + ch_len]);
        i += ch_len;
    }
    out
}
