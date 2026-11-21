// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The sidebar shows the tree as a flat list of visible rows (ARIA tree with `aria-level`),
// which keeps focus handling and keyboard navigation simple.

import type { TreeNode } from "../commands/types";

export interface Row {
  /** Unique across folders and requests (a folder and a request may share a path). */
  key: string;
  node: TreeNode;
  /** 1 = child of the collection root. */
  level: number;
  /** Key of the parent row, `null` at the top level. */
  parent: string | null;
  posinset: number;
  setsize: number;
}

export const rowKey = (node: TreeNode) => `${node.kind === "folder" ? "f" : "r"}:${node.path}`;

/** Visible rows of `root`'s children; folders whose path is in `collapsed` hide their children. */
export function visibleRows(root: TreeNode | null, collapsed: ReadonlySet<string>): Row[] {
  const rows: Row[] = [];
  const walk = (children: TreeNode[], level: number, parent: string | null) => {
    children.forEach((node, i) => {
      const key = rowKey(node);
      rows.push({ key, node, level, parent, posinset: i + 1, setsize: children.length });
      if (node.kind === "folder" && !collapsed.has(node.path)) walk(node.children, level + 1, key);
    });
  };
  if (root?.kind === "folder") walk(root.children, 1, null);
  return rows;
}

/** Paths of all requests under `node`. */
export function requestPaths(node: TreeNode | null): string[] {
  if (!node) return [];
  return node.kind === "request" ? [node.path] : node.children.flatMap(requestPaths);
}

/** Type-ahead: the next row after `from` whose name starts with `char`, wrapping around. */
export function typeahead(rows: readonly Row[], from: number, char: string): number {
  const c = char.toLowerCase();
  for (let step = 1; step <= rows.length; step++) {
    const i = (from + step) % rows.length;
    if (rows[i].node.name.toLowerCase().startsWith(c)) return i;
  }
  return from;
}

/** DOM id of the tab at `index` (the panel is labelled by the active one). */
export const tabElementId = (index: number) => `rt-tab-${index}`;
