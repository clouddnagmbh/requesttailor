<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Where a new item goes (wave 14, strand B): the collection and, unless `folders` is off, the
     folder inside it. Collections come from the sidebar (`AppState.collections`); a tree that the
     sidebar has not read yet is read once with `collections.tree`. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import type { TreeNode } from "../commands/types";
  import { folderOptions } from "./forms";

  let {
    collection = $bindable(),
    folder = $bindable(""),
    folders = true,
    error,
  }: { collection: string | null; folder?: string; folders?: boolean; error?: string } = $props();

  const app = getApp();
  let fetched = $state<Record<string, TreeNode | null>>({});

  const tree = $derived(collection ? (app.trees[collection] ?? fetched[collection] ?? null) : null);
  const options = $derived(folderOptions(tree));
  const collectionName = $derived(app.collections.find((c) => c.id === collection)?.name ?? "");

  $effect(() => {
    const id = collection;
    if (!folders || !id || app.trees[id] || id in fetched) return;
    fetched[id] = null;
    app.backend.collections.tree(id).then(
      (t) => (fetched[id] = t),
      () => {},
    );
  });

  // A folder that is not in the chosen collection falls back to its top level.
  $effect(() => {
    if (folders && tree && !options.some((o) => o.path === folder)) folder = "";
  });
</script>

<div class="target">
  <label class="field">
    <span class="label">Collection</span>
    <select data-testid="new-collection" bind:value={collection} disabled={app.collections.length === 0}>
      {#if app.collections.length === 0}<option value={null}>No collection open</option>{/if}
      {#each app.collections as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
    </select>
  </label>
  {#if folders}
    <label class="field">
      <span class="label">Folder</span>
      <select data-testid="new-folder" bind:value={folder} disabled={!collection}>
        {#each options as o (o.path)}
          <option value={o.path}>{o.depth === 0 ? `${collectionName || "Collection"} (top level)` : `${" ".repeat(o.depth - 1)}${o.name}`}</option>
        {/each}
      </select>
    </label>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .target {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--rt-gap);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .error {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--rt-danger);
    font-size: var(--rt-size-small);
  }
</style>
