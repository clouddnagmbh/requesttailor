// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The reactive side of tab groups: group state over `AppState.openTabs`, remembered per device
// (localStorage, like the other shell conveniences; never written into a collection). The pure
// rules are in tab-groups.ts. `attach()` (called once by the shell, which is always mounted)
// keeps the groups in step with the tabs: closed tabs leave their group, a tab that arrives
// while "New tab in group" is pending joins it, groups stay contiguous, and showing a tab of a
// collapsed group expands it.

import { untrack } from "svelte";

import type { RequestRef } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { load, store } from "./storage";
import {
  addAllUngrouped,
  addToGroup,
  outsideGroup,
  contiguous,
  createGroup,
  dropGroup,
  dropTab,
  dropTabAtEnd,
  emptyState,
  forget,
  groupOf,
  isGroupState,
  keyOf,
  membersOf,
  navigable,
  newGroupId,
  numberTarget,
  recolorGroup,
  removeFromGroup,
  renameGroup,
  setCollapsed,
  stepGroup,
  stepTab,
  stepTarget,
  trimmed,
  ungroup,
  type GroupState,
  type TabGroup,
} from "./tab-groups";

const STORAGE_KEY = "tab-groups";

export type Drag = { kind: "tab"; key: string } | { kind: "group"; id: string };

export class TabGroups {
  state = $state<GroupState>(emptyState());
  /** The group a new tab joins (set by "New tab in group"). */
  pending = $state<string | null>(null);
  /** What is being dragged, if anything. */
  drag = $state<Drag | null>(null);
  /** The group whose rename popover is open. */
  editing = $state<string | null>(null);

