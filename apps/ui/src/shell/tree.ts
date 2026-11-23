// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The sidebar shows the tree as a flat list of visible rows (ARIA tree with `aria-level`),
// which keeps focus handling and keyboard navigation simple.

import type { CollectionEntry, TreeNode } from "../commands/types";

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

/** A collection node of the sidebar (wave 14): the top level of the tree. */
export interface CollectionNode {
  kind: "collection";
  path: "";
  name: string;
  entry: CollectionEntry;
}

/** A row of the sidebar's tree over several collections (wave 14). */
export interface SideRow {
  /** Unique across collections: `c:<id>` for a collection, `<id>\u001ff:<path>` and
   *  `<id>\u001fr:<path>` for its folders and requests. */
  key: string;
  node: CollectionNode | TreeNode;
  collection: string;
  /** 1 = a collection, 2 = a child of a collection's root. */
  level: number;
  parent: string | null;
  posinset: number;
  setsize: number;
  /** Collection rows: expanded, but the tree is not read yet. */
  loading?: boolean;
}

export const collectionKey = (id: string) => `c:${id}`;
export const sideKey = (collection: string, node: TreeNode) => `${collection}\u001f${rowKey(node)}`;
/** What `collapsed` holds for a folder of a collection. */
export const folderKey = (collection: string, path: string) => `${collection}\u001f${path}`;

/**
 * The visible rows of every collection: each collection, and below an expanded one
 * (`expanded` holds collection ids) its folders and requests; folders whose `folderKey` is in
 * `collapsed` hide their children. A collection whose tree is not in `trees` yet is
 * `loading`.
 */
export function sideRows(
  collections: readonly CollectionEntry[],
  trees: Readonly<Record<string, TreeNode>>,
  expanded: ReadonlySet<string>,
  collapsed: ReadonlySet<string>,
): SideRow[] {
  const rows: SideRow[] = [];
  collections.forEach((entry, ci) => {
    const id = entry.id;
    const key = collectionKey(id);
    const open = expanded.has(id);
    const tree = trees[id];
    rows.push({
      key,
      node: { kind: "collection", path: "", name: entry.name, entry },
      collection: id,
      level: 1,
      parent: null,
      posinset: ci + 1,
      setsize: collections.length,
      loading: open && !tree,
    });
    if (!open || tree?.kind !== "folder") return;
    const walk = (children: TreeNode[], level: number, parent: string) => {
      children.forEach((node, i) => {
        const k = sideKey(id, node);
        rows.push({ key: k, node, collection: id, level, parent, posinset: i + 1, setsize: children.length });
        if (node.kind === "folder" && !collapsed.has(folderKey(id, node.path))) walk(node.children, level + 1, k);
      });
    };
    walk(tree.children, 2, key);
  });
  return rows;
}

/** Paths of all requests under `node`. */
export function requestPaths(node: TreeNode | null): string[] {
  if (!node) return [];
  return node.kind === "request" ? [node.path] : node.children.flatMap(requestPaths);
}

/** Type-ahead: the next row after `from` whose name starts with `char`, wrapping around. */
export function typeahead(rows: readonly { node: { name: string } }[], from: number, char: string): number {
  const c = char.toLowerCase();
  for (let step = 1; step <= rows.length; step++) {
    const i = (from + step) % rows.length;
    if (rows[i].node.name.toLowerCase().startsWith(c)) return i;
  }
  return from;
}

/** DOM id of the tab at `index` (the panel is labelled by the active one). */
export const tabElementId = (index: number) => `rt-tab-${index}`;
