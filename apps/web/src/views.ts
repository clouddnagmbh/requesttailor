// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ViewsApi` of the web app (wave 18, strand G), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/views_js.rs). The views file is a companion file staged like any edit; a
// view is evaluated in the core on the kept response.

import type { ResponseView, ViewLanguage, ViewResult, ViewsApi } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The views part of rt-wasm's `Workspace`. */
export interface CoreViewsWorkspace {
  viewsList(path: string): ResponseView[];
  viewsSave(path: string, view: ResponseView): ResponseView[];
  viewsRemove(path: string, name: string): ResponseView[];
  viewsApply(responseId: number, view: ResponseView): ViewResult;
  viewsLanguages(): ViewLanguage[];
}

export function webViews(session: () => Session): ViewsApi {
  const run = async <T>(f: (ws: CoreViewsWorkspace) => T): Promise<T> => {
    try {
      return f(session().ws as unknown as CoreViewsWorkspace);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    list: (path) => run((ws) => ws.viewsList(path)),
    save: (path, view) => run((ws) => ws.viewsSave(path, view)),
    remove: (path, name) => run((ws) => ws.viewsRemove(path, name)),
    apply: (_path, responseId, view) => run((ws) => ws.viewsApply(responseId, view)),
    languages: () => run((ws) => ws.viewsLanguages()),
  };
}
