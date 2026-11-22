// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The services object the web app passes with every send to rt-wasm's `Workspace.send`
// (crates/rt-wasm/src/services_js.rs reads it). Each field comes from its strand's own
// module: `plugins` (wave 5 strand A, plugins.ts), `auth` (strand C, auth.ts), `cookies`
// (strand D, cookies.ts), `scripts` (wave 7 strand A, scripts.ts: trusted or not). This file
// only puts them together.

import type { GuardVerdict, SendCheck } from "@requesttailor/ui";

import type { WebAuthApi } from "./auth";
import type { WebCookieApi } from "./cookies";
import type { WebPluginApi } from "./plugins";
import type { WebScriptTrust } from "./scripts";

export interface CoreSendServices {
  plugins?: unknown;
  auth?: unknown;
  cookies?: unknown;
  scripts?: boolean;
  /** Wave 10: the distribution's send guard (crates/rt-wasm/src/guard_js.rs). */
  guard?: SendGuardFn;
}

/** A distribution's send guard: synchronous, asked before each request goes out. A guard that
 *  throws or answers something else refuses the request. */
export type SendGuardFn = (check: SendCheck) => GuardVerdict;

export interface SendServiceParts {
  plugins: WebPluginApi;
  auth: WebAuthApi;
  cookies: WebCookieApi;
  scriptTrust: Pick<WebScriptTrust, "forSend">;
  /** Wave 10, optional. */
  sendGuard?: SendGuardFn;
}

/** The services for one send from `collectionId` with `environment`. */
export async function sendServices(
  parts: SendServiceParts,
  collectionId: string,
  environment: string | null,
): Promise<CoreSendServices> {
  const [plugins, auth, cookies] = await Promise.all([
    parts.plugins.forSend(collectionId, environment),
    parts.auth.forSend(collectionId, environment),
    parts.cookies.forSend(collectionId, environment),
  ]);
  const services: CoreSendServices = {
    plugins,
    auth,
    cookies,
    scripts: parts.scriptTrust.forSend(collectionId),
  };
  if (parts.sendGuard) services.guard = parts.sendGuard;
  return services;
}
