// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The services object the web app passes with every send to rt-wasm's `Workspace.send`
// (crates/rt-wasm/src/services_js.rs reads it). Each field comes from its strand's own
// module: `plugins` (wave 5 strand A, plugins.ts), `auth` (strand C, auth.ts), `cookies`
// (strand D, cookies.ts). This file only puts them together.

import type { WebAuthApi } from "./auth";
import type { WebCookieApi } from "./cookies";
import type { WebPluginApi } from "./plugins";

export interface CoreSendServices {
  plugins?: unknown;
  auth?: unknown;
  cookies?: unknown;
}

export interface SendServiceParts {
  plugins: WebPluginApi;
  auth: WebAuthApi;
  cookies: WebCookieApi;
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
  return { plugins, auth, cookies };
}
