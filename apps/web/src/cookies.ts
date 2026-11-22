// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CookieApi` of the web app (wave 5, strand D). The browser's `fetch` keeps its own cookies
// and forbids the `Cookie` header, so the app's jar applies only to sends through the
// companion. Interface commit: an empty jar.

import type { CookieApi } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

/** `CookieApi` plus the jar of a send (the `cookies` field `services_js` reads). */
export interface WebCookieApi extends CookieApi {
  forSend(collectionId: string, environment: string | null): Promise<unknown>;
}

export function webCookies(_host: WebHost): WebCookieApi {
  return {
    list: async () => [],
    delete: async () => {},
    clear: async () => {},
    forSend: async () => undefined,
  };
}
