// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Where a request URL's OData service starts, shared by all strands: the service root is
//! the key for cached metadata, CSRF tokens and ETags.

use crate::edm::Version;

/// A request URL split into service root, resource path and query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// With a trailing `/`.
    pub service_root: String,
    /// After the root, without the leading `/`, e.g. `Books(1)/author` or `$batch`.
    pub resource: String,
    /// Without the `?`; may be empty.
    pub query: String,
    /// Guessed from the URL (`/sap/opu/odata4/`, `/odata/v4/` → V4, `/sap/opu/odata/` →
    /// V2); `None` when the URL does not say. Cached metadata is the authority.
    pub version: Option<Version>,
}

/// Finds the service root of `url` by the usual layouts:
/// - SAP Gateway V2: `…/sap/opu/odata/<namespace>/<SERVICE>/…`
/// - SAP V4: `…/sap/opu/odata4/<ns>/<group>/<repository>/<ns>/<service>/<version>/…`
/// - CAP: `…/odata/v4/<service>/…` and `…/odata/v2/<service>/…`
/// - otherwise: the path up to a segment that looks like an entity set call (`Name(…)`), a
///   `$`-segment, or the last segment.
///
/// `known_roots` are roots with cached metadata; the longest matching prefix wins over the
/// rules above.
pub fn split(url: &str, known_roots: &[String]) -> Option<Target> {
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let base = base.split_once('#').map_or(base, |(b, _)| b);
    let scheme_end = base.find("://")? + 3;
    let path_start = base[scheme_end..].find('/').map(|i| scheme_end + i)?;
    let (origin, path) = base.split_at(path_start);

    if let Some(root) = known_roots
        .iter()
        .filter(|r| base.starts_with(r.trim_end_matches('/')))
        .max_by_key(|r| r.len())
    {
        let resource = base[root.trim_end_matches('/').len()..].trim_start_matches('/');
        return Some(Target {
            service_root: format!("{}/", root.trim_end_matches('/')),
            resource: resource.to_owned(),
            query: query.to_owned(),
            version: None,
        });
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let find = |marker: &[&str]| {
        segments
            .windows(marker.len())
            .position(|w| w.iter().zip(marker).all(|(a, b)| a.eq_ignore_ascii_case(b)))
    };
    let (root_len, version) = if let Some(i) = find(&["sap", "opu", "odata4"]) {
        (i + 3 + 6, Some(Version::V4))
    } else if let Some(i) = find(&["sap", "opu", "odata"]) {
        (i + 3 + 2, Some(Version::V2))
    } else if let Some(i) = find(&["odata", "v4"]) {
        (i + 2 + 1, Some(Version::V4))
    } else if let Some(i) = find(&["odata", "v2"]) {
        (i + 2 + 1, Some(Version::V2))
    } else {
        let end = segments
            .iter()
            .position(|s| s.starts_with('$') || s.contains('('))
            .unwrap_or(segments.len().saturating_sub(1));
        (end, None)
    };
    let root_len = root_len.min(segments.len());
    let root = segments[..root_len].join("/");
    Some(Target {
        service_root: format!("{origin}/{root}{}", if root.is_empty() { "" } else { "/" }),
        resource: segments[root_len..].join("/"),
        query: query.to_owned(),
        version,
    })
}

/// Storage key for data of a service root (`<what>:<root>`), e.g. `metadata:` or `csrf:`.
/// Plugin storage keys have at most 256 bytes; longer roots are hashed.
pub fn storage_key(what: &str, service_root: &str) -> String {
    let key = format!("{what}:{service_root}");
    if key.len() <= 256 {
        return key;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in service_root.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{what}:#{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sap_gateway_v2() {
        let t = split(
            "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet('1')?$format=json",
            &[],
        )
        .unwrap();
        assert_eq!(
            t.service_root,
            "https://s4.example.com/sap/opu/odata/sap/ZRT_SALES_SRV/"
        );
        assert_eq!(t.resource, "SalesOrderSet('1')");
        assert_eq!(t.query, "$format=json");
        assert_eq!(t.version, Some(Version::V2));
    }

    #[test]
    fn sap_v4() {
        let t = split(
            "https://s4.example.com/sap/opu/odata4/sap/zrt_sales/srvd_a2x/sap/zrt_sales/0001/SalesOrder",
            &[],
        )
        .unwrap();
        assert!(t.service_root.ends_with("/zrt_sales/0001/"));
        assert_eq!(t.resource, "SalesOrder");
        assert_eq!(t.version, Some(Version::V4));
    }

    #[test]
    fn cap_v4_and_known_roots() {
        let t = split(
            "http://localhost:4004/odata/v4/catalog/Books(1)/author",
            &[],
        )
        .unwrap();
        assert_eq!(t.service_root, "http://localhost:4004/odata/v4/catalog/");
        assert_eq!(t.resource, "Books(1)/author");
        let known = vec!["http://localhost:4004/api/".to_owned()];
        let t = split("http://localhost:4004/api/Books?$top=1", &known).unwrap();
        assert_eq!(t.service_root, "http://localhost:4004/api/");
        assert_eq!(t.resource, "Books");
    }

    #[test]
    fn generic_and_batch() {
        let t = split("http://h/svc/Orders(5)", &[]).unwrap();
        assert_eq!(t.service_root, "http://h/svc/");
        let t = split("http://h/svc/$batch", &[]).unwrap();
        assert_eq!(t.resource, "$batch");
    }

    #[test]
    fn long_keys_are_hashed() {
        let long = format!("https://h/{}/", "a".repeat(300));
        assert!(storage_key("metadata", &long).len() <= 256);
        assert_eq!(storage_key("csrf", "https://h/s/"), "csrf:https://h/s/");
    }
}
