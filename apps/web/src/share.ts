// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShareApi` of the web app (wave 17, strand C), over rt-wasm (crates/rt-wasm/src/share_js.rs).
// Nothing here makes a network request: a link is made and read in the core, from the request
// as it is in memory. The base of a link is the app's own address (without query and fragment),
// unless the caller passes one.

import type { CopiedAs, CopyKind, CopyOptions, ShareApi, ShareLink, SharePayload, ShareOptions } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The share part of rt-wasm's `Workspace`. */
export interface CoreShareWorkspace {
  shareLink(path: string, options: ShareOptions): ShareLink;
  shareCopyAs(path: string, kind: CopyKind, options: CopyOptions): CopiedAs;
}

/** The free functions of rt-wasm for sharing. */
export interface CoreShare {
  shareOpen(input: string): SharePayload;
  shareQr(link: string): string;
}

/** This app's address without query and fragment (`https://app.example/path/`). */
export function ownBase(location: Pick<Location, "origin" | "pathname"> = window.location): string {
  return `${location.origin}${location.pathname}`;
}

export function webShare(session: () => Session, core: () => Promise<CoreShare>, base: () => string = ownBase): ShareApi {
  const ws = () => session().ws as unknown as CoreShareWorkspace;
  const wrap = async <T>(f: () => T | Promise<T>): Promise<T> => {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    link: (path, options) => wrap(() => ws().shareLink(path, { ...options, base: options?.base ?? base() })),
    open: (input) => wrap(async () => (await core()).shareOpen(input)),
    copyAs: (path, kind, options) => wrap(() => ws().shareCopyAs(path, kind, options ?? {})),
    qr: (link) => wrap(async () => (await core()).shareQr(link)),
  };
}
