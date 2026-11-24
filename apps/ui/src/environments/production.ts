// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: what the app does about a production environment. The rules here are the
// twin of `rt_app::agents::is_production` (the policy flag of a listed environment, else the
// name) and of the confirmation only for writing requests; whether a request writes is asked
// of the core (`Backend.agents.check`), never worked out in TypeScript.

import type { AgentPolicy, Effect } from "../commands/types";
import { load, store } from "../shell/storage";
import { isProductionName } from "./ops";

/** Production: the policy's `production` flag when it lists `environment`, else the name
 *  (`prod`, `prd` or the word `live`). Sending without an environment is never production. */
export function isProduction(policy: AgentPolicy | null, environment: string | null): boolean {
  if (!environment) return false;
  const entry = policy?.environments.find((e) => e.environment === environment);
  return entry ? entry.production : isProductionName(environment);
}

export function sendLabel(production: boolean, sending: boolean): string {
  if (sending) return "Sending…";
  return production ? "Send to PRODUCTION" : "Send";
}

export interface ConfirmInfo {
  method: string;
  /** The URL as written (variables not resolved), so no secret value can be in it. */
  url: string;
  environment: string;
}

export function confirmQuestion(info: ConfirmInfo): string {
  return `Send ${info.method} to ${info.environment}?`;
}

/** The confirmation is asked for a writing request in production while it is on. Reading
 *  requests are never asked. */
export function shouldConfirm(input: { production: boolean; effect: Effect; enabled: boolean }): boolean {
  return input.production && input.effect === "write" && input.enabled;
}

const CONFIRM_KEY = "production-confirm";
const isBool = (v: unknown): v is boolean => typeof v === "boolean";

/** Per device; on by default. */
export function confirmEnabled(): boolean {
  return load(CONFIRM_KEY, true, isBool);
}

export function setConfirmEnabled(on: boolean): void {
  store(CONFIRM_KEY, on ? undefined : false);
}
