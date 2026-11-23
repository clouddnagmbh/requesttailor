<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar "Collections" (wave 14, strand A): every collection of the set as a node of one ARIA
     tree (roving focus, type-ahead), its folders and requests below when expanded (other
     collections' trees are read on first expand). Header: "New…" and "Add collection…".
     Collection and folder rows have a context menu (right click, Shift+F10, the context-menu
     key, or the row's "More" button) with the shell's actions and the extensions' node actions;
     the first `inline` node action is also a chip on the row. An expanded collection shows the
     extensions' hint for place "collection" at its top.
     The inline "new request" prompt (`newIn`, a folder of the active collection, `""` = root)
     and its kind (wave 8) stay as they were; `onrun` (wave 7) opens the runner. -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import type { CollectionEntry } from "../commands/types";
  import type { NodeTarget } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import { asKind, NEW_REQUEST_KINDS, type NewRequestKind } from "../stream/kind";
  import { Badge, Button, Chip, EmptyState, Icon, IconButton, Menu, Skeleton, type MenuEntry } from "../ui";
  import ExtensionHints from "./ExtensionHints.svelte";
  import { extensionNodeActions } from "./extension-hints";
  import { getExtensions } from "./extension-tabs";
  import { collectionKey, folderKey, sideKey, sideRows, typeahead, type SideRow } from "./tree";

  let {
    newIn = $bindable(null),
    hint,
    onrun,
    panelTabs = [],
    onpanel,
    onenvironments,
  }: {
    newIn?: string | null;
    hint: string;
    /** Opens the runner for `folder` (`null`: the whole collection) of `collection`. */
    onrun?: (collection: string, folder: string | null, name: string) => void;
    /** The collection tabs of plugins and extensions (the same for every collection). */
    panelTabs?: readonly { key: string; title: string }[];
    /** Opens collection tab `key` of `collection`. */
    onpanel?: (collection: string, key: string) => void;
    /** Opens the environments of `collection`. */
    onenvironments?: (collection: string) => void;
  } = $props();

  const app = getApp();
  const provided = getExtensions();
  /** Desktop only: an optional method of the desktop shell's collection set (not yet part of
   *  `CollectionSetApi`; reported as an interface need). */
  const collectionsApi = app.backend.collections as { reveal?: (id: string) => Promise<void> };
  const reveal = typeof collectionsApi.reveal === "function" ? (id: string) => collectionsApi.reveal!(id) : null;

  /** Expanded collections (ids); the active one opens by itself. */
  const expanded = new SvelteSet<string>();
  /** Collapsed folders (`folderKey`). */
  const collapsed = new SvelteSet<string>();
  const rows = $derived(sideRows(app.collections, app.trees, expanded, collapsed));
  let focusKey = $state<string | null>(null);
  let newName = $state("");
  let newKind = $state<NewRequestKind>("http");
  let treeEl = $state<HTMLElement>();
  const focusIndex = $derived(Math.max(0, rows.findIndex((r) => r.key === focusKey)));

  $effect(() => {
    const id = app.activeCollection;
    if (id) untrack(() => expanded.add(id));
  });

  const isActiveRequest = (row: SideRow) =>
    row.node.kind === "request" && row.collection === app.activeCollection && row.node.path === app.active;
  const rootOf = (entry: CollectionEntry) => app.roots.find((r) => r.id === entry.root);
  const requestKey = (id: string, path: string) => sideKey(id, { kind: "request", path, name: "", method: "" });

  /** Moves keyboard focus into the tree: the active request's row, else the active
   *  collection's, else the first row. */
  export async function focus(): Promise<void> {
    const id = app.activeCollection;
    if (id) {
      const active = app.active ? requestKey(id, app.active) : null;
      const want = active && rows.some((r) => r.key === active) ? active : collectionKey(id);
      if (focusKey !== want && !(active === null && rows.some((r) => r.key === focusKey))) focusKey = want;
    }
    await focusRow(rows[focusIndex]);
  }

  async function focusRow(row: SideRow | undefined) {
    if (!row) return;
    focusKey = row.key;
    await tick();
    treeEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(row.key)}"]`)?.focus();
  }

  const isOpen = (row: SideRow) =>
    row.node.kind === "collection"
      ? expanded.has(row.collection)
      : row.node.kind === "folder" && !collapsed.has(folderKey(row.collection, row.node.path));

  function toggle(row: SideRow, open = !isOpen(row)) {
    if (row.node.kind === "collection") {
      if (open) {
        expanded.add(row.collection);
        void app.ensureTree(row.collection);
      } else expanded.delete(row.collection);
    } else if (row.node.kind === "folder") {
      const k = folderKey(row.collection, row.node.path);
      if (open) collapsed.delete(k);
      else collapsed.add(k);
    }
  }

  function activate(row: SideRow) {
    focusKey = row.key;
    if (row.node.kind === "request") void app.selectRef({ collection: row.collection, path: row.node.path });
    else toggle(row);
  }

  function onkeydown(e: KeyboardEvent, row: SideRow, i: number) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      openMenu(row, e.currentTarget as HTMLElement);
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const branch = row.node.kind !== "request";
    const open = branch && isOpen(row);
    let handled = true;
    switch (e.key) {
      case "ArrowDown":
        focusRow(rows[Math.min(i + 1, rows.length - 1)]);
        break;
      case "ArrowUp":
        focusRow(rows[Math.max(i - 1, 0)]);
        break;
      case "Home":
        focusRow(rows[0]);
        break;
      case "End":
        focusRow(rows[rows.length - 1]);
        break;
      case "ArrowRight":
        if (branch && !open) toggle(row, true);
        else if (open) focusRow(rows[i + 1]?.parent === row.key ? rows[i + 1] : undefined);
        break;
      case "ArrowLeft":
        if (open) toggle(row, false);
        else focusRow(rows.find((r) => r.key === row.parent));
        break;
      case "Enter":
      case " ":
        activate(row);
        break;
      default:
        if (e.key.length === 1 && /\S/.test(e.key)) focusRow(rows[typeahead(rows, i, e.key)]);
        else handled = false;
    }
    if (handled) e.preventDefault();
  }

  /** Opens the inline prompt for `folder` of `collection` (made active first). */
  async function startNew(collection: string, folder: string) {
    if (app.activeCollection !== collection) await app.activate(collection);
    expanded.add(collection);
    newIn = folder;
  }

  // The prompt may also be opened from outside (Mod+N, palette): show it, start empty.
  $effect.pre(() => {
    if (newIn === null) return;
    const folder = newIn;
    untrack(() => {
      const id = app.activeCollection;
      if (id) {
        expanded.add(id);
        collapsed.delete(folderKey(id, folder));
      }
    });
    newName = "";
    newKind = "http";
  });

  async function commitNew() {
    const folder = newIn;
    const name = newName.trim();
    if (folder === null || !name) return;
    newIn = null;
    const kind = newKind;
    await app.create(folder, name);
    // The new request is the active one; give it its kind (unsaved, like the request itself).
    const path = app.active;
    const draft = path ? app.drafts[path] : undefined;
    if (kind !== "http" && path && draft) await app.edit(path, asKind($state.snapshot(draft), kind));
    await focus();
  }

  async function cancelNew() {
    if (newIn === null) return;
    newIn = null;
    await focus();
  }

  function onNewKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitNew();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancelNew();
    }
  }

  // Leaving the prompt without Enter cancels, unless the prompt is going away anyway (moving
  // between the name and the kind stays).
  function onNewBlur(e: FocusEvent) {
    const row = (e.currentTarget as HTMLElement).closest(".new");
    if (row && e.relatedTarget instanceof Node && row.contains(e.relatedTarget)) return;
    if (newIn !== null) newIn = null;
  }

  const autofocus = (el: HTMLInputElement) => {
    el.focus();
  };

  // ---- node actions and context menus ----

  const targetOf = (row: SideRow): NodeTarget | null =>
    row.node.kind === "collection"
      ? { kind: "collection", collection: row.collection }
      : row.node.kind === "folder"
        ? { kind: "folder", collection: row.collection, path: row.node.path }
        : null;

  /** Node actions of the extensions for a row; computed for visible rows only. */
  function nodeActions(row: SideRow) {
    const target = targetOf(row);
    return target ? extensionNodeActions(provided, target) : [];
  }

  function runAction(run: () => unknown) {
    try {
      void Promise.resolve(run()).catch((e) => console.warn("RequestTailor: node action failed:", e));
    } catch (e) {
      console.warn("RequestTailor: node action failed:", e);
    }
  }

  const revealLabel = (() => {
    const p = typeof navigator === "undefined" ? "" : navigator.platform;
    return /Mac/i.test(p) ? "Show in Finder" : /Win/i.test(p) ? "Show in Explorer" : "Show in file manager";
  })();

  function menuFor(row: SideRow): MenuEntry[] {
    const id = row.collection;
    const extra: MenuEntry[] = nodeActions(row).map((ref) => ({
      id: ref.key,
      label: ref.action.label,
      icon: ref.action.icon,
      run: () => runAction(ref.action.run),
    }));
    const tail = (entries: MenuEntry[]): MenuEntry[] => (entries.length ? [{ separator: true }, ...entries] : []);
    if (row.node.kind === "folder") {
      const folder = row.node;
      return [
        { id: "new", label: "New…", icon: "add", run: () => app.openNew({ collection: id, folder: folder.path }) },
        { id: "new-request", label: "New request here", icon: "http", run: () => startNew(id, folder.path) },
        ...(onrun ? [{ id: "run", label: "Run folder", icon: "play", run: () => onrun(id, folder.path, folder.name) }] : []),
        ...tail(extra),
      ];
    }
    if (row.node.kind !== "collection") return [];
    const entry = row.node.entry;
    const root = rootOf(entry);
    const siblings = root ? root.collections.length : 1;
    const views: MenuEntry[] = [];
    if (onrun) views.push({ id: "run", label: "Run collection", icon: "play", run: () => onrun(id, null, entry.name) });
    if (panelTabs.length && onpanel) {
      views.push({
        id: "views",
        label: "Collection views",
        icon: "diagram",
        items: panelTabs.map((t) => ({ id: `view:${t.key}`, label: t.title, run: () => onpanel(id, t.key) })),
      });
    }
    if (onenvironments) views.push({ id: "environments", label: "Environments…", icon: "environment", run: () => onenvironments(id) });
    if (reveal) views.push({ id: "reveal", label: revealLabel, icon: "folder-open", run: () => reveal(id) });
    return [
      { id: "new", label: "New…", icon: "add", run: () => app.openNew({ collection: id }) },
      { id: "new-request", label: "New request", icon: "http", shortcut: "Mod+N", run: () => startNew(id, "") },
      ...tail(views),
      ...tail(extra),
      { separator: true },
      {
        id: "remove",
        label: siblings > 1 ? `Remove “${root?.name ?? entry.name}” from sidebar` : "Remove from sidebar",
        hint: siblings > 1 ? `${siblings} collections` : undefined,
        icon: "close",
        run: () => app.removeRoot(entry.root),
      },
    ];
  }

  let menu = $state<{ items: MenuEntry[]; anchor: HTMLElement | { x: number; y: number }; label: string } | null>(null);
  let menuOpen = $state(false);

  function openMenu(row: SideRow, anchor: HTMLElement | { x: number; y: number }) {
    const items = menuFor(row);
    if (items.length === 0) return;
    focusKey = row.key;
    menu = { items, anchor, label: `${row.node.name} actions` };
    menuOpen = true;
  }

  function oncontextmenu(e: MouseEvent, row: SideRow) {
    if (row.node.kind === "request") return;
    e.preventDefault();
    openMenu(row, { x: e.clientX, y: e.clientY });
  }

  const stop = (e: Event) => e.stopPropagation();
  const isEmpty = (id: string) => {
    const t = app.trees[id];
    return t?.kind === "folder" && t.children.length === 0;
  };
