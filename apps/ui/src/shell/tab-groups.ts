// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tab groups, the way Chrome has them: a group has a name (may be empty), one of eight colours
// and a collapsed flag; its tabs sit next to each other in the tab order. Pure functions over
// the tab order (keys, see `keyOf`) and a `GroupState`; the reactive wrapper is
// tab-groups.svelte.ts, the tests are tab-groups.test.ts. Groups are per-device UI state and
// never written into a collection.

import { fitTabs, TAB_MORE } from "./tab-fit";

export const GROUP_COLORS = 8;
/** Names of the eight colours (`--rt-lane-1…8`), in palette order. */
export const COLOR_NAMES = ["Blue", "Green", "Amber", "Purple", "Red", "Teal", "Pink", "Olive"] as const;

export interface TabGroup {
  id: string;
  name: string;
  /** Index into the palette, 0 to 7. */
  color: number;
  collapsed: boolean;
}

export interface GroupState {
  groups: TabGroup[];
  /** Tab key to group id. */
  members: Record<string, string>;
}

export const emptyState = (): GroupState => ({ groups: [], members: {} });

export interface TabKeyed {
  collection: string;
  path: string;
}
export const keyOf = (ref: TabKeyed): string => `${ref.collection}\u0000${ref.path}`;

export const groupOf = (state: GroupState, key: string): TabGroup | undefined => {
  const id = state.members[key];
  return id === undefined ? undefined : state.groups.find((g) => g.id === id);
};

/** The keys of `order` that belong to group `id`, in tab order. */
export const membersOf = (order: readonly string[], state: GroupState, id: string): string[] =>
  order.filter((k) => state.members[k] === id);

/** The order with every group's tabs next to each other: a group sits where its first tab is. */
export function contiguous(order: readonly string[], state: GroupState): string[] {
  const out: string[] = [];
  const done = new Set<string>();
  for (const k of order) {
    const g = groupOf(state, k);
    if (!g) out.push(k);
    else if (!done.has(g.id)) {
      done.add(g.id);
      out.push(...membersOf(order, state, g.id));
    }
  }
  return out;
}

/** Drops groups without any member left. */
function tidy(state: GroupState): GroupState {
  const used = new Set(Object.values(state.members));
  const groups = state.groups.filter((g) => used.has(g.id));
  return groups.length === state.groups.length ? state : { ...state, groups };
}

/** Forgets the membership of tabs that were closed; a group without tabs goes away. */
export function forget(state: GroupState, removed: Iterable<string>): GroupState {
  const members = { ...state.members };
  let any = false;
  for (const k of removed) if (k in members) (delete members[k], (any = true));
  return any ? tidy({ ...state, members }) : state;
}

/** The colour used least so far (the first of equals), so a new group looks new. */
export function nextColor(state: GroupState): number {
  const uses = Array.from({ length: GROUP_COLORS }, () => 0);
  for (const g of state.groups) uses[g.color]++;
  return uses.indexOf(Math.min(...uses));
}

export function newGroupId(state: GroupState): string {
  let n = state.groups.length + 1;
  while (state.groups.some((g) => g.id === `g${n}`)) n++;
  return `g${n}`;
}

/** A new group around one tab, where the tab is. */
export function createGroup(state: GroupState, key: string, id = newGroupId(state), color = nextColor(state)): GroupState {
  const without = forget(state, [key]);
  return { groups: [...without.groups, { id, name: "", color, collapsed: false }], members: { ...without.members, [key]: id } };
}

/** Puts a tab at the end of an existing group (it moves next to the group's tabs). */
export function addToGroup(order: readonly string[], state: GroupState, key: string, id: string): { order: string[]; state: GroupState } {
  if (!state.groups.some((g) => g.id === id)) return { order: [...order], state };
  const members = { ...state.members, [key]: id };
  const next: GroupState = tidy({ ...state, members });
  const rest = order.filter((k) => k !== key);
  const last = rest.reduce((at, k, i) => (members[k] === id ? i : at), -1);
  rest.splice(last < 0 ? rest.length : last + 1, 0, key);
  return { order: contiguous(rest, next), state: next };
}

