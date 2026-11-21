// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shared app state (Svelte 5 runes). Every strand reads it; changes go through these methods
// so behaviour stays the same in every component. Components get it with `getApp()`.
// Adding fields or methods is an interface change (working rule 1).

import { getContext, setContext } from "svelte";

import type { Backend, CollectionInfo } from "../commands/backend";
import type { Environment, RequestSpec, StoredResponse, TreeNode } from "../commands/types";

const KEY = Symbol("requesttailor-app");

export class AppState {
  readonly backend: Backend;

  collection = $state<CollectionInfo | null>(null);
  tree = $state<TreeNode | null>(null);
  /** Open tabs (request paths), in tab order. */
  tabs = $state<string[]>([]);
  /** The request shown in the editor. */
  active = $state<string | null>(null);
  /** Current editor state per open request; sent and saved from here. */
  drafts = $state<Record<string, RequestSpec>>({});
  /** Paths with unsaved changes (backend's view after the last edit). */
  dirty = $state<string[]>([]);
  environments = $state<Environment[]>([]);
  environment = $state<string | null>(null);
  /** Last response per request path. */
  results = $state<Record<string, StoredResponse>>({});
  sending = $state<Record<string, boolean>>({});
  /** Last error to show in the status area, or `null`. */
  error = $state<string | null>(null);

  constructor(backend: Backend) {
    this.backend = backend;
  }

  private async guard<T>(f: () => Promise<T>): Promise<T | undefined> {
    try {
      this.error = null;
      return await f();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
      return undefined;
    }
  }

  async open(): Promise<void> {
    await this.guard(async () => {
      const info = await this.backend.openCollection();
      if (!info) return;
      this.collection = info;
      this.tabs = [];
      this.active = null;
      this.drafts = {};
      this.results = {};
      this.tree = await this.backend.tree();
      this.environments = await this.backend.environments();
      this.environment = this.environments[0]?.name ?? null;
      this.dirty = await this.backend.dirty();
    });
  }

  /** Opens a request in a tab (or focuses its tab). */
  async select(path: string): Promise<void> {
    await this.guard(async () => {
      if (!this.drafts[path]) this.drafts[path] = await this.backend.getRequest(path);
      if (!this.tabs.includes(path)) this.tabs = [...this.tabs, path];
      this.active = path;
    });
  }

  close(path: string): void {
    const i = this.tabs.indexOf(path);
    if (i < 0) return;
    this.tabs = this.tabs.filter((p) => p !== path);
    if (this.active === path) this.active = this.tabs[Math.min(i, this.tabs.length - 1)] ?? null;
  }

  /** Records an edit from the editor and tells the backend (in memory, unsaved). */
  async edit(path: string, spec: RequestSpec): Promise<void> {
    this.drafts[path] = spec;
    await this.guard(async () => {
      await this.backend.updateRequest(path, $state.snapshot(spec) as RequestSpec);
      this.dirty = await this.backend.dirty();
      if (this.tree) this.tree = await this.backend.tree();
    });
  }

  async create(folder: string, name: string): Promise<void> {
    await this.guard(async () => {
      const path = await this.backend.createRequest(folder, name);
      this.tree = await this.backend.tree();
      this.dirty = await this.backend.dirty();
      await this.select(path);
    });
  }

  async save(): Promise<void> {
    await this.guard(async () => {
      await this.backend.save();
      this.dirty = await this.backend.dirty();
    });
  }

  async send(path: string | null = this.active): Promise<void> {
    if (!path) return;
    this.sending[path] = true;
    await this.guard(async () => {
      const previous = this.results[path];
      const result = await this.backend.send(path, { environment: this.environment, vars: [] });
      this.results[path] = result;
      if (previous?.response_id != null) await this.backend.releaseResponse(previous.response_id);
    });
    this.sending[path] = false;
  }

  setEnvironment(name: string | null): void {
    this.environment = name;
  }

  /** Re-reads environments after edits; keeps the active one if it still exists. */
  async reloadEnvironments(): Promise<void> {
    await this.guard(async () => {
      this.environments = await this.backend.environments();
      if (!this.environments.some((e) => e.name === this.environment)) {
        this.environment = this.environments[0]?.name ?? null;
      }
      this.dirty = await this.backend.dirty();
    });
  }
}

export function provideApp(state: AppState): AppState {
  return setContext(KEY, state);
}

export function getApp(): AppState {
  const state = getContext<AppState | undefined>(KEY);
  if (!state) throw new Error("getApp() called outside <AppShell>");
  return state;
}
