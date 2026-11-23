// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Test page of the "New…" dialog (`harness.html`): the mock backend, a test extension with one
// tile (with a badge), and `collections.create` answered here, since the mock has a single
// collection until several collections arrive. Calls are recorded on `window.__rt`.

import { MockBackend, startApp, type AppExtension, type NewCollection, type NewTarget } from "@requesttailor/ui";

const backend = new MockBackend();
const calls: { created: NewCollection[]; ran: NewTarget[] } = { created: [], ran: [] };

backend.collections.create = async (spec: NewCollection) => {
  calls.created.push(spec);
  const info = await backend.openCollection();
  return info && { ...info, name: spec.name };
};

const extension: AppExtension = {
  id: "test",
  newItems: () => [
    {
      id: "thing",
      title: "Test thing",
      description: "A tile from an extension.",
      group: "Extras",
      icon: "spark",
      badge: "Beta",
      run: (_ctx, target) => {
        calls.ran.push(target);
      },
    },
  ],
};

(window as unknown as { __rt: unknown }).__rt = { backend, calls };

startApp(document.getElementById("app")!, backend, { extensions: [extension] });
