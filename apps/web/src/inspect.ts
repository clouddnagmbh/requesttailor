// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InspectApi` of the web app (wave 18, strand B), over rt-wasm (crates/rt-wasm/src/inspect_js.rs).
// Tokens are read from the resolved request and the kept response; the checks look at the
// response the browser already got and send nothing. Rules switched off on this device are a
// list of ids in localStorage (never a value); the collection's list is
// `extensions.requesttailor.checks.ignore` in `opencollection.yml`, staged like any edit.

import type { InspectApi, SendInput } from "@requesttailor/ui";
import type { CheckScope, FoundToken, JwtInfo, Observation } from "@requesttailor/ui";

import { toBackendError, type CoreModule, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** What rt-wasm exports for the checks (`inspect_js.rs`). */
export interface CoreInspectModule {
  inspectJwt(token: string): JwtInfo;
}

/** The checks part of rt-wasm's `Workspace`. */
export interface CoreInspectWorkspace {
  inspectTokens(path: string, input: CoreSendInput, responseId: number | null): FoundToken[];
  inspectChecks(responseId: number, deviceIgnored: string[]): Observation[];
  inspectIgnoreCollection(id: string): void;
  inspectCollectionIgnored(): string[];
}

export interface InspectHost {
  session(): Session;
  core(): Promise<CoreModule>;
  /** The core's send input as a send gets it (session variables, secret values). */
  coreInput(input: SendInput): Promise<CoreSendInput>;
}

export const DEVICE_KEY = "rt.inspect.checks-off";

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The rule ids switched off on this device. */
export function deviceIgnored(): string[] {
  try {
    const raw = storage()?.getItem(DEVICE_KEY);
    const list: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

function ignoreOnDevice(id: string): void {
  const list = deviceIgnored();
  if (list.includes(id)) return;
  try {
    const s = storage();
    if (!s) throw new Error("no storage");
    s.setItem(DEVICE_KEY, JSON.stringify([...list, id]));
  } catch {
    throw toBackendError(new Error("This browser keeps no settings; the rule stays on."));
  }
}

export function webInspect(host: InspectHost): InspectApi {
  const ws = () => host.session().ws as unknown as CoreInspectWorkspace;
  async function guard<T>(f: () => Promise<T> | T): Promise<T> {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  }
  return {
    jwt: (token) => guard(async () => ((await host.core()) as unknown as CoreInspectModule).inspectJwt(token)),
    tokens: (target) =>
      guard(async () => {
        await host.core();
        const input = await host.coreInput({ environment: target.environment ?? null, vars: [] });
        return ws().inspectTokens(target.path, input, target.response_id ?? null);
      }),
    checks: (target) =>
      guard(async () => {
        await host.core();
        return target.response_id == null ? [] : ws().inspectChecks(target.response_id, deviceIgnored());
      }),
    ignoreCheck: (id: string, scope: CheckScope) =>
      guard(async () => {
        if (scope === "device") return ignoreOnDevice(id);
        await host.core();
        ws().inspectIgnoreCollection(id);
      }),
  };
}
