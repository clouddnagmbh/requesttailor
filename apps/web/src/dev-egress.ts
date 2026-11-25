// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A demo extension for the dev server only (`?extensions=egress`, with the mock or the real
// backend): one palette command, "Egress demo: send call", that sends `window.__egressRequest`
// through `ExtensionContext.egress` (the one-time question included) and leaves what came back,
// or the error kind, in `window.__egressResult`. `main.ts` imports this file only under
// `import.meta.env.DEV`; no build contains it.

import type { AppExtension, EgressRequest } from "@requesttailor/ui";

interface Slots {
  __egressRequest?: EgressRequest;
  __egressResult?: { ok: true; value: unknown } | { ok: false; kind: string; message: string };
}

export const egressDemoExtension: AppExtension = {
  id: "egress-demo",
  commands: (ctx) => [
    {
      id: "send",
      label: "Egress demo: send call",
      run: async () => {
        const w = window as unknown as Slots;
        const request = w.__egressRequest;
        if (!request) return;
        try {
          w.__egressResult = { ok: true, value: await ctx.egress.request(request) };
        } catch (e) {
          const err = e as { kind?: string; message?: string };
          w.__egressResult = { ok: false, kind: err.kind ?? "shell", message: err.message ?? String(e) };
        }
      },
    },
  ],
};
