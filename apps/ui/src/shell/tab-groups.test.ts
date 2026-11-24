// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import {
  addAllUngrouped,
  addToGroup,
  chipWidth,
  contiguous,
  createGroup,
  dropGroup,
  dropTab,
  dropTabAtEnd,
  emptyState,
  fitGroups,
  forget,
  groupOf,
  isGroupState,
  keyOf,
  layout,
  membersOf,
  moveGroup,
  nextColor,
  outsideGroup,
  numberTarget,
  recolorGroup,
  removeFromGroup,
  renameGroup,
  setCollapsed,
  stepGroup,
  stepTab,
  stepTarget,
  ungroup,
  type GroupState,
} from "./tab-groups";

const order = ["a", "b", "c", "d", "e", "f"];
const grouped = (members: Record<string, string>, collapsed: string[] = []): GroupState => ({
  groups: [...new Set(Object.values(members))].map((id, i) => ({ id, name: id, color: i, collapsed: collapsed.includes(id) })),
  members,
});

describe("grouping", () => {
  it("makes a keyed identity from collection and path", () => {
    expect(keyOf({ collection: "c", path: "x.yml" })).toBe("c\u0000x.yml");
  });

  it("creates a group around one tab, in place, with the least used colour", () => {
    const s = createGroup(emptyState(), "c");
    expect(s.groups).toEqual([{ id: "g1", name: "", color: 0, collapsed: false }]);
    expect(groupOf(s, "c")?.id).toBe("g1");
    expect(nextColor(s)).toBe(1);
    expect(createGroup(s, "d").groups[1].color).toBe(1);
    expect(createGroup(s, "d").groups[1].id).toBe("g2");
  });

  it("adds a tab to a group by moving it next to the group's last tab", () => {
    const s = grouped({ b: "g", c: "g" });
    const r = addToGroup(order, s, "f", "g");
    expect(r.order).toEqual(["a", "b", "c", "f", "d", "e"]);
    expect(r.state.members.f).toBe("g");
  });

  it("moves a tab from one group into another", () => {
    const s = grouped({ a: "x", b: "y", c: "y" });
    const r = addToGroup(order, s, "a", "y");
    expect(r.order.slice(0, 3)).toEqual(["b", "c", "a"]);
    expect(r.state.groups.map((g) => g.id)).toEqual(["y"]);
  });

  it("removes a tab from its group; it stays right after the group", () => {
    const s = grouped({ a: "g", b: "g", c: "g" });
    const r = removeFromGroup(order, s, "a");
    expect(r.order).toEqual(["b", "c", "a", "d", "e", "f"]);
    expect(groupOf(r.state, "a")).toBeUndefined();
  });

  it("removes the group with its last tab", () => {
    const s = createGroup(emptyState(), "a");
    expect(removeFromGroup(order, s, "a").state.groups).toEqual([]);
    expect(forget(s, ["a"]).groups).toEqual([]);
  });

  it("forgets only closed tabs and keeps groups that still have tabs", () => {
    const s = grouped({ a: "g", b: "g" });
    const f = forget(s, ["a", "zzz"]);
    expect(f.members).toEqual({ b: "g" });
    expect(f.groups).toHaveLength(1);
  });

  it("ungroups without touching the order", () => {
    const s = ungroup(grouped({ a: "g", b: "g" }), "g");
    expect(s).toEqual(emptyState());
  });

  it("renames (trimmed), recolours (clamped) and collapses", () => {
    let s = createGroup(emptyState(), "a");
    s = renameGroup(s, "g1", "  Orders ");
    s = recolorGroup(s, "g1", 99);
    s = setCollapsed(s, "g1", true);
    expect(s.groups[0]).toEqual({ id: "g1", name: "Orders", color: 7, collapsed: true });
  });

  it("keeps the tabs of a group together in the order", () => {
    const s = grouped({ b: "g", e: "g" });
    expect(contiguous(order, s)).toEqual(["a", "b", "e", "c", "d", "f"]);
    expect(membersOf(order, s, "g")).toEqual(["b", "e"]);
  });
});

