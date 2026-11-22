// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The secret scanner before a commit (wave 5, strand E): reads the files a commit would take
// from the collection's directory handle, as they are on disk, and scans them with the core
// (`scanSecrets`, i.e. `rt_app::secret_scan`), so the web app finds exactly what the desktop
// app and `rt scan` find. Files the scanner skips (deleted, not UTF-8, with NUL bytes, over
// 4 MiB) are left out, as in `rt_app::secret_scan::text_of`.

import { BackendError, type SecretFinding } from "@requesttailor/ui";

import type { CoreModule } from "../wasm";
import { directoryFs } from "./fs";

/** Larger files are not scanned (`rt_app::secret_scan::MAX_SCAN_BYTES`). */
export const MAX_SCAN_BYTES = 4 * 1024 * 1024;

export type Scanner = Pick<CoreModule, "scanSecrets" | "secretRefusal">;

/** The text the scanner reads, or `null` for a file it skips. */
export function textOf(bytes: Uint8Array): string | null {
  if (bytes.length > MAX_SCAN_BYTES || bytes.includes(0)) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** `[path, text]` of `paths` (collection-relative) that exist and are text. */
export async function readForScan(handle: FileSystemDirectoryHandle, paths: string[]): Promise<[string, string][]> {
  const { promises: fs } = directoryFs(handle);
  const out: [string, string][] = [];
  for (const path of paths) {
    let bytes: Uint8Array;
    try {
      bytes = (await fs.readFile(path)) as Uint8Array;
    } catch (e) {
      if ((e as { code?: string }).code === "ENOENT") continue;
      throw e;
    }
    const text = textOf(bytes);
    if (text !== null) out.push([path, text]);
  }
  return out;
}

/** Credentials typed into `paths`. */
export async function scanPaths(
  handle: FileSystemDirectoryHandle,
  paths: string[],
  core: Scanner,
): Promise<SecretFinding[]> {
  return core.scanSecrets(await readForScan(handle, paths));
}

/** Throws `secrets-found` (as the desktop app) when `paths` contain credentials. */
export async function refuseSecrets(
  handle: FileSystemDirectoryHandle,
  paths: string[],
  core: Scanner,
): Promise<void> {
  const findings = await scanPaths(handle, paths, core);
  if (findings.length > 0) throw new BackendError("secrets-found", core.secretRefusal(findings));
}