/** Keys of the tabs outside group `id` (ungrouped or in another group), in tab order. */
export const outsideGroup = (order: readonly string[], state: GroupState, id: string): string[] =>
  order.filter((k) => state.members[k] !== id);

/** Moves every ungrouped tab into group `id`. */
export function addAllUngrouped(order: readonly string[], state: GroupState, id: string): { order: string[]; state: GroupState } {
  let r = { order: [...order], state };
  if (!state.groups.some((g) => g.id === id)) return r;
  for (const k of order) if (!state.members[k]) r = addToGroup(r.order, r.state, k, id);
  return r;
}

/** Takes a tab out of its group; it stays right after the group's remaining tabs. */
export function removeFromGroup(order: readonly string[], state: GroupState, key: string): { order: string[]; state: GroupState } {
  const g = groupOf(state, key);
  if (!g) return { order: [...order], state };
  const next = forget(state, [key]);
  const rest = order.filter((k) => k !== key);
  const last = rest.reduce((at, k, i) => (next.members[k] === g.id ? i : at), -1);
  rest.splice(last < 0 ? order.indexOf(key) : last + 1, 0, key);
  return { order: rest, state: next };
}

export function ungroup(state: GroupState, id: string): GroupState {
  const members = Object.fromEntries(Object.entries(state.members).filter(([, g]) => g !== id));
  return { groups: state.groups.filter((g) => g.id !== id), members };
}

const update = (state: GroupState, id: string, patch: Partial<TabGroup>): GroupState => ({
  ...state,
  groups: state.groups.map((g) => (g.id === id ? { ...g, ...patch } : g)),
});
export const renameGroup = (state: GroupState, id: string, name: string) => update(state, id, { name: name.trim() });
export const recolorGroup = (state: GroupState, id: string, color: number) =>
  update(state, id, { color: Math.min(GROUP_COLORS - 1, Math.max(0, Math.trunc(color))) });
export const setCollapsed = (state: GroupState, id: string, collapsed: boolean) => update(state, id, { collapsed });

/** The first key of the block (a group's tabs, or the tab itself) that holds `key`. */
function blockOf(order: readonly string[], state: GroupState, key: string): string[] {
  const g = groupOf(state, key);
  return g ? membersOf(order, state, g.id) : [key];
}

/**
 * Drag and drop of a tab onto the tab `target` (left or right half): it lands before or after
 * it and joins the target's group, or is ungrouped when the target is.
 */
export function dropTab(order: readonly string[], state: GroupState, key: string, target: string, after: boolean): { order: string[]; state: GroupState } {
  if (key === target) return { order: [...order], state };
  const g = groupOf(state, target);
  let next = forget(state, [key]);
  if (g) next = { ...next, members: { ...next.members, [key]: g.id } };
  const rest = order.filter((k) => k !== key);
  rest.splice(rest.indexOf(target) + (after ? 1 : 0), 0, key);
  return { order: contiguous(rest, next), state: tidy(next) };
}

/** Drops a tab on empty space of the strip: ungrouped, at the end. */
export function dropTabAtEnd(order: readonly string[], state: GroupState, key: string): { order: string[]; state: GroupState } {
  const next = forget(state, [key]);
  return { order: [...order.filter((k) => k !== key), key], state: next };
}

/** Moves a whole group before `beforeKey` (the start of that tab's block), `null` for the end. */
export function moveGroup(order: readonly string[], state: GroupState, id: string, beforeKey: string | null): string[] {
  const block = membersOf(order, state, id);
  if (block.length === 0) return [...order];
  const rest = order.filter((k) => state.members[k] !== id);
  const at = beforeKey === null || block.includes(beforeKey) ? rest.length : rest.indexOf(blockOf(rest, state, beforeKey)[0]);
  rest.splice(at < 0 ? rest.length : at, 0, ...block);
  return rest;
}

