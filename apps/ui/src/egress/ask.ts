// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: the one-time question "Allow <extension> to contact <host> for <purpose>?"
// (a dialog over `AppState.showDialog`, `AskDialog.svelte`). Asked once per host and device; the
// answer "allow" is stored by the caller through `Backend.egress.allow`, "don't allow" is not
// stored (the extension gets the `not-allowed` error and nothing is sent).

import type { AppState } from "../state/app.svelte";
import type { EgressRequest } from "../commands/types";
import AskDialog from "./AskDialog.svelte";

/** The secret names a header refers to (`{{secret:<name>}}`), never values. */
export function secretNames(request: EgressRequest): string[] {
  const names = new Set<string>();
  for (const [, value] of request.headers ?? []) {
    for (const m of value.matchAll(/\{\{\s*secret:([^}]*?)\s*\}\}/g)) names.add(m[1]);
  }
  return [...names];
}

/** `host` or `host:port`, the way the list shows it. Plain http on its default port is port 80,
 *  https on its default port is no port (443). */
export function hostLabel(host: string, port: number | null): string {
  return port === null ? host : `${host}:${port}`;
}

export async function askEgress(app: AppState, request: EgressRequest): Promise<boolean> {
  let url: URL;
  try {
    url = new URL(request.url);
  } catch {
    return false;
  }
  const answer = await app.showDialog({
    title: "Allow network access?",
    component: AskDialog,
    props: {
      extension: request.extension,
      purpose: request.purpose,
      host: url.hostname,
      port: url.port ? Number(url.port) : url.protocol === "http:" ? 80 : null,
      method: request.method,
      secrets: secretNames(request),
    },
    width: 480,
  });
  return answer === true;
}
