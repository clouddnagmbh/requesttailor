// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Extensions of the app (wave 9): how a distribution adds its own commands, header items and
// start-up work to the shared UI without changing it. The builds of this repository pass none.
// Extensions are compiled into a distribution; plugins (WASM, `../plugins`) stay the way to
// extend the app at run time.

import type { Component } from "svelte";

import type { Backend } from "./commands/backend";
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
}

export interface StartOptions {
  extensions?: readonly AppExtension[];
}
