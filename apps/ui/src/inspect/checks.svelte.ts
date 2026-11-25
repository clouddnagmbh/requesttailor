// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: the checks of a response, asked once per response and shared by the chip and
// the tab. Switching a rule off bumps the version, so both ask again.

import type { StoredResponse } from "../commands/types";
import type { Observation } from "../commands/types";
import type { AppState } from "../state/app.svelte";

let version = $state(0);
const cache = new Map<string, Promise<Observation[]>>();
const MAX = 50;

/** Changes whenever the answers may have changed; read it inside an `$effect`. */
export function checksVersion(): number {
  return version;
}

/** Forgets every answer (a rule was switched off). */
export function resetChecks(): void {
  cache.clear();
  version++;
}

/** The observations of `response`; none if the backend says no (the checks never block). */
export function loadChecks(app: AppState, requestPath: string, response: StoredResponse): Promise<Observation[]> {
  const id = response.response_id;
  if (id == null) return Promise.resolve([]);
  const key = `${app.collection?.id ?? ""}\u0000${id}`;
  let found = cache.get(key);
  if (!found) {
    found = app.backend.inspect
      .checks({ path: requestPath, environment: app.environment, response_id: id })
      .catch(() => [] as Observation[]);
    if (cache.size >= MAX) cache.delete(cache.keys().next().value as string);
    cache.set(key, found);
  }
  return found;
}