describe("drag and drop", () => {
  const s = grouped({ b: "g", c: "g" });

  it("drops a tab on a grouped tab: it joins, before or after", () => {
    const left = dropTab(order, s, "f", "c", false);
    expect(left.order).toEqual(["a", "b", "f", "c", "d", "e"]);
    expect(left.state.members.f).toBe("g");
    expect(dropTab(order, s, "f", "c", true).order).toEqual(["a", "b", "c", "f", "d", "e"]);
  });

  it("drops a grouped tab on an ungrouped one: it leaves", () => {
    const r = dropTab(order, s, "b", "e", true);
    expect(r.order).toEqual(["a", "c", "d", "e", "b", "f"]);
    expect(groupOf(r.state, "b")).toBeUndefined();
    expect(groupOf(r.state, "c")?.id).toBe("g");
  });

  it("reorders plain tabs", () => {
    expect(dropTab(order, emptyState(), "a", "c", true).order).toEqual(["b", "c", "a", "d", "e", "f"]);
  });

  it("drops on empty space: ungrouped, at the end", () => {
    const r = dropTabAtEnd(order, s, "b");
    expect(r.order.at(-1)).toBe("b");
    expect(groupOf(r.state, "b")).toBeUndefined();
  });

  it("moves a whole group before or after another tab's block", () => {
    expect(moveGroup(order, s, "g", "e")).toEqual(["a", "d", "b", "c", "e", "f"]);
    expect(moveGroup(order, s, "g", null)).toEqual(["a", "d", "e", "f", "b", "c"]);
    expect(dropGroup(order, s, "g", "e", true)).toEqual(["a", "d", "e", "b", "c", "f"]);
    expect(dropGroup(order, s, "g", "a", false)).toEqual(["b", "c", "a", "d", "e", "f"]);
  });

  it("drops a group onto another group's block as a whole", () => {
    const two = grouped({ b: "g", c: "g", e: "h", f: "h" });
    expect(dropGroup(order, two, "g", "f", false)).toEqual(["a", "d", "b", "c", "e", "f"]);
    expect(dropGroup(order, two, "h", "b", false)).toEqual(["a", "e", "f", "b", "c", "d"]);
  });
});

describe("keyboard moves", () => {
  it("swaps plain tabs and stops at the ends", () => {
    expect(stepTab(order, emptyState(), "b", 1).order).toEqual(["a", "c", "b", "d", "e", "f"]);
    expect(stepTab(order, emptyState(), "a", -1).order).toEqual(order);
  });

  it("hops an ungrouped tab over a whole group", () => {
    const s = grouped({ c: "g", d: "g" });
    expect(stepTab(order, s, "b", 1).order).toEqual(["a", "c", "d", "b", "e", "f"]);
    expect(stepTab(order, s, "e", -1).order).toEqual(["a", "b", "e", "c", "d", "f"]);
  });

  it("swaps inside a group and leaves it at the edge", () => {
    const s = grouped({ c: "g", d: "g" });
    expect(stepTab(order, s, "c", 1).order).toEqual(["a", "b", "d", "c", "e", "f"]);
    const out = stepTab(order, s, "d", 1);
    expect(out.order).toEqual(order);
    expect(groupOf(out.state, "d")).toBeUndefined();
    expect(groupOf(out.state, "c")?.id).toBe("g");
  });

  it("moves a group by one block", () => {
    const s = grouped({ c: "g", d: "g" });
    expect(stepGroup(order, s, "g", 1)).toEqual(["a", "b", "e", "c", "d", "f"]);
    expect(stepGroup(order, s, "g", -1)).toEqual(["a", "c", "d", "b", "e", "f"]);
    expect(stepGroup(["c", "d", "a"], s, "g", -1)).toEqual(["c", "d", "a"]);
  });

  it("skips collapsed groups when stepping and counting", () => {
    const s = grouped({ b: "g", c: "g" }, ["g"]);
    // visible: a(0) d(3) e(4) f(5)
    expect(stepTarget(order, s, 0, 1)).toBe(3);
    expect(stepTarget(order, s, 3, -1)).toBe(0);
    expect(stepTarget(order, s, 5, 1)).toBe(0);
    expect(stepTarget(order, s, 0, -1)).toBe(5);
    expect(stepTarget(order, s, 1, 1)).toBe(3);
    expect(stepTarget(order, s, 2, -1)).toBe(0);
    expect(numberTarget(order, s, 2)).toBe(3);
    expect(numberTarget(order, s, 9)).toBe(-1);
    expect(numberTarget(order, s, 9, true)).toBe(5);
  });
});

