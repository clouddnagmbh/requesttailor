// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `LintApi` of the web app (wave 18, strand F), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/lint_js.rs). The lint works on the model in memory: it reads nothing
// from the folder and sends nothing.

import type { LintApi, LintReport, LintRule, LintSettings } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The lint part of rt-wasm's `Workspace`. */
export interface CoreLintWorkspace {
  lintRun(): LintReport;
  lintSettings(): LintSettings;
  lintSetSettings(settings: LintSettings): void;
  lintIgnore(path: string, rule: LintRule): void;
}

export function webLint(session: () => Session): LintApi {
  const run = async <T>(f: (ws: CoreLintWorkspace) => T): Promise<T> => {
    try {
      return f(session().ws as unknown as CoreLintWorkspace);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    run: () => run((ws) => ws.lintRun()),
    settings: () => run((ws) => ws.lintSettings()),
    setSettings: (settings) => run((ws) => ws.lintSetSettings(settings)),
    ignore: (path, rule) => run((ws) => ws.lintIgnore(path, rule)),
  };
}
