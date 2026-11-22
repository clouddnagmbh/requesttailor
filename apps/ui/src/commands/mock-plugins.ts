// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Plugins in the mock backend (wave 5, strand A owns this file; strand B adds the example
// plugin's tab replies for its UI tests). Interface commit: nothing installed.

import { BackendError, type PluginApi } from "./backend";

export function mockPlugins(): PluginApi {
  const none = (id: string) => new BackendError("plugin", `plugin ${id} is not installed`);
  return {
    list: async () => [],
    inspect: async () => {
      throw new BackendError("plugin", "not a RequestTailor plugin: the mock installs none yet");
    },
    install: async () => {
      throw new BackendError("plugin", "not a RequestTailor plugin: the mock installs none yet");
    },
    remove: async (id) => {
      throw none(id);
    },
    setEnabled: async (id) => {
      throw none(id);
    },
    renderTab: async (plugin) => {
      throw none(plugin);
    },
    action: async (plugin) => {
      throw none(plugin);
    },
  };
}
