// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A demo extension for the dev server only (`npm run dev`, `?backend=mock&extensions=demo`): it
// shows hints, a node chip, a dialog and the tab openers of the extension points of wave 14, so
// they can be tried and tested. `main.ts` imports this file only under `import.meta.env.DEV`;
// no build contains it (`scripts/check-public-build.mjs` and `dev-extensions.test.ts`).

import type { AppExtension, ExtensionContext } from "@requesttailor/ui";

import DemoDialog from "./dev-extensions/DemoDialog.svelte";
import Demo16Panel from "./dev-extensions/Demo16Panel.svelte";
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
    {
      id: "ask-ai",
      label: "Demo: ask the AI",
      run: async () => {
        const w = window as unknown as { __demoAi?: unknown };
        try {
          w.__demoAi = await ctx.ai.complete({
            purpose: "Demo",
            system: "You are a demo.",
            messages: [{ role: "user", content: `Say hi. token=${(window as unknown as { __demoToken?: string }).__demoToken ?? "sk-demo-secret"}` }],
          });
        } catch (e) {
          w.__demoAi = { error: (e as { kind?: string }).kind ?? String(e) };
        }
      },
    },
  ],
};

// ---- Wave 16: editor, request, response, run and range actions, suggestions and the panel ----
// `?backend=mock&extensions=demo16`. Every action leaves a line in `window.__demo16` (for the
// tests); no model is called.

type Demo16Window = { __demo16?: string[] };
const log = (line: string) => ((window as unknown as Demo16Window).__demo16 ??= []).push(line);

async function suggestHeader(ctx: ExtensionContext, path: string) {
  const spec = await ctx.backend.getRequest(path);
  const applied = await ctx.suggest({
    kind: "request",
    path,
    spec: { ...spec, headers: [...spec.headers, { name: "X-Request-Id", value: "{{requestId}}", enabled: true }] },
    note: { title: "Trace this request", explanation: "Adds `X-Request-Id` so the call can be found in the server log." },
  });
  log(`suggest-request:${applied}`);
}

export const demo16Extension: AppExtension = {
  id: "demo16",
  editorActions: (_ctx, target) =>
    target.kind === "script"
      ? [
          {
            id: "log",
            label: "Add logging",
            icon: "spark",
            inline: true,
            shortcut: "Mod-Shift-l",
            run: async (editor) => {
              const before = editor.text().replace(/\n$/, "");
              const next = `${before ? `${before}\n` : ""}console.log("done");\n`;
              log(`propose-script:${await editor.propose(next, { title: "Log when the script ends", explanation: "One `console.log` at the end, so the **Console** tab shows it ran." })}`);
            },
          },
          {
            id: "wrap",
            label: "Wrap in try/catch",
            run: async (editor) => {
              log(`propose-wrap:${await editor.propose(`try {\n${editor.text()}\n} catch (e) {\n  console.error(e);\n}\n`)}`);
            },
          },
        ]
      : target.kind === "body"
        ? [{ id: "compact", label: "Compact JSON", icon: "json", inline: true, run: async (editor) => void log(`propose-body:${await editor.propose(JSON.stringify(JSON.parse(editor.text() || "{}")))}`) }]
        : [],
  requestActions: (ctx, target) => [
    { id: "header", label: "Suggest a header", icon: "spark", inline: true, run: () => suggestHeader(ctx, target.path) },
    {
      id: "new",
      label: "Suggest a health check",
      icon: "add",
      run: async () =>
        log(
          `suggest-new:${await ctx.suggest({ kind: "new-request", folder: "", spec: { ...target.spec, name: "Health check", url: "{{baseUrl}}/health" }, note: { title: "A request to check the service is up" } })}`,
        ),
    },
    {
      id: "file",
      label: "Suggest release notes",
      icon: "file-text",
      run: async () => log(`suggest-file:${await ctx.suggest({ kind: "file", path: "RELEASE-NOTES.md", text: "# Release notes\n\n- Added a trace header.\n" })}`),
    },
  ],
  responseActions: (_ctx, target) => [
    { id: "explain", label: target.failed ? "Explain the failure" : "Explain the response", icon: "help", inline: target.failed, run: () => void log(`response:${target.path}:${target.failed}`) },
  ],
  runActions: (_ctx, target) => [
    { id: "why", label: target.kind === "bisect" ? "Why did it break?" : "Summarise the run", icon: "bisect", inline: true, run: () => void log(`run:${target.kind}`) },
  ],
  rangeActions: (_ctx, target) => [
    { id: "notes", label: "Release notes", icon: "tag", inline: true, run: () => void log(`range:${target.source}:${target.from}..${target.to}`) },
  ],
  assistant: { title: "Assistant", icon: "spark", component: Demo16Panel },
};
