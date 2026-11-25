<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Save as view" (wave 18, strand G): names the JSONPath of the response filter and picks how
     it shows (JSON, table with columns, text). Saving stages `<request>.views.json` next to the
     request like any edit; nothing is written until the collection is saved, and the file holds
     the expression, never a value of the response. A name that exists is replaced. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { ResponseView, ViewDisplay } from "../commands/types";
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import { Banner, Button, DialogFooter, SegmentedControl, TextField } from "../ui";
  import { DISPLAYS, nameProblem, replaces, splitColumns } from "./model";
  import { bumpViews } from "./views.svelte";

  let { close, props }: ExtensionDialogProps = $props();
  const app = getApp();
  // svelte-ignore state_referenced_locally
  const given = props as { path: string; expression: string };

  let name = $state("");
  let display = $state<ViewDisplay>("json");
  let columns = $state("");
  let existing = $state<ResponseView[]>([]);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const problem = $derived(nameProblem(name));
  const replacing = $derived(replaces(name, existing));

  onMount(() => {
    app.backend.views
      .list(given.path)
      .then((list) => (existing = list))
      .catch(() => (existing = []));
  });

  async function save() {
    if (problem !== null || busy) return;
    busy = true;
    error = null;
    const view: ResponseView = {
      name: name.trim(),
      language: "jsonpath",
      expression: given.expression,
      display,
      ...(display === "table" ? { columns: splitColumns(columns) } : {}),
    };
    try {
      await app.backend.views.save(given.path, view);
      await app.refreshDirty();
      bumpViews();
      close(view.name);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      void save();
    }
  }
</script>

<div class="body" data-testid="save-view-dialog">
  <p class="expr">
    <span class="label">Expression</span>
    <code data-testid="save-view-expression">{given.expression}</code>
  </p>
  <TextField
    label="Name"
    bind:value={name}
    {onkeydown}
    placeholder="e.g. Active users"
    autocomplete="off"
    spellcheck="false"
    data-testid="save-view-name"
    data-autofocus
  />
  <div class="field">
    <span class="label" id="save-view-display-label">Show as</span>
    <SegmentedControl
      label="Show as"
      options={DISPLAYS.map((d) => ({ value: d.value, label: d.label }))}
      bind:value={display}
      data-testid="save-view-display"
    />
  </div>
  {#if display === "table"}
    <TextField
      label="Columns (optional, comma separated)"
      bind:value={columns}
      {onkeydown}
      placeholder="id, name, email"
      autocomplete="off"
      spellcheck="false"
      data-testid="save-view-columns"
    />
  {/if}
  {#if replacing}
    <p class="note" data-testid="save-view-replaces">A view named “{name.trim()}” exists and will be replaced.</p>
  {/if}
  {#if error}<Banner tone="danger" data-testid="save-view-error">{error}</Banner>{/if}
  <p class="note">
    The view is kept next to the request as a small file with the expression only, never a value of the response. It is
    written when you save the collection.
  </p>
</div>
<DialogFooter>
  <Button variant="ghost" onclick={() => close()}>Cancel</Button>
  <Button variant="primary" data-testid="save-view-save" disabled={problem !== null || busy} onclick={save}>Save view</Button>
</DialogFooter>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .expr {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
    margin: 0;
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  code {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-1);
  }
  .note {
    margin: 0;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
</style>
