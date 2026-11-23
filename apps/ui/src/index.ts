// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Public entry of the shared UI. Shells (apps/web, apps/desktop) import only from here.

import { mount } from "svelte";

import type { Backend } from "./commands/backend";
import type { StartOptions } from "./extensions";
import AppShell from "./shell/AppShell.svelte";
import "./styles/tokens.css";

export type * from "./commands/backend";
export type * from "./commands/types";
export { BackendError } from "./commands/backend";
export { LOCAL_CLONE_UNSUPPORTED, looksLocal } from "./commands/clone-source";
export { MockBackend } from "./commands/mock";
// Wave 12 interface: placeholders the shells use until each strand fills in its own file.
export * from "./commands/wave12-stubs";
// Wave 14 interface: the collection set over the single open collection (strand A replaces it).
export * from "./commands/wave14-stubs";

export type * from "./extensions";

/**
 * Mounts the app into `target` with the given backend. A distribution passes its extensions in
 * `options` (wave 9, `./extensions`); the builds of this repository pass none.
 */
export function startApp(target: HTMLElement, backend: Backend, options: StartOptions = {}) {
  return mount(AppShell, { target, props: { backend, extensions: options.extensions ?? [] } });
}
