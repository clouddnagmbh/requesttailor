// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the app (wave 9): how a distribution adds its own commands, header items and
// start-up work to the shared UI without changing it. The builds of this repository pass none.
// Extensions are compiled into a distribution; plugins (WASM, `../plugins`) stay the way to
// extend the app at run time.

import type { Component } from "svelte";

import type { Backend } from "./commands/backend";
import type { StoredResponse } from "./commands/types";
import type { AppState } from "./state/app.svelte";

/** What an extension sees: the backend the app was started with and the shared app state. */
export interface ExtensionContext {
  readonly backend: Backend;
  readonly app: AppState;
}

/** A command in the palette. */
export interface ExtensionCommand {
  /** Unique across extensions; prefixed with the extension's id by the shell. */
  id: string;
  label: string;
  run: () => unknown;
}

/** Where an extension tab is shown: among the request editor's tabs, among the response's tabs,
 *  or in the collection panel of the main area (header button "Collection", palette). */
export type ExtensionTabPlace = "request" | "response" | "collection";

/** What an extension tab component gets. */
export interface ExtensionTabProps {
  ctx: ExtensionContext;
  /** Path of the request the tab belongs to; `""` for collection tabs. */
  requestPath: string;
  /** Response tabs: the response shown (`response_id` for `backend.responseBytes`); else `null`. */
  response: StoredResponse | null;
  /** Hands the request editor's pending typing to the backend. Await it before reading the
   *  request (`backend.getRequest`) or replacing it (`app.edit`). Resolves at once outside
   *  request tabs. */
  settle: () => Promise<void>;
}

/** A tab rendered by the app like a plugin's tab, but as a Svelte component of a distribution. */
export interface ExtensionTab {
  /** Unique within the extension; the shell keys the tab `<extension id>/<tab id>`. */
  id: string;
  title: string;
  place: ExtensionTabPlace;
  /** Response tabs only: media types the tab is for, matched like a plugin tab's
   *  `content_types` (`application/json`, `application/*`; `application/xml` also matches
   *  `application/atom+xml`). Absent or empty: every response. */
  contentTypes?: readonly string[];
  component: Component<ExtensionTabProps>;
}

export interface AppExtension {
  /** Stable id, e.g. `example`; also the prefix of its command ids (`example:…`). */
  readonly id: string;
  /**
   * Palette commands. Called inside a reactive scope, so it may read `ctx.app` and return
   * different commands as the state changes.
   */
  commands?(ctx: ExtensionContext): readonly ExtensionCommand[];
  /** Rendered at the end of the header, before the palette button, in the order given. */
  header?: Component<{ ctx: ExtensionContext }>;
  /** Rendered once at the end of the shell (dialogs, banners); shown or hidden by itself. */
  overlay?: Component<{ ctx: ExtensionContext }>;
  /** Runs once after the first frame; errors are logged, never shown. */
  start?(ctx: ExtensionContext): void | Promise<void>;
  /**
   * Request, response and collection tabs (since wave 13). Called inside a reactive scope like
   * `commands`, so an extension can show tabs only while it is allowed to. Collection tabs
   * get palette entries ("Collection: <title>") like plugin collection tabs.
   */
  tabs?(ctx: ExtensionContext): readonly ExtensionTab[];
}

export interface StartOptions {
  extensions?: readonly AppExtension[];
}
