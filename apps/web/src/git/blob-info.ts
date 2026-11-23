// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the viewers get of a file (`GitApi.blob`, wave 15 strand B), from its bytes: the same
// rules as `rt_app::git::files` and `rt-git/src/blob.rs` (text up to 8 MiB, images up to 16 MiB
// as base64, binary and big files as metadata, Git LFS pointers recognised).

import type { GitBlob } from "@requesttailor/ui";

export const BLOB_TEXT_LIMIT = 8 * 1024 * 1024;
export const BLOB_IMAGE_LIMIT = 16 * 1024 * 1024;
/** Looked at to tell binary from text (Git's window). */
const SNIFF = 8 * 1024;
const LFS_POINTER_MAX = 1024;
const LFS_PREFIX = "version https://git-lfs.github.com/spec/v1";

const MEDIA: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  ico: "image/x-icon",
  bmp: "image/bmp",
  avif: "image/avif",
  pdf: "application/pdf",
};

/** `rt_app::git::files::media_type`: by extension, case-insensitive. */
export function mediaType(path: string): string | undefined {
  const dot = path.lastIndexOf(".");
  if (dot < 0 || path.lastIndexOf("/") > dot) return undefined;
  return MEDIA[path.slice(dot + 1).toLowerCase()];
}

export function base64(bytes: Uint8Array): string {
  let out = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) out += String.fromCharCode(...bytes.subarray(i, i + step));
  return btoa(out);
}

const strict = new TextDecoder("utf-8", { fatal: true });

/** `bytes` as the viewers see them. `rev` is the commit it was read at (`undefined`: the
 *  working tree), `oid` its blob id if known. */
export function describeBlob(path: string, rev: string | undefined, bytes: Uint8Array, oid: string | undefined): GitBlob {
  const media = mediaType(path);
  const image = media?.startsWith("image/") ?? false;
  const raster = image && media !== "image/svg+xml";
  const size = bytes.length;
  const sniffed = bytes.subarray(0, SNIFF);
  let text: string | null = null;
  if (!sniffed.includes(0)) {
    try {
      text = strict.decode(bytes);
    } catch {
      text = null;
    }
  }
  const binary = raster || text === null;
  const limit = image ? Math.max(BLOB_IMAGE_LIMIT, BLOB_TEXT_LIMIT) : BLOB_TEXT_LIMIT;
  const out: GitBlob = { path, size, binary, truncated: size > limit, lfs_pointer: false };
  if (rev !== undefined) out.rev = rev;
  if (oid !== undefined) out.oid = oid;
  if (media) out.media_type = media;
  if (text !== null && size <= LFS_POINTER_MAX && text.startsWith(LFS_PREFIX)) out.lfs_pointer = true;
  if (!binary && text !== null && size <= BLOB_TEXT_LIMIT) out.text = text;
  if (image && size <= BLOB_IMAGE_LIMIT) out.base64 = base64(bytes);
  return out;
}