  /** The group touched last, so palette commands still find it after it was collapsed. */
  private recent: string | null = null;
  private seen: Set<string> | null = null;
  private pendingTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly app: AppState) {
    this.state = load<GroupState>(STORAGE_KEY, emptyState(), isGroupState);
  }

  // ---- reading ----

  private get order(): string[] {
    return this.app.openTabs.map(keyOf);
  }
  groupOfRef = (ref: RequestRef): TabGroup | undefined => groupOf(this.state, keyOf(ref));
  /** Groups that have an open tab, in tab order. */
  get groups(): TabGroup[] {
    const live = new Set(this.order.map((k) => this.state.members[k]));
    return this.state.groups.filter((g) => live.has(g.id));
  }
  group(id: string): TabGroup | undefined {
    return this.state.groups.find((g) => g.id === id);
  }
  refsOf(id: string): RequestRef[] {
    const keys = new Set(membersOf(this.order, this.state, id));
    return this.app.openTabs.filter((t) => keys.has(keyOf(t)));
  }
  /** Indices of the tabs outside collapsed groups. */
  get navigable(): number[] {
    return navigable(this.order, this.state);
  }

  // ---- changes ----

  private commit(state: GroupState, order?: string[]): void {
    this.state = trimmed(state);
    if (order) this.reorder(order);
    store(STORAGE_KEY, this.state);
  }

  private reorder(order: string[]): void {
    const byKey = new Map(this.app.openTabs.map((t) => [keyOf(t), t]));
    const next = order.flatMap((k) => byKey.get(k) ?? []);
    if (next.length === this.app.openTabs.length && next.some((t, i) => t !== this.app.openTabs[i])) this.app.openTabs = next;
  }

  /** "Add tab to new group": returns the id; the rename popover opens. */
  newGroup(ref: RequestRef, edit = true): string {
    const id = newGroupId(this.state);
    this.commit(createGroup(this.state, keyOf(ref), id));
    this.recent = id;
    if (edit) this.editing = id;
    return id;
  }

  add(ref: RequestRef, id: string): void {
    const r = addToGroup(this.order, this.state, keyOf(ref), id);
    this.recent = id;
    this.commit(r.state, r.order);
  }

  /** Moves every ungrouped tab into the group. */
  addAllUngrouped(id: string): void {
    const r = addAllUngrouped(this.order, this.state, id);
    this.recent = id;
    this.commit(r.state, r.order);
  }

  /** Open tabs outside the group, in tab order. */
  outside(id: string): RequestRef[] {
    const keys = new Set(outsideGroup(this.order, this.state, id));
    return this.app.openTabs.filter((t) => keys.has(keyOf(t)));
  }

  remove(ref: RequestRef): void {
    const r = removeFromGroup(this.order, this.state, keyOf(ref));
    this.commit(r.state, r.order);
  }

  rename(id: string, name: string): void {
    this.commit(renameGroup(this.state, id, name));
  }
  recolor(id: string, color: number): void {
    this.commit(recolorGroup(this.state, id, color));
  }

  ungroup(id: string): void {
    this.commit(ungroup(this.state, id));
  }

  async closeGroup(id: string): Promise<void> {
    const refs = this.refsOf(id);
    this.commit(ungroup(this.state, id));
    await this.app.closeRefs(refs);
  }

  /** Collapses or expands. Collapsing the group of the shown tab shows a tab outside it first. */
  async toggle(id: string, collapsed?: boolean): Promise<void> {
    const g = this.group(id);
    if (!g) return;
    this.recent = id;
    const to = collapsed ?? !g.collapsed;
    if (to) {
      const shown = this.app.activeTabIndex;
      const order = this.order;
      if (shown >= 0 && this.state.members[order[shown]] === id) {
        const members = new Set(membersOf(order, this.state, id));
        const outside = this.navigable.filter((i) => !members.has(order[i]));
        const after = outside.find((i) => i > shown) ?? outside.at(-1);
        if (after !== undefined) await this.app.selectRef(this.app.openTabs[after]);
      }
    }
    this.commit(setCollapsed(this.state, id, to));
  }

  /** Makes sure the group of a tab is open (a tab of a collapsed group was chosen). */
  reveal(ref: RequestRef): void {
    const g = this.groupOfRef(ref);
    if (g?.collapsed) this.commit(setCollapsed(this.state, g.id, false));
  }

  /** "New tab in group": the next tab that opens joins it. */
  openIn(id: string): void {
    this.pending = id;
    this.app.openNew();
  }

  moveTab(ref: RequestRef, dir: -1 | 1): void {
    const r = stepTab(this.order, this.state, keyOf(ref), dir);
    this.commit(r.state, r.order);
  }
  moveGroup(id: string, dir: -1 | 1): void {
    this.commit(this.state, stepGroup(this.order, this.state, id, dir));
  }

  dropTab(key: string, target: string, after: boolean): void {
    const r = dropTab(this.order, this.state, key, target, after);
    this.commit(r.state, r.order);
  }
  dropTabOnGroup(key: string, id: string): void {
    const r = addToGroup(this.order, this.state, key, id);
    this.commit(r.state, r.order);
  }
  dropTabAtEnd(key: string): void {
    const r = dropTabAtEnd(this.order, this.state, key);
    this.commit(r.state, r.order);
  }
  dropGroup(id: string, target: string, after: boolean): void {
    this.commit(this.state, dropGroup(this.order, this.state, id, target, after));
  }
  dropGroupAtEnd(id: string): void {
    const rest = this.order.filter((k) => this.state.members[k] !== id);
    this.commit(this.state, [...rest, ...membersOf(this.order, this.state, id)]);
  }

  // ---- navigation that skips collapsed groups ----

  async step(delta: 1 | -1): Promise<void> {
    const to = stepTarget(this.order, this.state, this.app.activeTabIndex, delta);
    if (to >= 0) await this.app.selectRef(this.app.openTabs[to]);
  }
  async goto(n: number, last = false): Promise<void> {
    const to = numberTarget(this.order, this.state, n, last);
    if (to >= 0) await this.app.selectRef(this.app.openTabs[to]);
  }

  // ---- palette commands ----

  /** The group of the shown tab, else the one touched last (it may be collapsed). */
  get current(): TabGroup | undefined {
    const i = this.app.activeTabIndex;
    const own = i < 0 ? undefined : groupOf(this.state, keyOf(this.app.openTabs[i]));
    if (own) return own;
    const last = this.recent ? this.groups.find((g) => g.id === this.recent) : undefined;
    return last;
  }
  newFromCurrent(): void {
    const i = this.app.activeTabIndex;
    if (i >= 0) this.newGroup(this.app.openTabs[i]);
  }

  // ---- keeping in step with the tabs ----

  /** Called with the current tab keys whenever `openTabs` changes. */
  sync(keys: string[]): void {
    const prev = this.seen;
    this.seen = new Set(keys);
    let state = this.state;
    if (prev) {
      const gone = [...prev].filter((k) => !this.seen!.has(k));
      state = forget(state, gone);
      const added = keys.filter((k) => !prev.has(k));
      if (this.pending && added.length > 0) {
        const id = this.pending;
        this.pending = null;
        if (state.groups.some((g) => g.id === id)) {
          const r = addToGroup(keys, state, added[0], id);
          this.commit(r.state, r.order);
          return;
        }
      }
    }
    const order = contiguous(keys, state);
    this.commit(state, order.some((k, i) => k !== keys[i]) ? order : undefined);
  }

  /** Hooks the groups to the tabs; call once from an always-mounted component. */
  attach(): void {
    $effect(() => {
      const keys = this.app.openTabs.map(keyOf);
      untrack(() => this.sync(keys));
    });
    $effect(() => {
      const i = this.app.activeTabIndex;
      const ref = i >= 0 ? this.app.openTabs[i] : null;
      if (ref) untrack(() => this.reveal(ref));
    });
    // A pending "New tab in group" ends with the dialog if nothing was created.
    $effect(() => {
      if (this.app.newDialog === null && untrack(() => this.pending)) {
        clearTimeout(this.pendingTimer);
        this.pendingTimer = setTimeout(() => (this.pending = null), 600);
      }
    });
  }
}

const instances = new WeakMap<AppState, TabGroups>();
/** The groups of an app (one per `AppState`). */
export function getTabGroups(app: AppState): TabGroups {
  let g = instances.get(app);
  if (!g) instances.set(app, (g = new TabGroups(app)));
  return g;
}
