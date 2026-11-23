// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the app (wave 9): how a distribution adds its own commands, header items and
// start-up work to the shared UI without changing it. The builds of this repository pass none.
// Extensions are compiled into a distribution; plugins (WASM, `../plugins`) stay the way to
// extend the app at run time.

import type { Component } from "svelte";

import type { Backend } from "./commands/backend";
import type { RequestSpec, StoredResponse } from "./commands/types";
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

/** What a diff-notes component gets: the two revisions of a range (since wave 13). */
export interface ExtensionDiffNotesProps {
  ctx: ExtensionContext;
  /** Old side (a revision: branch, tag, commit). */
  from: string;
  /** New side. */
  to: string;
}

/** Where the "New…" dialog was opened for (since wave 14): the collection and folder its forms
 *  start with. `collection: null` when no collection is open. */
export interface NewTarget {
  collection: string | null;
  /** Folder path in that collection, `""` for its root. */
  folder: string;
}

/** A tile in the "New…" dialog (since wave 14), after the built-in ones. */
export interface ExtensionNewItem {
  /** Unique within the extension; the dialog keys the tile `<extension id>/<item id>`. */
  id: string;
  title: string;
  /** One short sentence under the title. */
  description: string;
  /** Group heading the tile appears under; tiles without one go under "More". Groups appear
   *  after the built-in ones, in the order they first occur. */
  group?: string;
  /** An icon of the dialog's set (`new/icons.ts`: `http`, `graphql`, `grpc`, `websocket`,
   *  `sse`, `collection`, `folder`, `environment`, `mcp`, `import`, `service`, `diagram`,
   *  `trace`, `spark`); unknown or absent: a generic tile icon. */
  icon?: string;
  /** A short word shown on the tile (at most 12 characters). */
  badge?: string;
  /** Runs after the dialog closed. */
  run(ctx: ExtensionContext, target: NewTarget): unknown;
}

/** What a hint is about (since wave 14). */
export type HintTarget =
  /** Above the request editor of `path` in collection `collection`. */
  | { place: "request"; collection: string; path: string; spec: RequestSpec }
  /** At the top of an expanded collection node in the sidebar. */
  | { place: "collection"; collection: string };

/** A small, calm note the shell shows in a defined place (since wave 14): never a modal, never
 *  blocking, always dismissible. The shell shows at most one hint per place at a time (the
 *  first not dismissed, extensions in order) and remembers a dismissal per device. */
export interface ExtensionHint {
  /** Unique within the extension; dismissals are stored under `<extension id>/<hint id>`. */
  id: string;
  /** One or two short sentences, plain text. */
  text: string;
  /** Up to two buttons; running one does not dismiss the hint unless `dismiss` is set. */
  actions?: readonly { label: string; run: () => unknown; dismiss?: boolean }[];
  /** `device` (default): once dismissed, never again on this device; `target`: dismissed for
   *  this collection or request only. */
  scope?: "device" | "target";
}

/** A collection or folder node of the sidebar (since wave 14). */
export type NodeTarget =
  | { kind: "collection"; collection: string }
  | { kind: "folder"; collection: string; path: string };

/** An entry in a collection or folder node's context menu (since wave 14). */
export interface ExtensionNodeAction {
  /** Unique within the extension. */
  id: string;
  label: string;
  /** Also shown as a small chip on the row itself (at most one per row: the first). */
  inline?: boolean;
  /** An icon of the "New…" dialog's set, for the chip. */
  icon?: string;
  run: () => unknown;
}

/** What a dialog component of an extension gets (since wave 14, `AppState.showDialog`). */
export interface ExtensionDialogProps {
  ctx: ExtensionContext;
  /** Closes the dialog; `showDialog` resolves with `result`. */
  close: (result?: unknown) => void;
  /** The `props` passed to `showDialog`. */
  props: Record<string, unknown>;
}

/** A dialog shown by the shell (since wave 14): title bar, close button, Escape and focus
 *  handling, light and dark theme, centred; one at a time. */
export interface ExtensionDialog {
  /** Title bar text and the dialog's accessible name. */
  title: string;
  component: Component<ExtensionDialogProps>;
  props?: Record<string, unknown>;
  /** Width in CSS pixels (default 560); never wider than the window minus 32 px. */
  width?: number;
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
  /**
   * Notes on a range of revisions (since wave 13), shown after the OpenAPI contract notes in
   * the semantic diff of two revisions and in the pull-request review, e.g. the changes of
   * another description file. Renders nothing when it has nothing to say (or is not allowed
   * to); reads files with `ctx.backend.git.show`/`treeAt`.
   */
  diffNotes?: Component<ExtensionDiffNotesProps>;
  /**
   * Tiles of the "New…" dialog (since wave 14). Called inside a reactive scope like `commands`
   * while the dialog is open; must not write `$state`.
   */
  newItems?(ctx: ExtensionContext): readonly ExtensionNewItem[];
  /**
   * Hints for a place (since wave 14). Called inside a reactive scope for the shown request and
   * for every expanded collection node, so it must be cheap and synchronous (work from data
   * the extension prepared before, e.g. in `start`) and must not write `$state`.
   */
  hints?(ctx: ExtensionContext, target: HintTarget): readonly ExtensionHint[];
  /**
   * Context-menu entries of collection and folder nodes (since wave 14). Called for visible
   * nodes inside a reactive scope: cheap, synchronous, no `$state` writes.
   */
  nodeActions?(ctx: ExtensionContext, target: NodeTarget): readonly ExtensionNodeAction[];
}

export interface StartOptions {
  extensions?: readonly AppExtension[];
}
