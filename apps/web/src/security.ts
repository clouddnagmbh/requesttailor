// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SecurityApi` of the web app (wave 19, strand R), over rt-wasm (`crates/rt-wasm/src/security_js.rs`).
// The passive rules look at the response the browser already got and send nothing; the browser's
// `fetch` tells nothing about TLS, so the TLS rules are silent here. The settings of this device
// (one flag and rule ids, never a value) are in localStorage; a collection switches rules off in
// `checks.ignore` of `opencollection.yml` (shared with the checks chip and tab).

import type { SecurityApi, SecurityFinding, SecurityRule, SecuritySettings } from "@requesttailor/ui";

import { deviceIgnored } from "./inspect";
import { toBackendError, type CoreModule } from "./wasm";
import type { Session } from "./web-backend";

/** What rt-wasm exports for the security checks (`security_js.rs`). */
export interface CoreSecurityModule {
  securityRules(): SecurityRule[];
  securityIgnoredBy(settings: SecuritySettings): string[];
}

/** The security part of rt-wasm's `Workspace`. */
export interface CoreSecurityWorkspace {
  securityCheck(responseId: number, deviceIgnored: string[], settings: SecuritySettings): SecurityFinding[];
}

export interface SecurityHost {
  session(): Session;
  core(): Promise<CoreModule>;
}

export const SETTINGS_KEY = "rt.security.settings";

const DEFAULTS: SecuritySettings = { enabled: true, disabled: [] };

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The settings of this device; the defaults when storage is missing or odd. */
export function readSettings(): SecuritySettings {
  try {
    const raw = storage()?.getItem(SETTINGS_KEY);
    const value: unknown = raw ? JSON.parse(raw) : null;
    if (value && typeof value === "object") {
      const v = value as { enabled?: unknown; disabled?: unknown };
      return {
        enabled: v.enabled !== false,
        disabled: Array.isArray(v.disabled) ? v.disabled.filter((x): x is string => typeof x === "string") : [],
      };
    }
  } catch {
    // Fall through to the defaults.
  }
  return { ...DEFAULTS, disabled: [] };
}

/**
 * The rule ids the settings of this device switch off, for the checks chip and tab (they run the
 * same rules in one pass). Empty when the core is not loaded yet or the settings leave all on.
 */
export async function securityIgnored(core: () => Promise<CoreModule>): Promise<string[]> {
  const settings = readSettings();
  if (settings.enabled && (settings.disabled ?? []).length === 0) return [];
  return ((await core()) as unknown as CoreSecurityModule).securityIgnoredBy(settings);
}

export function webSecurity(host: SecurityHost): SecurityApi {
  const ws = () => host.session().ws as unknown as CoreSecurityWorkspace;
  async function guard<T>(f: () => Promise<T> | T): Promise<T> {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  }
  return {
    check: (target) =>
      guard(async () => {
        await host.core();
        return ws().securityCheck(target.response_id, deviceIgnored(), readSettings());
      }),
    rules: () => guard(async () => ((await host.core()) as unknown as CoreSecurityModule).securityRules()),
    settings: () => guard(() => readSettings()),
    setSettings: (settings) =>
      guard(() => {
        const s = storage();
        if (!s) throw new Error("This browser keeps no settings; the rules stay on.");
        s.setItem(SETTINGS_KEY, JSON.stringify({ enabled: settings.enabled !== false, disabled: settings.disabled ?? [] }));
      }),
  };
}
