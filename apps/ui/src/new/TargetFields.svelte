<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Where a new item goes (wave 14, strand B): the collection and, unless `folders` is off, the
     folder inside it. Collections come from the sidebar (`AppState.collections`); a tree that the
     sidebar has not read yet is read once with `collections.tree`. -->
<script lang="ts">
  import type { TreeNode } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Select, type SelectOption } from "../ui";
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
  const collectionName = $derived(app.collections.find((c) => c.id === collection)?.name ?? "Collection");

  const collectionOptions = $derived<SelectOption[]>(
    app.collections.length === 0
      ? [{ value: "", label: "No collection open", disabled: true }]
      : app.collections.map((c) => ({ value: c.id, label: c.name })),
  );
  // An em space per level keeps the nesting readable in the native list.
  const folderSelectOptions = $derived<SelectOption[]>(
    options.map((o) => ({
      value: o.path,
      label: o.depth === 0 ? `${collectionName} (top level)` : `${" ".repeat(o.depth - 1)}${o.name}`,
    })),
  );

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

<div class="target" class:single={!folders}>
  <Select
    label="Collection"
    icon="collection"
    options={collectionOptions}
    bind:value={() => collection ?? "", (v) => (collection = v === "" ? null : v)}
    disabled={app.collections.length === 0}
    error={error}
    data-testid="new-collection"
  />
  {#if folders}
    <Select label="Folder" icon="folder" options={folderSelectOptions} bind:value={folder} disabled={!collection} data-testid="new-folder" />
  {/if}
</div>

<style>
  .target {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--rt-space-5);
  }
  .single {
    grid-template-columns: minmax(0, 1fr);
  }
</style>
