// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tabs of a distribution's extensions (`AppExtension.tabs`, since wave 13): the shell provides
// its extensions once, the request editor, the response panel and the collection panel read
// them here. Without a provider (components rendered on their own, tests) there are none.
// Also the diff notes (`AppExtension.diffNotes`) of the diff and review views.

import { getContext, setContext } from "svelte";

import type { Component } from "svelte";

import type {
  AppExtension,
  ExtensionContext,
  ExtensionDiffNotesProps,
  ExtensionTab,
  ExtensionTabPlace,
} from "../extensions";
import { matchesContentType } from "../plugins/permissions";

export interface ExtensionTabRef {
  /** `<extension id>/<tab id>`, unique. */
  key: string;
  extension: string;
  tab: ExtensionTab;
}

export interface ProvidedExtensions {
  readonly extensions: readonly AppExtension[];
  readonly ctx: ExtensionContext;
}

const KEY = Symbol("rt-extensions");

export function provideExtensions(provided: ProvidedExtensions): ProvidedExtensions {
  setContext(KEY, provided);
  return provided;
}

/** The shell's extensions, or `null` outside the shell. */
export function getExtensions(): ProvidedExtensions | null {
  return getContext<ProvidedExtensions | undefined>(KEY) ?? null;
}

/** Tabs of `extensions` for `place`, in extension order; response tabs filtered by the
 *  response's content type (`null`: none known). An extension whose `tabs` throws shows none. */
export function extensionTabsFor(
  provided: ProvidedExtensions | null,
  place: ExtensionTabPlace,
  contentType: string | null = null,
): ExtensionTabRef[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => {
    let tabs: readonly ExtensionTab[];
    try {
      tabs = x.tabs?.(provided.ctx) ?? [];
    } catch (e) {
      console.warn(`RequestTailor: tabs of extension ${x.id} failed:`, e);
      return [];
    }
    return tabs
      .filter((t) => t.place === place && (place !== "response" || matchesContentType([...(t.contentTypes ?? [])], contentType)))
      .map((tab) => ({ key: `${x.id}/${tab.id}`, extension: x.id, tab }));
  });
}

/** The diff-notes components of `provided`, in extension order. */
export function extensionDiffNotes(
  provided: ProvidedExtensions | null,
): { extension: string; component: Component<ExtensionDiffNotesProps> }[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => (x.diffNotes ? [{ extension: x.id, component: x.diffNotes }] : []));
}