/** Drop of a group onto the tab `target`: before its block (left half) or after it. */
export function dropGroup(order: readonly string[], state: GroupState, id: string, target: string, after: boolean): string[] {
  if (state.members[target] === id) return [...order];
  const block = blockOf(order, state, target);
  if (!after) return moveGroup(order, state, id, block[0]);
  const nextKey = order[order.indexOf(block[block.length - 1]) + 1];
  return moveGroup(order, state, id, nextKey ?? null);
}

/** One step of a group to the left (`-1`) or right (`1`): it hops over the neighbouring block. */
export function stepGroup(order: readonly string[], state: GroupState, id: string, dir: -1 | 1): string[] {
  const block = membersOf(order, state, id);
  if (block.length === 0) return [...order];
  const at = dir < 0 ? order.indexOf(block[0]) - 1 : order.indexOf(block[block.length - 1]) + 1;
  const neighbour = order[at];
  if (neighbour === undefined) return [...order];
  return dropGroup(order, state, id, neighbour, dir > 0);
}

/**
 * One step of a tab (Alt+Left / Alt+Right): inside a group it swaps with its neighbour; at the
 * edge of its group it leaves the group; an ungrouped tab hops over a neighbouring group.
 */
export function stepTab(order: readonly string[], state: GroupState, key: string, dir: -1 | 1): { order: string[]; state: GroupState } {
  const i = order.indexOf(key);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= order.length) {
    // The first or last tab of the whole strip: only a grouped tab can still leave.
    return groupOf(state, key) && (j < 0 || j >= order.length) ? { order: [...order], state: forget(state, [key]) } : { order: [...order], state };
  }
  const g = groupOf(state, key);
  const other = order[j];
  if (g) {
    if (state.members[other] !== g.id) return { order: [...order], state: forget(state, [key]) };
    const next = [...order];
    [next[i], next[j]] = [next[j], next[i]];
    return { order: next, state };
  }
  const block = blockOf(order, state, other);
  const rest = order.filter((k) => k !== key);
  const at = dir < 0 ? rest.indexOf(block[0]) : rest.indexOf(block[block.length - 1]) + 1;
  rest.splice(at, 0, key);
  return { order: rest, state };
}

// ---- layout ----

export type StripItem =
  | { kind: "chip"; group: TabGroup; keys: string[] }
  | { kind: "tab"; index: number; key: string; group?: TabGroup };

/** Chips and tabs in strip order; the tabs of collapsed groups are left out. */
export function layout(order: readonly string[], state: GroupState): { items: StripItem[]; tabs: number[] } {
  const items: StripItem[] = [];
  const tabs: number[] = [];
  const seen = new Set<string>();
  order.forEach((key, index) => {
    const group = groupOf(state, key);
    if (group && !seen.has(group.id)) {
      seen.add(group.id);
      items.push({ kind: "chip", group, keys: membersOf(order, state, group.id) });
    }
    if (group?.collapsed) return;
    items.push({ kind: "tab", index, key, group });
    tabs.push(index);
  });
  return { items, tabs };
}

/** What a group is called in menus and lists: its name, or its colour when it has none. */
export const groupLabel = (g: Pick<TabGroup, "name" | "color">): string => g.name || `${COLOR_NAMES[g.color]} group`;

export const CHIP_MIN = 40;
export const CHIP_MAX = 144;
/** Width of a group chip, in px: from the name, plus room for the count when collapsed. */
export function chipWidth(group: Pick<TabGroup, "name" | "collapsed">, count: number): number {
  const label = group.name ? group.name.length * 7 + 30 : 22;
  const counted = group.collapsed ? 30 + String(count).length * 7 : 0;
  return Math.min(CHIP_MAX, Math.max(CHIP_MIN, label + counted));
}

