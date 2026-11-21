// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Public entry of the shared UI. Shells (apps/web, apps/desktop) import only from here.

import { mount } from "svelte";

import type { Backend } from "./commands/backend";
import AppShell from "./shell/AppShell.svelte";

export type * from "./commands/backend";
export type * from "./commands/types";
export { BackendError } from "./commands/backend";
export { MockBackend } from "./commands/mock";

/** Mounts the app into `target` with the given backend. */
export function startApp(target: HTMLElement, backend: Backend) {
  return mount(AppShell, { target, props: { backend } });
}
