// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SnapshotApi` of the web app (wave 17, strand B), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/snapshots_js.rs). The kept response still has the secret values of its
// send, so freezing and checking first ask the core for the response's environment and pass
// that environment's secret values (only to mask them; the browser's encrypted store).

import type { FreezeOptions, Snapshot, SnapshotApi, SnapshotCheck, SnapshotEntry, SnapshotFreeze } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The snapshot part of rt-wasm's `Workspace`. */
export interface CoreSnapshotWorkspace {
  snapshotsEnvironment(responseId: number): string | undefined;
  snapshotsRead(path: string): Snapshot | null;
  snapshotsFreeze(path: string, responseId: number, options: FreezeOptions, secrets: string[]): SnapshotFreeze;
  snapshotsSetIgnore(path: string, ignore: string[]): Snapshot;
  snapshotsCheck(path: string, responseId: number, secrets: string[]): SnapshotCheck;
  snapshotsList(): SnapshotEntry[];
}

/** The values of the enabled secret variables of `environment` (`null`: none). */
export type SecretValues = (session: Session, environment: string | null) => Promise<string[]>;

export function webSnapshots(session: () => Session, secretValues: SecretValues): SnapshotApi {
  const run = async <T>(f: (ws: CoreSnapshotWorkspace, s: Session) => T | Promise<T>): Promise<T> => {
    try {
      const s = session();
      return await f(s.ws as unknown as CoreSnapshotWorkspace, s);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  const secretsOf = (ws: CoreSnapshotWorkspace, s: Session, responseId: number) =>
    secretValues(s, ws.snapshotsEnvironment(responseId) ?? null);
  return {
    read: (path) => run((ws) => ws.snapshotsRead(path)),
    freeze: (path, responseId, options) =>
      run(async (ws, s) => ws.snapshotsFreeze(path, responseId, options ?? {}, await secretsOf(ws, s, responseId))),
    setIgnore: (path, ignore) => run((ws) => ws.snapshotsSetIgnore(path, ignore)),
    check: (path, responseId) =>
      run(async (ws, s) => ws.snapshotsCheck(path, responseId, await secretsOf(ws, s, responseId))),
    list: () => run((ws) => ws.snapshotsList()),
  };
}