</script>

{#snippet prompt(level: number)}
  <div class="row new" role="none" style:--level={level}>
    <input
      data-testid="new-request-name"
      aria-label="New request name"
      placeholder="Request name, Enter to create"
      bind:value={newName}
      onkeydown={onNewKeydown}
      onblur={onNewBlur}
      {@attach autofocus}
    />
    <select
      data-testid="new-request-kind"
      aria-label="New request kind"
      bind:value={newKind}
      onkeydown={onNewKeydown}
      onblur={onNewBlur}
    >
      {#each NEW_REQUEST_KINDS as [k, label] (k)}<option value={k}>{label}</option>{/each}
    </select>
  </div>
{/snippet}

{#snippet chip(row: SideRow)}
  {@const inline = nodeActions(row).find((a) => a.action.inline)}
  {#if inline}
    <span class="chip" role="none" onclick={stop} onkeydown={stop}>
      <Chip
        size="sm"
        tone="accent"
        label={inline.action.label}
        icon={inline.action.icon}
        tabindex={-1}
        data-testid="node-chip"
        onclick={() => runAction(inline.action.run)}
      />
    </span>
  {/if}
{/snippet}

<nav data-testid="sidebar" aria-label="Collections" class="sidebar">
  <div class="head">
    <span class="title">Collections</span>
    <span class="tools">
      <IconButton
        icon="add"
        size="sm"
        label="New…"
        data-testid="sidebar-new"
        tooltipPlacement="bottom"
        onclick={() => app.openNew({ collection: app.activeCollection })}
      />
      <IconButton
        icon="folder-open"
        size="sm"
        label="Add collection…"
        data-testid="add-collection"
        tooltipPlacement="bottom"
        onclick={() => app.addCollection()}
      />
    </span>
  </div>
  {#if app.collections.length > 0}
    <div class="tree" role="tree" aria-label="Collections" bind:this={treeEl}>
      {#each rows as row, i (row.key)}
        {@const node = row.node}
        {@const open = isOpen(row)}
        <div
          class="row {node.kind}"
          class:active={isActiveRequest(row)}
          class:current={node.kind === "collection" && row.collection === app.activeCollection}
          role="treeitem"
          tabindex={i === focusIndex ? 0 : -1}
          data-key={row.key}
          data-path={node.path}
          data-collection={row.collection}
          data-testid={node.kind === "collection" ? "tree-collection" : node.kind === "folder" ? "tree-folder" : "tree-request"}
          aria-level={row.level}
          aria-posinset={row.posinset}
          aria-setsize={row.setsize}
          aria-expanded={node.kind !== "request" ? open : undefined}
          aria-selected={node.kind === "request" ? isActiveRequest(row) : undefined}
          aria-busy={row.loading ? true : undefined}
          title={node.kind === "collection" ? node.entry.location : undefined}
          style:--level={row.level}
          onclick={() => activate(row)}
          onkeydown={(e) => onkeydown(e, row, i)}
          oncontextmenu={(e) => oncontextmenu(e, row)}
          onfocus={() => (focusKey = row.key)}
        >
          {#if node.kind === "request"}
            <span class="method"><Badge kind="method" value={node.method} /></span>
            <span class="name">{node.name}</span>
          {:else}
            <span class="twisty" class:open aria-hidden="true"><Icon name="chevron-right" size="sm" /></span>
            <span class="glyph" aria-hidden="true">
              <Icon name={node.kind === "collection" ? "collection" : open ? "folder-open" : "folder"} size="sm" />
            </span>
            <span class="name">{node.name}</span>
            {@render chip(row)}
            {#if node.kind === "collection" && node.entry.dirty > 0}
              <span
                class="dirty"
                data-testid="collection-dirty"
                title="{node.entry.dirty} unsaved {node.entry.dirty === 1 ? 'change' : 'changes'}"
              >
                <Badge kind="count" tone="accent" value={node.entry.dirty} />
              </span>
            {/if}
            <span class="actions" role="none" onclick={stop} onkeydown={stop}>
              {#if node.kind === "folder" && onrun}
                <IconButton
                  icon="play"
                  size="sm"
                  tabindex={-1}
                  data-testid="run-folder"
                  label="Run {node.name}"
                  onclick={() => onrun(row.collection, node.path, node.name)}
                />
              {/if}
              <IconButton
                icon="add"
                size="sm"
                tabindex={-1}
                data-testid={node.kind === "collection" && row.collection === app.activeCollection ? "new-request" : undefined}
                label="New request in {node.name}"
                shortcut={node.kind === "collection" ? hint : undefined}
                onclick={() => startNew(row.collection, node.path)}
              />
              <IconButton
                icon="more"
                size="sm"
                tabindex={-1}
                data-testid="node-menu"
                label="More actions for {node.name}"
                aria-haspopup="menu"
                onclick={(e) => openMenu(row, (e.currentTarget as HTMLElement).closest(".row") as HTMLElement)}
              />
            </span>
          {/if}
        </div>
        {#if node.kind === "collection" && open}
          <div class="hint" role="none">
            <ExtensionHints target={{ place: "collection", collection: row.collection }} />
          </div>
          {#if row.collection === app.activeCollection && newIn === ""}{@render prompt(2)}{/if}
          {#if row.loading}
            <div class="row placeholder" role="none" style:--level={2}><Skeleton width="70%" label="Loading requests" /></div>
            <div class="row placeholder" role="none" style:--level={2}><Skeleton width="45%" label="Loading requests" /></div>
          {:else if isEmpty(row.collection) && !(row.collection === app.activeCollection && newIn === "")}
            <div class="row none" role="none" style:--level={2}>
              <span class="muted">No requests yet</span>
              <Button variant="ghost" size="sm" onclick={() => startNew(row.collection, "")}>New request</Button>
            </div>
          {/if}
        {/if}
        {#if node.kind === "folder" && row.collection === app.activeCollection && newIn === node.path}{@render prompt(row.level + 1)}{/if}
      {/each}
    </div>
  {:else}
    <div class="empty">
      <EmptyState icon="collection" size="sm" title="No collections yet" text="Open a folder with collections, or make one with New….">
        {#snippet actions()}
          <Button size="sm" variant="primary" icon="add" onclick={() => app.openNew({ collection: null })}>New…</Button>
          <Button size="sm" icon="folder-open" onclick={() => app.addCollection()}>Open folder…</Button>
        {/snippet}
      </EmptyState>
    </div>
  {/if}
</nav>

{#if menu}
  <Menu items={menu.items} anchor={menu.anchor} label={menu.label} bind:open={menuOpen} onclose={() => (menu = null)} />
{/if}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--rt-bg-panel);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-2);
    min-height: var(--rt-control-lg);
    padding: 0 var(--rt-space-3) 0 var(--rt-side-inset);
  }
  .title {
    font: var(--rt-font-caption);
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .tools {
    display: flex;
    gap: var(--rt-space-1);
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 0 var(--rt-space-3) var(--rt-space-4);
  }
  /* Indent per level: one twisty (16px) plus the gap, so a child's twisty sits under its
     parent's icon. */
  .row {
    --indent: calc(16px + var(--rt-space-3));
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-md);
    padding: 0 var(--rt-space-2) 0 calc(var(--rt-space-2) + (var(--level) - 1) * var(--indent));
    border-radius: var(--rt-radius-md);
    font: var(--rt-font-body);
    color: var(--rt-fg);
    cursor: default;
    white-space: nowrap;
    user-select: none;
    transition: background-color var(--rt-dur-fast) var(--rt-ease);
  }
  .row[role="treeitem"]:hover {
    background: var(--rt-bg-hover);
  }
  .row.active,
  .row.active:hover {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .row:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .row.collection {
    font-weight: var(--rt-weight-medium);
  }
  .row.collection:not(:first-child) {
    margin-top: var(--rt-space-2);
  }
  .twisty {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    color: var(--rt-fg-subtle);
    transition: transform var(--rt-dur-fast) var(--rt-ease);
  }
  .twisty.open {
    transform: rotate(90deg);
  }
  .glyph {
    flex: none;
    display: inline-flex;
    color: var(--rt-fg-muted);
  }
  .current .glyph {
    color: var(--rt-accent);
  }
  /* A request's method sits where a folder's twisty and icon are. */
  .method {
    flex: none;
    display: inline-flex;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip,
  .dirty {
    flex: none;
    display: inline-flex;
  }
  /* Row actions float over the end of the row while it is hovered or focused, so names keep
     their width otherwise. */
  .actions {
    position: absolute;
    top: 0;
    bottom: 0;
    right: var(--rt-space-1);
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-1);
    padding-left: var(--rt-space-4);
    background: linear-gradient(to right, transparent, var(--row-bg) var(--rt-space-4));
    opacity: 0;
    transition: opacity var(--rt-dur-fast) var(--rt-ease);
  }
  .row {
    --row-bg: var(--rt-bg-panel);
  }
  .row[role="treeitem"]:hover,
  .row:focus-within {
    --row-bg: var(--rt-bg-hover);
  }
  .row.active {
    --row-bg: var(--rt-accent-subtle);
  }
  .row:hover .actions,
  .row:focus-within .actions,
  .row:focus .actions {
    opacity: 1;
  }
  .hint {
    padding-left: calc(var(--rt-space-2) + 16px + var(--rt-space-3));
  }
  .hint:empty {
    display: none;
  }
  .placeholder,
  .none {
    padding-left: calc(var(--rt-space-2) + (var(--level) - 1) * var(--indent) + 16px + var(--rt-space-3));
  }
  .none {
    gap: var(--rt-space-4);
  }
  .muted {
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .new {
    gap: var(--rt-space-2);
  }
  .new select {
    flex: none;
    height: var(--rt-control-sm);
    font: var(--rt-font-small);
    color: var(--rt-fg);
    background: var(--rt-surface-2);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-sm);
  }
  .new input {
    flex: 1;
    min-width: 0;
    height: var(--rt-control-sm);
    box-sizing: border-box;
    font: var(--rt-font-body);
    color: var(--rt-fg);
    background: var(--rt-surface-2);
    border: 1px solid var(--rt-accent);
    border-radius: var(--rt-radius-sm);
    padding: 0 var(--rt-space-3);
  }
  .empty {
    padding: var(--rt-space-6) var(--rt-space-4);
  }
</style>
