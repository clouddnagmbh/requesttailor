// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A minimal `.zip` reader for importing a multi-file OpenAPI spec as one archive.
//!
//! Reads the central directory (the end-of-central-directory record is searched from the
//! end), then every entry from its local header: stored (method 0) and deflated (method 8,
//! inflated with `miniz_oxide`) entries. Directories, hidden files and folders (a path
//! component starting with `.`) and macOS's `__MACOSX` folder are skipped. Not supported, with
//! a clear error: encrypted entries, other compression methods, ZIP64 archives.
//!
//! Hostile input never panics: every offset is checked against the data, the number of
//! entries is bounded, and the inflated size of all entries together is bounded by
//! [`MAX_INPUT_BYTES`] (a zip bomb fails with [`ImportError::TooLarge`]).

use super::{ImportError, MAX_INPUT_BYTES};

/// Most entries read from one archive.
const MAX_ENTRIES: usize = 4096;

const EOCD_SIG: u32 = 0x0605_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

fn bad(message: impl Into<String>) -> ImportError {
    ImportError::Files {
        message: format!("not a readable zip archive: {}", message.into()),
    }
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// `true` if the data starts like a zip archive.
pub(super) fn is_zip(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06")
}

/// Offset of the end-of-central-directory record.
fn find_eocd(data: &[u8]) -> Option<usize> {
    // 22 bytes of record plus at most 65535 bytes of comment.
    let lowest = data.len().saturating_sub(22 + 0xFFFF);
    let mut at = data.len().checked_sub(22)?;
    loop {
        if u32_at(data, at) == Some(EOCD_SIG) {
            return Some(at);
        }
        if at == lowest {
            return None;
        }
        at -= 1;
    }
}

/// Whether an entry is left out: directories, hidden files, `__MACOSX`.
fn skipped(name: &str) -> bool {
    name.is_empty()
        || name.ends_with('/')
        || name
            .split('/')
            .any(|c| c.starts_with('.') || c == "__MACOSX")
}

pub(super) fn unzip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, ImportError> {
    let too_large = || ImportError::TooLarge {
        limit_mb: (MAX_INPUT_BYTES / (1024 * 1024)) as u32,
    };
    if data.len() > MAX_INPUT_BYTES {
        return Err(too_large());
    }
    let eocd = find_eocd(data).ok_or_else(|| bad("no central directory"))?;
    let entries = u16_at(data, eocd + 10).ok_or_else(|| bad("truncated"))? as usize;
    let cd_size = u32_at(data, eocd + 12).ok_or_else(|| bad("truncated"))?;
    let cd_offset = u32_at(data, eocd + 16).ok_or_else(|| bad("truncated"))?;
    if entries == 0xFFFF || cd_size == u32::MAX || cd_offset == u32::MAX {
        return Err(bad("ZIP64 archives are not supported"));
    }
    if entries > MAX_ENTRIES {
        return Err(bad(format!("more than {MAX_ENTRIES} entries")));
    }
    let mut at = cd_offset as usize;
    let mut out = Vec::new();
    let mut total = 0usize;
    for _ in 0..entries {
        if u32_at(data, at) != Some(CENTRAL_SIG) {
            return Err(bad("damaged central directory"));
        }
        let field = |off: usize| u16_at(data, at + off).ok_or_else(|| bad("truncated"));
        let flags = field(8)?;
        let method = field(10)?;
        let compressed = u32_at(data, at + 20).ok_or_else(|| bad("truncated"))? as usize;
        let size = u32_at(data, at + 24).ok_or_else(|| bad("truncated"))? as usize;
        let name_len = field(28)? as usize;
        let extra_len = field(30)? as usize;
        let comment_len = field(32)? as usize;
        let local = u32_at(data, at + 42).ok_or_else(|| bad("truncated"))? as usize;
        let name_bytes = data
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| bad("truncated"))?;
        let name = String::from_utf8_lossy(name_bytes).replace('\\', "/");
        at += 46 + name_len + extra_len + comment_len;
        if skipped(&name) {
            continue;
        }
        if flags & 1 != 0 {
            return Err(bad(format!("{name} is encrypted")));
        }
        if u32_at(data, local) != Some(LOCAL_SIG) {
            return Err(bad(format!("{name}: damaged entry")));
        }
        let local_name = u16_at(data, local + 26).ok_or_else(|| bad("truncated"))? as usize;
        let local_extra = u16_at(data, local + 28).ok_or_else(|| bad("truncated"))? as usize;
        let start = local + 30 + local_name + local_extra;
        let raw = start
            .checked_add(compressed)
            .and_then(|end| data.get(start..end))
            .ok_or_else(|| bad(format!("{name}: truncated")))?;
        let room = MAX_INPUT_BYTES - total;
        let bytes = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, room.saturating_add(1))
                .map_err(|e| {
                    if e.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput {
                        too_large()
                    } else {
                        bad(format!("{name}: damaged compressed data"))
                    }
                })?,
            m => {
                return Err(bad(format!(
                    "{name} uses compression method {m}; only stored and deflated entries are \
                     supported"
                )));
            }
        };
        if bytes.len() != size && method == 0 {
            return Err(bad(format!("{name}: damaged entry")));
        }
        if bytes.len() > room {
            return Err(too_large());
        }
        total += bytes.len();
        out.push((name.trim_start_matches('/').to_owned(), bytes));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A zip archive of `files`, stored or deflated (with `miniz_oxide`).
    pub(crate) fn build(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let (method, body) = if deflate {
                (8u16, miniz_oxide::deflate::compress_to_vec(data, 6))
            } else {
                (0u16, data.to_vec())
            };
            let offset = out.len() as u32;
            let header = |sig: u32, central_entry: bool| {
                let mut h = Vec::new();
                h.extend(sig.to_le_bytes());
                if central_entry {
                    h.extend(20u16.to_le_bytes()); // made by
                }
                h.extend(20u16.to_le_bytes()); // needed
                h.extend(0u16.to_le_bytes()); // flags
                h.extend(method.to_le_bytes());
                h.extend([0u8; 4]); // time, date
                h.extend(0u32.to_le_bytes()); // crc (not checked)
                h.extend((body.len() as u32).to_le_bytes());
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((name.len() as u16).to_le_bytes());
                h.extend(0u16.to_le_bytes()); // extra
                if central_entry {
                    h.extend(0u16.to_le_bytes()); // comment
                    h.extend(0u16.to_le_bytes()); // disk
                    h.extend(0u16.to_le_bytes()); // internal attributes
                    h.extend(0u32.to_le_bytes()); // external attributes
                    h.extend(offset.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            let local = header(LOCAL_SIG, false);
            central.extend(header(CENTRAL_SIG, true));
            out.extend(local);
            out.extend(&body);
        }
        let cd_offset = out.len() as u32;
        out.extend(&central);
        out.extend(EOCD_SIG.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(cd_offset.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[test]
    fn stored_and_deflated_entries_are_read_and_junk_is_skipped() {
        let files: &[(&str, &[u8])] = &[
            ("api/openapi.yaml", b"openapi: 3.0.3\n"),
            ("api/schemas/", b""),
            ("api/schemas/pet.yaml", b"type: object\n"),
            ("__MACOSX/api/._openapi.yaml", b"junk"),
            ("api/.DS_Store", b"junk"),
        ];
        for deflate in [false, true] {
            let zip = build(files, deflate);
            assert!(is_zip(&zip));
            let got = unzip(&zip).unwrap();
            assert_eq!(
                got,
                vec![
                    ("api/openapi.yaml".to_owned(), b"openapi: 3.0.3\n".to_vec()),
                    (
                        "api/schemas/pet.yaml".to_owned(),
                        b"type: object\n".to_vec()
                    ),
                ]
            );
        }
    }

    #[test]
    fn hostile_archives_fail_cleanly() {
        let zip = build(&[("a.yaml", b"openapi: 3.0.3\n")], true);
        // Every truncation and every single flipped byte: an error or a result, never a panic.
        for cut in 0..zip.len() {
            let _ = unzip(&zip[..cut]);
        }
        for i in 0..zip.len() {
            let mut z = zip.clone();
            z[i] ^= 0xFF;
            let _ = unzip(&z);
        }
        assert!(unzip(b"PK\x03\x04 not really").is_err());
        assert!(unzip(b"").is_err());
        // A bomb: 70 MB of zeros deflate to a few KB, but are over the limit.
        let zeros = vec![0u8; MAX_INPUT_BYTES + 1024];
        let bomb = build(&[("big.yaml", &zeros)], true);
        assert!(bomb.len() < 1024 * 1024);
        assert!(matches!(unzip(&bomb), Err(ImportError::TooLarge { .. })));
    }
}
