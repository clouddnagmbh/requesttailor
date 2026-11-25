<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The response's "Views" tab (wave 18, strand G): the saved views of the request
     (`<request>.views.json`), each a JSONPath with a display (JSON, table, text). The core
     evaluates the view on the kept response; this tab gets the shaped rows, never the body.
     Views are made with "Save as view" next to the filter on the Body tab. -->
<script lang="ts">
  import type { ResponseView, ViewResult } from "../commands/types";
  import CodeView from "../response/CodeView.svelte";
  import type { ResponseSlotProps } from "../response/slots";
  import { Banner, Button, EmptyState } from "../ui";
  import { cellText, jsonOf, textLines, viewLabel } from "./model";
  import { bumpViews, viewsVersion } from "./views.svelte";

  let { app, requestPath, response }: ResponseSlotProps = $props();

  let views = $state<ResponseView[]>([]);
  let loaded = $state(false);
  let selected = $state<string | null>(null);
  let result = $state.raw<ViewResult | null>(null);
  let error = $state<string | null>(null);

  const current = $derived(views.find((v) => v.name === selected) ?? views[0] ?? null);

  $effect(() => {
    void viewsVersion();
    let live = true;
    app.backend.views
      .list(requestPath)
      .then((list) => {
        if (!live) return;
        views = list;
        loaded = true;
      })
      .catch((e) => {
        if (!live) return;
        views = [];
        loaded = true;
        error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      live = false;
    };
  });

  $effect(() => {
    const view = current;
    const id = response.response_id;
    result = null;
    error = null;
    if (!view || id == null) return;
    let live = true;
    app.backend.views
      .apply(requestPath, id, view)
      .then((r) => {
        if (live) result = r;
      })
      .catch((e) => {
        if (live) error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      live = false;
    };
  });

  async function remove(view: ResponseView) {
    try {
      await app.backend.views.remove(requestPath, view.name);
      await app.refreshDirty();
      selected = null;
      bumpViews();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="views" data-testid="views-panel">
  {#if loaded && views.length === 0}
    <EmptyState
      title="No saved views"
      text="Type a JSONPath in the filter on the Body tab, then choose “Save as view”. The view is kept next to the request and shows up here for every response."
      data-testid="views-empty"
    />
  {:else}
    <div class="picker" role="group" aria-label="Saved views">
      {#each views as v (v.name)}
        <button
          type="button"
          class="pick"
          aria-pressed={current?.name === v.name}
          data-testid="views-pick"
          data-name={v.name}
          onclick={() => (selected = v.name)}>{viewLabel(v)}</button
        >
      {/each}
    </div>
    {#if current}
      <div class="head">
        <code class="expr" data-testid="views-expression">{current.expression}</code>
        <Button size="sm" variant="ghost" data-testid="views-remove" onclick={() => remove(current)}>Remove view</Button>
      </div>
    {/if}
    {#if error}<Banner tone="danger" data-testid="views-error">{error}</Banner>{/if}
    {#if result}
      {#if result.notice}<p class="notice" data-testid="views-notice">{result.notice}</p>{/if}
      {#if result.display === "table" && (result.columns ?? []).length > 0}
        <div class="scroll" data-testid="views-table-wrap">
          <table data-testid="views-table">
            <thead>
              <tr>{#each result.columns ?? [] as c (c)}<th scope="col">{c}</th>{/each}</tr>
            </thead>
            <tbody>
              {#each result.rows ?? [] as row, i (i)}
                <tr data-testid="views-row">{#each row as cell, j (j)}<td>{cellText(cell)}</td>{/each}</tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if result.display === "text"}
        <pre class="text" data-testid="views-text">{textLines(result).join("\n")}</pre>
      {:else if result.values.length > 0}
        {#key result}
          <div class="fill" data-testid="views-json">
            <CodeView text={JSON.stringify(jsonOf(result), null, 2)} lang="json" label="View result" testid="views-json-code" />
          </div>
        {/key}
      {/if}
    {/if}
  {/if}
</div>

<style>
  .views {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-2) var(--rt-gap);
    font-size: var(--rt-size-small);
    overflow: auto;
  }
  .picker {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-2);
  }
  .pick {
    padding: var(--rt-space-1) var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-pill);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .pick[aria-pressed="true"] {
    background: var(--rt-surface-2);
    border-color: var(--rt-accent, currentColor);
  }
  .pick:focus-visible {
    outline: none;
    box-shadow: var(--rt-focus-ring);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-3);
  }
  .expr {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .notice {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .scroll {
    overflow: auto;
    min-height: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    padding: var(--rt-space-1) var(--rt-space-3);
    text-align: left;
    border-bottom: 1px solid var(--rt-border);
    font-family: var(--rt-font-mono);
    white-space: nowrap;
    max-width: 40ch;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--rt-surface-2);
    font-weight: 600;
  }
  .text {
    margin: 0;
    font-family: var(--rt-font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .fill {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
