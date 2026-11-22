// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The services object the web app passes with every send to rt-wasm's `Workspace.send`
// (crates/rt-wasm/src/services_js.rs reads it). Each wave 5 strand fills its own field from
// its own module: `plugins` (strand A, plugins.ts), `auth` (strand C, auth.ts), `cookies`
// (strand D, cookies.ts). Interface commit: all empty.

export interface CoreSendServices {
  plugins?: unknown;
  auth?: unknown;
  cookies?: unknown;
}

/** The services for one send from `collectionId` with `environment`. */
export async function sendServices(collectionId: string, environment: string | null): Promise<CoreSendServices> {
  void collectionId;
  void environment;
  return {};
}