export interface GroupFit {
  items: StripItem[];
  /** Indices of tabs behind the overflow button (collapsed groups do not count). */
  hidden: number[];
  width: number;
}

/**
 * The strip with groups: chips take their own width, the tabs of collapsed groups are gone, and
 * the rest shares what is left as in `fitTabs`. The active tab always stays visible.
 */
export function fitGroups(input: { order: readonly string[]; state: GroupState; active: number; available: number; moreWidth?: number }): GroupFit {
  const { order, state, active, available } = input;
  const lay = layout(order, state);
  const chips = lay.items.filter((i) => i.kind === "chip");
  const reserved = chips.reduce((sum, c) => sum + chipWidth(c.group, c.keys.length), 0);
  const fit = fitTabs({
    count: lay.tabs.length,
    active: lay.tabs.indexOf(active),
    available: available > 0 ? Math.max(1, available - reserved) : 0,
    moreWidth: input.moreWidth ?? TAB_MORE,
  });
  const shown = new Set(fit.visible.map((p) => lay.tabs[p]));
  const items = lay.items.filter((i) => (i.kind === "tab" ? shown.has(i.index) : i.group.collapsed || i.keys.some((k) => shown.has(order.indexOf(k)))));
  return { items, hidden: fit.hidden.map((p) => lay.tabs[p]), width: fit.width };
}

// ---- keyboard navigation (collapsed groups are skipped) ----

/** Indices of the tabs that can be stepped to: those outside collapsed groups. */
export const navigable = (order: readonly string[], state: GroupState): number[] => layout(order, state).tabs;

/** Index of the next (`1`) or previous (`-1`) tab, wrapping, skipping collapsed groups; `-1` if none. */
export function stepTarget(order: readonly string[], state: GroupState, active: number, delta: 1 | -1): number {
  const nav = navigable(order, state);
  if (nav.length === 0) return -1;
  if (active < 0) return nav[0];
  const at = nav.indexOf(active);
  if (at >= 0) return nav[(at + delta + nav.length) % nav.length];
  // The active tab sits in a collapsed group: the nearest visible one in that direction.
  const after = nav.filter((i) => i > active);
  const before = nav.filter((i) => i < active);
  return delta > 0 ? (after[0] ?? nav[0]) : (before.at(-1) ?? nav.at(-1)!);
}

/** The `n`-th (1-based) tab of the strip, or the last one (Mod+9), counting only visible tabs. */
export function numberTarget(order: readonly string[], state: GroupState, n: number, last = false): number {
  const nav = navigable(order, state);
  return last ? (nav.at(-1) ?? -1) : (nav[n - 1] ?? -1);
}

export const isGroup = (v: unknown): v is TabGroup =>
  typeof v === "object" &&
  v !== null &&
  typeof (v as TabGroup).id === "string" &&
  typeof (v as TabGroup).name === "string" &&
  Number.isInteger((v as TabGroup).color) &&
  (v as TabGroup).color >= 0 &&
  (v as TabGroup).color < GROUP_COLORS &&
  typeof (v as TabGroup).collapsed === "boolean";

/** Validates what localStorage held (anything else means "no groups"). */
export function isGroupState(v: unknown): v is GroupState {
  if (typeof v !== "object" || v === null) return false;
  const s = v as GroupState;
  return (
    Array.isArray(s.groups) &&
    s.groups.every(isGroup) &&
    typeof s.members === "object" &&
    s.members !== null &&
    Object.values(s.members).every((g) => typeof g === "string")
  );
}

/** What is stored: at most this many remembered tabs, so a device store cannot grow forever. */
export const MAX_REMEMBERED = 400;
export function trimmed(state: GroupState): GroupState {
  const entries = Object.entries(state.members);
  if (entries.length <= MAX_REMEMBERED) return state;
  return tidy({ ...state, members: Object.fromEntries(entries.slice(-MAX_REMEMBERED)) });
}
