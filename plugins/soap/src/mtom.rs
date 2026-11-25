// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reading MTOM/XOP responses (`multipart/related`): the root part is the envelope, the other
//! parts are attachments the envelope refers to with `cid:`. Read only; the plugin never
//! writes MTOM.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub content_id: String,
    pub content_type: String,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The root part (the envelope text).
    pub root: String,
    pub attachments: Vec<Attachment>,
}

/// The `boundary` of a `multipart/related` content type, if it is one.
pub fn boundary(content_type: &str) -> Option<String> {
    if !content_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("multipart/related")
    {
        return None;
    }
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.trim().split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Splits a package. The first part is the root; a package without parts gives `None`.
pub fn split(content_type: &str, body: &[u8]) -> Option<Package> {
    let boundary = boundary(content_type)?;
    let delim = format!("--{boundary}").into_bytes();
    let mut parts: Vec<(String, String, &[u8])> = Vec::new();
    let mut pos = find(body, &delim, 0)?;
    loop {
        let start = pos + delim.len();
        if body[start..].starts_with(b"--") {
            break;
        }
        let part_start = body[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| start + i + 1)?;
        let next = find(body, &delim, part_start);
        let part_end = next.unwrap_or(body.len());
        let part = &body[part_start..part_end];
        let (headers, content) = match find(part, b"\r\n\r\n", 0) {
            Some(i) => (&part[..i], &part[i + 4..]),
            None => match find(part, b"\n\n", 0) {
                Some(i) => (&part[..i], &part[i + 2..]),
                None => (&part[..0], part),
            },
        };
        let content = content
            .strip_suffix(b"\r\n")
            .or(content.strip_suffix(b"\n"))
            .unwrap_or(content);
        let header_text = String::from_utf8_lossy(headers).into_owned();
        let get = |name: &str| {
            header_text.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_owned())
            })
        };
        parts.push((
            get("Content-ID")
                .unwrap_or_default()
                .trim_matches(['<', '>'])
                .to_owned(),
            get("Content-Type").unwrap_or_default(),
            content,
        ));
        match next {
            Some(n) => pos = n,
            None => break,
        }
    }
    let mut parts = parts.into_iter();
    let (_, _, root) = parts.next()?;
    Some(Package {
        root: String::from_utf8_lossy(root).into_owned(),
        attachments: parts
            .map(|(id, ct, content)| Attachment {
                content_id: id,
                content_type: ct,
                size: content.len(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_root_and_attachments() {
        let body = b"--b\r\nContent-Type: application/xop+xml\r\nContent-ID: <root>\r\n\r\n<a/>\r\n--b\r\nContent-Type: image/png\r\nContent-ID: <img1>\r\n\r\n\x89PNG\x00\x01\r\n--b--\r\n";
        let p = split(
            "multipart/related; type=\"application/xop+xml\"; boundary=\"b\"",
            body,
        )
        .unwrap();
        assert_eq!(p.root, "<a/>");
        assert_eq!(
            p.attachments,
            vec![Attachment {
                content_id: "img1".into(),
                content_type: "image/png".into(),
                size: 6
            }]
        );
        assert!(boundary("text/xml").is_none());
    }
}