describe("layout and fit", () => {
  it("puts a chip before the tabs of its group and drops collapsed tabs", () => {
    const s = grouped({ b: "g", c: "g" });
    const l = layout(order, s);
    expect(l.items.map((i) => (i.kind === "chip" ? `chip:${i.group.id}` : i.key))).toEqual(["a", "chip:g", "b", "c", "d", "e", "f"]);
    const c = layout(order, setCollapsed(s, "g", true));
    expect(c.items.map((i) => (i.kind === "chip" ? "chip" : i.key))).toEqual(["a", "chip", "d", "e", "f"]);
    expect(c.tabs).toEqual([0, 3, 4, 5]);
  });

  it("sizes chips from the name, wider with the count when collapsed", () => {
    const open = chipWidth({ name: "", collapsed: false }, 3);
    expect(open).toBeLessThan(chipWidth({ name: "Orders", collapsed: false }, 3));
    expect(chipWidth({ name: "Orders", collapsed: true }, 3)).toBeGreaterThan(chipWidth({ name: "Orders", collapsed: false }, 3));
    expect(chipWidth({ name: "x".repeat(100), collapsed: false }, 3)).toBeLessThanOrEqual(144);
  });

  it("shares only the width the chips leave, and never loses the active tab", () => {
    const many = Array.from({ length: 12 }, (_, i) => `t${i}`);
    const s = grouped({ t2: "g", t3: "g" });
    const w = 800;
    const fit = fitGroups({ order: many, state: s, active: 11, available: w });
    const tabs = fit.items.filter((i) => i.kind === "tab");
    expect(tabs.some((t) => t.kind === "tab" && t.index === 11)).toBe(true);
    const shownTabs = tabs.length;
    const noGroups = fitGroups({ order: many, state: emptyState(), active: 11, available: w });
    expect(shownTabs).toBeLessThanOrEqual(noGroups.items.length);
    expect(shownTabs + fit.hidden.length).toBe(12);
  });

  it("collapsed groups free the space and are not counted as hidden", () => {
    const many = Array.from({ length: 10 }, (_, i) => `t${i}`);
    const open = grouped({ t0: "g", t1: "g", t2: "g", t3: "g" });
    const closed = setCollapsed(open, "g", true);
    const a = fitGroups({ order: many, state: open, active: 9, available: 600 });
    const b = fitGroups({ order: many, state: closed, active: 9, available: 600 });
    expect(b.hidden.length).toBeLessThan(a.hidden.length);
    const all = [...b.hidden, ...b.items.flatMap((i) => (i.kind === "tab" ? [i.index] : []))].sort((x, y) => x - y);
    expect(all).toEqual([4, 5, 6, 7, 8, 9]);
  });

  it("drops the chip of a group whose tabs are all behind the overflow button", () => {
    const many = Array.from({ length: 12 }, (_, i) => `t${i}`);
    const s = grouped({ t10: "g", t11: "g" });
    const fit = fitGroups({ order: many, state: s, active: 0, available: 500 });
    expect(fit.items.some((i) => i.kind === "chip")).toBe(false);
    const collapsed = fitGroups({ order: many, state: setCollapsed(s, "g", true), active: 0, available: 500 });
    expect(collapsed.items.some((i) => i.kind === "chip")).toBe(true);
  });

  it("shows everything while the strip is not measured", () => {
    expect(fitGroups({ order, state: emptyState(), active: 0, available: 0 }).hidden).toEqual([]);
  });
});

describe("storage", () => {
  it("accepts what it wrote and rejects the rest", () => {
    expect(isGroupState(createGroup(emptyState(), "a"))).toBe(true);
    expect(isGroupState({ groups: [{ id: "g", name: "", color: 9, collapsed: false }], members: {} })).toBe(false);
    expect(isGroupState({ groups: [], members: { a: 1 } })).toBe(false);
    expect(isGroupState(null)).toBe(false);
    expect(isGroupState("x")).toBe(false);
  });
});

describe("chip menu: add tabs", () => {
  it("lists the tabs outside the group, grouped ones included", () => {
    const s = grouped({ b: "g", c: "h" });
    expect(outsideGroup(order, s, "g")).toEqual(["a", "c", "d", "e", "f"]);
  });
  it("adds one tab next to the group's tabs", () => {
    const s = grouped({ b: "g" });
    const r = addToGroup(order, s, "e", "g");
    expect(r.order).toEqual(["a", "b", "e", "c", "d", "f"]);
    expect(groupOf(r.state, "e")?.id).toBe("g");
  });
  it("adds all ungrouped tabs and leaves other groups alone", () => {
    const s = grouped({ b: "g", d: "h" });
    const r = addAllUngrouped(order, s, "g");
    expect(membersOf(r.order, r.state, "g")).toEqual(["b", "a", "c", "e", "f"].sort((x, y) => r.order.indexOf(x) - r.order.indexOf(y)));
    expect(groupOf(r.state, "d")?.id).toBe("h");
    expect(outsideGroup(r.order, r.state, "g")).toEqual(["d"]);
  });
  it("ignores an unknown group", () => {
    expect(addAllUngrouped(order, emptyState(), "x").order).toEqual(order);
  });
});
