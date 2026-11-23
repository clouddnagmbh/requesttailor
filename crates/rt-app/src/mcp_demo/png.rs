// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A tiny PNG encoder for the `image` tool: RGBA, uncompressed deflate blocks. No
//! dependency, the same bytes on every platform.

use base64::Engine;

const SIZE: u32 = 64;

/// A 64×64 disc in `color` with a soft highlight, on a transparent background, as base64.
pub(super) fn badge(color: [u8; 3]) -> String {
    let mut raw = Vec::with_capacity((SIZE * (SIZE * 4 + 1)) as usize);
    let center = (SIZE as f64 - 1.0) / 2.0;
    let radius = 28.0;
    for y in 0..SIZE {
        raw.push(0); // filter: none
        for x in 0..SIZE {
            let (dx, dy) = (x as f64 - center, y as f64 - center);
            let r = (dx * dx + dy * dy).sqrt();
            let alpha = (radius + 0.5 - r).clamp(0.0, 1.0);
            // Lighter towards the top left.
            let light = ((-dx - dy) / (2.0 * radius)).clamp(-1.0, 1.0) * 0.25 + 0.05;
            let shade = |c: u8| {
                let c = f64::from(c);
                let v = if light >= 0.0 {
                    c + (255.0 - c) * light
                } else {
                    c * (1.0 + light)
                };
                v.round().clamp(0.0, 255.0) as u8
            };
            raw.extend_from_slice(&[
                shade(color[0]),
                shade(color[1]),
                shade(color[2]),
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    base64::engine::general_purpose::STANDARD.encode(encode(SIZE, SIZE, &raw))
}

/// PNG bytes of `raw` (rows of RGBA, each starting with its filter byte).
fn encode(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bits, RGBA, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A zlib stream of uncompressed deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(0xffff).collect();
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_of_iend_is_the_known_value() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn badge_is_a_png() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(badge([0x3b, 0x82, 0xf6]))
            .unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 64);
        assert!(bytes.ends_with(&[0xae, 0x42, 0x60, 0x82]));
    }
}
