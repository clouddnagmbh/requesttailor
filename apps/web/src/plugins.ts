// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PluginApi` of the web app (wave 5, strand A): plugin modules in browser storage, run by
// rt-plugin's host inside rt-wasm (the same interpreter as natively). Interface commit:
// nothing installed.

import { BackendError, type PluginApi } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

/** `PluginApi` plus the plugin part of a send (the `plugins` field `services_js` reads). */
export interface WebPluginApi extends PluginApi {
  forSend(collectionId: string, environment: string | null): Promise<unknown>;
}

export function webPlugins(_host: WebHost): WebPluginApi {
  const notYet = () => new BackendError("unsupported", "plugins are not available yet");
  return {
    list: async () => [],
    inspect: async () => {
      throw notYet();
    },
    install: async () => {
      throw notYet();
    },
    remove: async () => {
      throw notYet();
    },
    setEnabled: async () => {
      throw notYet();
    },
    renderTab: async () => {
      throw notYet();
    },
    action: async () => {
      throw notYet();
    },
    forSend: async () => undefined,
  };
}
