<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sidebar: the collection's folder tree (ARIA tree, roving focus, type-ahead) and the inline
     "new request" name prompt. `newIn` is the folder the prompt is open for (`""` = root).
     `onrun` (wave 7): a folder row's run button opens the collection runner for it.
     Wave 8 (strand A): the prompt offers the kind of the new request (HTTP, GraphQL, gRPC,
     WebSocket). -->
<script lang="ts">
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { getApp } from "../state/app.svelte";
  import { asKind, NEW_REQUEST_KINDS, type NewRequestKind } from "../stream/kind";
  import MethodBadge from "./MethodBadge.svelte";
  import { rowKey, typeahead, visibleRows, type Row } from "./tree";

  let {
    newIn = $bindable(null),
    hint,
    onrun,
  }: { newIn?: string | null; hint: string; onrun?: (folder: string, name: string) => void } = $props();

  const app = getApp();
  const collapsed = new SvelteSet<string>();
  const rows = $derived(visibleRows(app.tree, collapsed));
  let focusKey = $state<string | null>(null);
  let newName = $state("");
  let newKind = $state<NewRequestKind>("http");
  let treeEl = $state<HTMLElement>();

  const focusIndex = $derived(Math.max(0, rows.findIndex((r) => r.key === focusKey)));

  /** Moves keyboard focus into the tree: the active request's row, or the first row. */
  export async function focus(): Promise<void> {
    if (app.active && !rows.some((r) => r.key === focusKey && r.node.path === app.active)) {
      const k = rowKey({ kind: "request", path: app.active, name: "", method: "" });
      if (rows.some((r) => r.key === k)) focusKey = k;
    }
    await focusRow(rows[focusIndex]);
  }

  async function focusRow(row: Row | undefined) {
    if (!row) return;
    focusKey = row.key;
    await tick();
    treeEl?.querySelector<HTMLElement>(`[data-key="${CSS.escape(row.key)}"]`)?.focus();
  }

  function toggle(row: Row, open = collapsed.has(row.node.path)) {
    if (row.node.kind !== "folder") return;
    if (open) collapsed.delete(row.node.path);
    else collapsed.add(row.node.path);
  }

  function activate(row: Row) {
    focusKey = row.key;
    if (row.node.kind === "request") app.select(row.node.path);
    else toggle(row);
  }

  function onkeydown(e: KeyboardEvent, row: Row, i: number) {
    if (e.target !== e.currentTarget || e.metaKey || e.ctrlKey || e.altKey) return;
    const isFolder = row.node.kind === "folder";
    const open = isFolder && !collapsed.has(row.node.path);
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
        if (isFolder && !open) toggle(row, true);
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

  function startNew(folder: string) {
    newIn = folder;
  }

  // The prompt may also be opened from outside (Mod+N, palette): show it, start empty.
  $effect.pre(() => {
    if (newIn === null) return;
    collapsed.delete(newIn);
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

<nav data-testid="sidebar" aria-label="Collection" class="sidebar">
  <div class="head">
    <span class="title">Requests</span>
    {#if app.collection}
      <button
        class="icon"
        data-testid="new-request"
        aria-label="New request"
        title="New request ({hint})"
        onclick={() => startNew("")}>+</button
      >
    {/if}
  </div>
  {#if app.tree}
    <div class="tree" role="tree" aria-label="Requests" bind:this={treeEl}>
      {#if newIn === ""}{@render prompt(1)}{/if}
      {#each rows as row, i (row.key)}
        {@const node = row.node}
        <div
          class="row"
          class:active={node.kind === "request" && node.path === app.active}
          role="treeitem"
          tabindex={i === focusIndex ? 0 : -1}
          data-key={row.key}
          data-path={node.path}
          data-testid={node.kind === "folder" ? "tree-folder" : "tree-request"}
          aria-level={row.level}
          aria-posinset={row.posinset}
          aria-setsize={row.setsize}
          aria-expanded={node.kind === "folder" ? !collapsed.has(node.path) : undefined}
          aria-selected={node.kind === "request" ? node.path === app.active : undefined}
          style:--level={row.level}
          onclick={() => activate(row)}
          onkeydown={(e) => onkeydown(e, row, i)}
          onfocus={() => (focusKey = row.key)}
        >
          {#if node.kind === "folder"}
            <span class="chevron" aria-hidden="true">{collapsed.has(node.path) ? "▸" : "▾"}</span>
            <span class="name">{node.name}</span>
            {#if onrun}
              <button
                class="icon add"
                tabindex="-1"
                data-testid="run-folder"
                aria-label="Run {node.name}"
                title="Run the requests in {node.name}"
                onclick={(e) => {
                  e.stopPropagation();
                  onrun(node.path, node.name);
                }}>▶</button
              >
            {/if}
            <button
              class="icon add"
              tabindex="-1"
              aria-label="New request in {node.name}"
              title="New request in {node.name}"
              onclick={(e) => {
                e.stopPropagation();
                startNew(node.path);
              }}>+</button
            >
          {:else}
            <MethodBadge method={node.method} />
            <span class="name">{node.name}</span>
          {/if}
        </div>
        {#if node.kind === "folder" && newIn === node.path}{@render prompt(row.level + 1)}{/if}
      {/each}
    </div>
  {:else}
    <p class="empty">Open a folder to see its requests.</p>
  {/if}
</nav>

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
    padding: 6px var(--rt-gap) 6px var(--rt-side-inset);
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding-bottom: var(--rt-gap);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding: 0 var(--rt-gap) 0 calc(var(--rt-side-inset) + (var(--level) - 1) * 14px);
    cursor: default;
    white-space: nowrap;
    user-select: none;
  }
  .row:hover {
    background: var(--rt-bg-hover);
  }
  .row.active {
    background: var(--rt-bg-selected);
  }
  .row:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .chevron {
    width: 10px;
    color: var(--rt-fg-muted);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .icon {
    border: 0;
    background: transparent;
    color: var(--rt-fg-muted);
    font: inherit;
    font-size: 15px;
    line-height: 1;
    width: 22px;
    height: 22px;
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--rt-bg-hover);
    color: var(--rt-fg);
  }
  .add {
    opacity: 0;
  }
  .row:hover .add,
  .row:focus-within .add,
  .row:focus .add {
    opacity: 1;
  }
  .new select {
    flex: none;
    font: inherit;
    font-size: var(--rt-size-small);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .new input {
    flex: 1;
    min-width: 0;
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-accent);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .empty {
    padding: 0 12px;
    color: var(--rt-fg-muted);
  }
</style>
