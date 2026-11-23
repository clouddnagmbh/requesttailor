// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { diffTree, initiallyOpen, leafText, parseJson, treeLines } from "./jsontree";

describe("JSON tree diff", () => {
  const old = { id: 1, owner: { name: "ada", role: "admin" }, tags: ["a"], meta: { v: 1, ok: true }, gone: "x" };
  const now = { id: 2, owner: { name: "ada", role: "admin" }, tags: ["a", "b"], meta: { v: 1, ok: true }, fresh: [1, 2] };
  const tree = diffTree(old, now);

  it("marks each node and counts the changes below", () => {
    expect(tree.kind).toBe("changed");
    const kids = Object.fromEntries(tree.children.map((c) => [c.key, c]));
    expect(kids.id!.kind).toBe("changed");
    expect(kids.owner!.kind).toBe("same");
    expect(kids.tags!.kind).toBe("changed");
    expect(kids.tags!.children.map((c: { kind: string }) => c.kind)).toEqual(["same", "added"]);
    expect(kids.gone!.kind).toBe("removed");
    expect(kids.fresh!.kind).toBe("added");
    expect(kids.fresh!.children).toHaveLength(2);
    // id, tags/1, gone, fresh/0, fresh/1.
    expect(tree.changes).toBe(5);
    // Keys keep the order of the older body, new ones follow.
    expect(tree.children.map((c) => c.key)).toEqual(["id", "owner", "tags", "meta", "gone", "fresh"]);
    expect(kids.tags!.children[1]!.pointer).toBe("/tags/1");
  });

  it("folds unchanged containers at first and unfolds on demand", () => {
    const open = initiallyOpen(tree);
    expect([...open].sort()).toEqual(["", "/tags"]);
    const lines = treeLines(tree, (p) => open.has(p));
    const keys = lines.map((l) => (l.closing ? `${l.key ?? ""}end` : l.key));
    expect(keys).toEqual([null, "id", "owner", "tags", "0", "1", "end", "meta", "gone", "fresh", "end"]);
    const owner = lines.find((l) => l.key === "owner")!;
    expect(owner).toMatchObject({ open: false, old: "{ 2 keys }", new: "{ 2 keys }", kind: "same" });
    const fresh = lines.find((l) => l.key === "fresh")!;
    expect(fresh).toMatchObject({ old: null, new: "[ 2 items ]", kind: "added" });
    const gone = lines.find((l) => l.key === "gone")!;
    expect(gone).toMatchObject({ old: '"x"', new: null, kind: "removed" });
    expect(lines.find((l) => l.key === "id")).toMatchObject({ old: "1", new: "2", depth: 1 });

    open.add("/owner");
    const more = treeLines(tree, (p) => open.has(p));
    expect(more.filter((l) => l.depth === 2 && l.pointer.startsWith("/owner/")).map((l) => l.key)).toEqual(["name", "role"]);
  });

  it("treats a change of type as one changed value", () => {
    const t = diffTree({ a: [1] }, { a: { x: 1 } });
    expect(t.children[0]).toMatchObject({ kind: "changed", container: null, changes: 1 });
    expect(diffTree({ a: 1 }, { a: 1 }).kind).toBe("same");
    expect(diffTree([1, { b: 2 }], [1, { b: 2 }]).changes).toBe(0);
  });

  it("shows a body that exists on one side only", () => {
    const t = diffTree(undefined, { a: 1 }, false, true);
    expect(t.kind).toBe("added");
    const lines = treeLines(t, () => true);
    expect(lines[0]).toMatchObject({ old: null, new: "{" });
    expect(lines[1]).toMatchObject({ key: "a", old: null, new: "1" });
  });

  it("cuts long values and parses previews", () => {
    expect(leafText("x".repeat(500), 20)).toHaveLength(20);
    expect(parseJson('{"a":1}')).toEqual({ value: { a: 1 } });
    expect(parseJson("not json")).toBeUndefined();
    expect(parseJson("")).toBeUndefined();
    expect(parseJson("null")).toEqual({ value: null });
  });

  it("keeps the line count bounded", () => {
    const big = Array.from({ length: 5000 }, (_, i) => i);
    const t = diffTree(big, [...big, 1]);
    expect(treeLines(t, () => true, 1000)).toHaveLength(1000);
  });
});
