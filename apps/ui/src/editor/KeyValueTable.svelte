<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Name/value rows with enable toggle, add and remove. Used for query and path params,
     headers, form fields, multipart parts and request variables. -->
<script lang="ts" module>
  export interface Row {
    name: string;
    value: string;
    enabled: boolean;
    /** Shown instead of an editable value (e.g. "secret"). */
    lockedValue?: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  import CodeField from "./CodeField.svelte";
  import type { VarLookup } from "./vars";

  let {
    rows,
    lookup,
    testid,
    label,
    fixedNames = false,
    namePlaceholder = "name",
    valuePlaceholder = "value",
    onchange,
    onadd,
    onremove,
    extra,
    extraHeader,
  }: {
    rows: Row[];
    lookup: VarLookup;
    testid: string;
    label: string;
    /** Names come from elsewhere (path params): no add, remove or renaming. */
    fixedNames?: boolean;
    namePlaceholder?: string;
    valuePlaceholder?: string;
    onchange: (index: number, patch: Partial<Row>) => void;
    onadd?: () => void;
    onremove?: (index: number) => void;
    /** Extra cells after the value (multipart: kind and content type). */
    extra?: Snippet<[number]>;
    extraHeader?: Snippet;
  } = $props();
</script>

<div class="table" data-testid={testid} role="group" aria-label={label}>
  {#if rows.length > 0}
    <div class="row head" class:has-extra={!!extra} aria-hidden="true">
      <span></span>
      <span>Name</span>
      <span>Value</span>
      {#if extra}<span class="extra">{#if extraHeader}{@render extraHeader()}{/if}</span>{/if}
      <span></span>
    </div>
  {/if}
  {#each rows as row, i}
    <div class="row" class:has-extra={!!extra} class:disabled={!row.enabled} data-testid="kv-row">
      <input
        type="checkbox"
        data-testid="kv-enabled"
        aria-label={`Enable ${row.name || "row " + (i + 1)}`}
        checked={row.enabled}
        onchange={(e) => onchange(i, { enabled: e.currentTarget.checked })}
      />
      <CodeField
        testid="kv-name"
        label={`${label} name ${i + 1}`}
        value={row.name}
        {lookup}
        readonly={fixedNames}
        placeholder={namePlaceholder}
        onchange={(v) => onchange(i, { name: v })}
      />
      {#if row.lockedValue !== undefined}
        <span class="locked" data-testid="kv-value">{row.lockedValue}</span>
      {:else}
        <CodeField
          testid="kv-value"
          label={`${label} value ${i + 1}`}
          value={row.value}
          {lookup}
          placeholder={valuePlaceholder}
          onchange={(v) => onchange(i, { value: v })}
        />
      {/if}
      {#if extra}<div class="extra">{@render extra(i)}</div>{/if}
      {#if !fixedNames && onremove}
        <button class="icon" data-testid="kv-remove" aria-label={`Remove ${row.name || "row " + (i + 1)}`} onclick={() => onremove(i)}
          >×</button
        >
      {:else}
        <span></span>
      {/if}
    </div>
  {/each}
  {#if !fixedNames && onadd}
    <button class="add" data-testid="kv-add" onclick={onadd}>+ Add</button>
  {/if}
</div>

<style>
  .table {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .row {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) minmax(0, 2fr) 24px;
    gap: 6px;
    align-items: center;
  }
  .row.head {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .row.disabled :global(.cm-content) {
    opacity: 0.55;
  }
  .row.has-extra {
    grid-template-columns: 20px minmax(0, 1fr) minmax(0, 2fr) minmax(0, 1.2fr) 24px;
  }
  .extra {
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
  }
  .locked {
    color: var(--rt-var-secret);
    font-style: italic;
    padding: 0 6px;
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  button:hover {
    background: var(--rt-bg-hover);
  }
  .icon {
    width: 24px;
    height: 24px;
    padding: 0;
    color: var(--rt-fg-muted);
  }
  .add {
    align-self: flex-start;
    padding: 2px 10px;
  }
</style>
