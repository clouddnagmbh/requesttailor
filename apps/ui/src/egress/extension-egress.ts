// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ExtensionContext.egress` (wave 19, strand B): guided network access for extensions. A call to
// a host the user has not allowed on this device asks once (`askEgress`) and, if allowed,
// records the host and repeats the call. Everything else (guard, masking, limits) is
// `Backend.egress`.

import type { Backend } from "../commands/backend";
import { BackendError } from "../commands/backend";
import type { EgressHost, EgressRequest, EgressResponse } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { askEgress } from "./ask";

/** What an extension sees of the network access. */
export interface ExtensionEgress {
  /** The hosts the user allowed on this device. */
  hosts(): Promise<EgressHost[]>;
  /** Sends one call (`GET` or `POST`) to an allowed host; asks the user once for a new host.
   *  Rejects with a `BackendError` (`unsupported` until strand B; `not-allowed` when the user
   *  declines; `blocked` when the guard refuses, as under air-gap rules). */
  request(request: EgressRequest): Promise<EgressResponse>;
}

function hostOf(url: string): { host: string; port: number | null } | null {
  try {
    const u = new URL(url);
    return { host: u.hostname, port: u.port ? Number(u.port) : null };
  } catch {
    return null;
  }
}

export function createExtensionEgress(backend: Backend, app: AppState): ExtensionEgress {
  return {
    hosts: () => backend.egress.hosts(),
    async request(request) {
      try {
        return await backend.egress.request(request);
      } catch (e) {
        if (!(e instanceof BackendError) || e.kind !== "not-allowed") throw e;
        const target = hostOf(request.url);
        if (!target || !(await askEgress(app, request))) throw e;
        await backend.egress.allow(target.host, target.port, request.extension, request.purpose);
        return backend.egress.request(request);
      }
    },
  };
}
