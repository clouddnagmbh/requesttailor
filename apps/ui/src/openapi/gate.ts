// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The run wizard's place in sending: `AppState.sendGate` asks the core what the request still
// needs (`OpenApiApi.requirements`, matched against the collection's spec); when something is
// missing or invalid, "Ready to send?" opens instead of sending into an error. "Send anyway"
// and "Don't ask for this request" (remembered on this device) are always there; Mod+Enter
// while it is open equals "Fill and send" (the shell's shortcut calls `send` again, and the
// gate hands it to the open panel).

import type { SendRequirements } from "../commands/types";
import type { AppState } from "../state/app.svelte";

const SKIP_KEY = "requesttailor.openapi.dont-ask";
/** Choices made while storage is blocked. */
const memory = new Set<string>();

function readSkips(): string[] {
  try {
    const raw = localStorage.getItem(SKIP_KEY);
    const list: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

const skipKey = (collection: string, path: string) => `${collection}\u0000${path}`;

/** "Don't ask for this request" was chosen on this device. */
export function isSkipped(collection: string, path: string): boolean {
  const key = skipKey(collection, path);
  return memory.has(key) || readSkips().includes(key);
}

export function setSkipped(collection: string, path: string, skip: boolean): void {
  const key = skipKey(collection, path);
  if (!skip) memory.delete(key);
  const list = readSkips().filter((k) => k !== key);
  if (skip) list.push(key);
  try {
    localStorage.setItem(SKIP_KEY, JSON.stringify(list.slice(-500)));
  } catch {
    // Storage blocked: the choice lasts for this session only.
    if (skip) memory.add(key);
  }
}

/** What the panel resolves with. */
export type GateResult = "send" | "cancel";

/** The open panel, so a second `send` (Mod+Enter) can submit it. */
export interface GateController {
  submit?: () => void;
}

export type ShowPanel = (path: string, requirements: SendRequirements, controller: GateController) => Promise<GateResult>;

/** Installs the gate on `app`; returns the uninstaller. */
export function installSendGate(app: AppState, show: ShowPanel): () => void {
  let open: { path: string; controller: GateController } | null = null;
  const gate = async (path: string): Promise<boolean> => {
    if (open) {
      if (open.path === path) open.controller.submit?.();
      return false;
    }
    const collection = app.collection?.id ?? "";
    if (isSkipped(collection, path)) return true;
    let req: SendRequirements | null = null;
    try {
      req = await app.backend.openapi.requirements(path, { environment: app.environment, vars: [] });
    } catch {
      return true;
    }
    if (!req || req.ready) return true;
    const controller: GateController = {};
    open = { path, controller };
    app.sending[path] = false;
    try {
      const result = await show(path, req, controller);
      if (result === "send") app.sending[path] = true;
      return result === "send";
    } finally {
      open = null;
    }
  };
  app.sendGate = gate;
  return () => {
    if (app.sendGate === gate) app.sendGate = null;
  };
}
