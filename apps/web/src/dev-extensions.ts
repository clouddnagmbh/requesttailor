// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A demo extension for the dev server only (`npm run dev`, `?backend=mock&extensions=demo`): it
// shows hints, a node chip, a dialog and the tab openers of the extension points of wave 14, so
// they can be tried and tested. `main.ts` imports this file only under `import.meta.env.DEV`;
// no build contains it (`scripts/check-public-build.mjs` and `dev-extensions.test.ts`).

import type { AppExtension } from "@requesttailor/ui";

import DemoDialog from "./dev-extensions/DemoDialog.svelte";
import DemoTab from "./dev-extensions/DemoTab.svelte";

export const demoExtension: AppExtension = {
  id: "demo",
  hints: (_ctx, target) =>
    target.place === "request"
      ? [
          {
            id: "request-tip",
            text: "Demo hint: this request can be opened in the demo tab.",
            actions: [{ label: "Open demo tab", run: () => _ctx.app.openRequestTab("demo/panel") }],
          },
        ]
      : [{ id: "collection-tip", text: "Demo hint for this collection.", scope: "target" }],
  nodeActions: (ctx, target) =>
    target.kind === "folder"
      ? [{ id: "open", label: "Demo", inline: true, run: () => ctx.app.openCollectionTab("demo/overview") }]
      : [],
  tabs: () => [
    { id: "panel", title: "Demo", place: "request", component: DemoTab },
    { id: "overview", title: "Demo overview", place: "collection", component: DemoTab },
  ],
  commands: (ctx) => [
    { id: "overview", label: "Demo: open overview", run: () => ctx.app.openCollectionTab("demo/overview") },
    {
      id: "dialog",
      label: "Demo: show dialog",
      run: async () => {
        const result = await ctx.app.showDialog({ title: "Demo dialog", component: DemoDialog, props: { message: "Who are you?" }, width: 420 });
        (window as unknown as { __demoResult?: unknown }).__demoResult = result;
      },
    },
  ],
};
