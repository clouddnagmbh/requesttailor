// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `OpenApiApi` of the web app, over rt-wasm's OpenAPI functions
// (crates/rt-wasm/src/openapi_js.rs): the same core steps as the desktop app.
//
// - `fetch`: with the browser's `fetch` unless the companion is on; when the server does not
//   allow the browser to read it (CORS) and the companion is paired, again through
//   `rt agent`; otherwise the error says how to get the spec in.
// - `plan`: kept by the import's core importer (`import.ts`), written like any plan.
// - `requirements` / `validateResponse`: the browser reads asynchronously, the core
//   synchronously, so this file reads the spec's saved files first (the path
//   `opencollection.yml` names, else `openapi.{yaml,yml,json}`) and hands their bytes over.
//   A spec the wizard just kept is staged in the workspace and read from there by the core.

import {
  BackendError,
  type FetchedSpec,
  type ImportSource,
  type OpenApiApi,
  type ResponseCheck,
  type SendInput,
  type SendRequirements,
  type SpecOverview,
} from "@requesttailor/ui";

import { directoryFs } from "./git/fs";
import type { WebImports } from "./import";
import { toBackendError, type CompanionCredentials, type CoreModule, type CoreSendInput } from "./wasm";
import type { WebHost } from "./web-backend";

/** What rt-wasm exports for the wizard (`openapi_js.rs`). */
export interface CoreOpenApiModule {
  openapiFetch(url: string, companion: CompanionCredentials | null): Promise<FetchedSpec>;
  openapiInspect(files: ImportSource[]): SpecOverview;
}

/** The `Workspace` methods of `openapi_js.rs`. */
interface OpenApiWorkspace {
  openapiSpecPath(): string | null | undefined;
  openapiRequirements(path: string, input: CoreSendInput, specs: [string, Uint8Array][]): SendRequirements | null | undefined;
  openapiValidateResponse(path: string, responseId: number, specs: [string, Uint8Array][]): ResponseCheck | null | undefined;
}

/** Looked for, in this order, when `opencollection.yml` names no spec (`SPEC_CANDIDATES`). */
const CANDIDATES = ["openapi.yaml", "openapi.yml", "openapi.json"];

export const CORS_HINT =
  "The server does not let this browser read the spec (CORS). Pair the companion (run `rt agent` on this computer) to fetch it, or download the file and drop it here.";

export interface WebOpenApiDeps {
  host: WebHost;
  imports: WebImports;
  /** The core's send input as a send gets it (session variables, secret values). */
  coreInput(input: SendInput): Promise<CoreSendInput>;
  /** Whether sending goes through the companion. */
  companionOn(): boolean;
}

/** A browser refusal (`fetch` rejected: CORS, mixed content, offline) rather than an HTTP
 *  status from the server. */
export function looksBlocked(message: string): boolean {
  return /failed to fetch|networkerror|load failed|blocked|cors/i.test(message) && !/\bHTTP \d{3}\b|status \d{3}/i.test(message);
}

export function webOpenApi(deps: WebOpenApiDeps): OpenApiApi {
  const core = async () => (await deps.host.deps.loadCore()) as CoreModule & CoreOpenApiModule;

  async function specs(ws: OpenApiWorkspace): Promise<[string, Uint8Array][]> {
    const configured = ws.openapiSpecPath() ?? null; // wasm-bindgen gives `undefined` for `None`
    const paths = configured !== null ? [configured.replace(/^\/+/, "")] : CANDIDATES;
    const { promises: fs } = directoryFs(deps.host.session().handle);
    const out: [string, Uint8Array][] = [];
    for (const path of paths) {
      if (path === "" || path.split("/").includes("..")) continue;
      try {
        out.push([path, (await fs.readFile(path)) as Uint8Array]);
        if (configured === null) break;
      } catch {
        // Missing (or a folder): the core treats it as no file.
      }
    }
    return out;
  }

  async function guard<T>(f: () => Promise<T>): Promise<T> {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  }

  return {
    fetch: (url) =>
      guard(async () => {
        const c = await core();
        if (deps.companionOn()) return c.openapiFetch(url, await deps.host.companionCredentials());
        try {
          return await c.openapiFetch(url, null);
        } catch (e) {
          const err = toBackendError(e);
          if (!looksBlocked(err.message)) throw err;
          const companion = await deps.host.companionCredentials();
          if (companion) return c.openapiFetch(url, companion);
          throw new BackendError(err.kind, `${err.message}. ${CORS_HINT}`);
        }
      }),

    inspect: (files) => guard(async () => (await core()).openapiInspect(files)),

    plan: (files, options) => deps.imports.openapiPlan(files, options),

    requirements: (path, input) =>
      guard(async () => {
        await core();
        const ws = deps.host.session().ws as unknown as OpenApiWorkspace;
        const read = await specs(ws);
        return ws.openapiRequirements(path, await deps.coreInput(input), read) ?? null;
      }),

    validateResponse: (path, responseId) =>
      guard(async () => {
        await core();
        const ws = deps.host.session().ws as unknown as OpenApiWorkspace;
        return ws.openapiValidateResponse(path, responseId, await specs(ws)) ?? null;
      }),
  };
}
