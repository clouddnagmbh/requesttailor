// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Public entry of the shared UI. Shells (apps/web, apps/desktop) import only from here.

import { mount } from "svelte";

import type { Backend } from "./commands/backend";
import type { StartOptions } from "./extensions";
import AppShell from "./shell/AppShell.svelte";
import "@tailor-kit/ui/styles/tokens.css";

export type * from "./commands/backend";
export type * from "./commands/types";
export { BackendError, MCP_DEMO_URL } from "./commands/backend";
export { LOCAL_CLONE_UNSUPPORTED, looksLocal } from "./commands/clone-source";
export { MockBackend } from "./commands/mock";
// Wave 12 interface: placeholders the shells use until each strand fills in its own file.
export * from "./commands/wave12-stubs";
// Wave 14 interface: the collection set over the single open collection (strand A replaces it).
export * from "./commands/wave14-stubs";
// Wave 17: a link that reached the app from outside waits in the share inbox (`apps/web/src/main.ts`).
export { offerSharedLink } from "./share/inbox.svelte";
export { looksShared } from "./share/detect";
export * from "./commands/git-methods";
export { applyLineSelection, chosenLines, splitLines } from "./commands/line-selection";
export * from "./commands/repositories";
// Wave 14, strand F: the graph's order and pages for shells that build it in TypeScript.
export { dateOrder, graphPage, reachable, sortChanges, sortRefs, type GraphSource, type OrderCommit } from "./git/graph/order";
export * from "./commands/openapi-stub";
/** Wave 14: branch and tag names as `rt_app::git::actions::check_ref_name` checks them (the web Git uses it too). */
export { refNameProblem, suggestRefName } from "./git/wizards/refname";
export type * from "./extensions";
export { AI_PROVIDERS, providerInfo } from "@tailor-kit/ai-ui";
// Wave 16: the diff of a suggested change, for assistants that show their own suggestions.
export { default as SuggestedEditView } from "./suggest/SuggestedEditView.svelte";
export { requestBlocks, scriptBlocks, textBlock, type PreviewBlock } from "./suggest/model";
// Wave 16: a model's Markdown answer rendered as text nodes only (never HTML).
export { Markdown, parseMarkdown, type MarkdownBlock, type MarkdownSpan } from "@tailor-kit/ai-ui";

// Wave 14 design foundation: UI primitives and the semantic icon set (guide: ui/README.md).
export * from "./ui";

/**
 * Mounts the app into `target` with the given backend. A distribution passes its extensions in
 * `options` (wave 9, `./extensions`); the builds of this repository pass none.
 */
export function startApp(target: HTMLElement, backend: Backend, options: StartOptions = {}) {
  return mount(AppShell, { target, props: { backend, extensions: options.extensions ?? [] } });
}

// Wave 17: the production rule (policy flag of a listed environment, else the name) for extensions.
export { isProduction } from "./environments/production";
export { isProductionName } from "./environments/ops";
