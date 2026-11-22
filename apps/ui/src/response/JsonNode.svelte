<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One node of the JSON tree. Children exist in the DOM only while expanded, and long
     arrays or objects show their children in pages. With `onpick` (wave 7), every node has a
     "set as variable" button, shown on hover and keyboard focus. -->
<script lang="ts">
  import JsonNode from "./JsonNode.svelte";

  let {
    name,
    value,
    path,
    expanded = false,
    onpick,
  }: {
    name: string | null;
    value: unknown;
    path: string;
    expanded?: boolean;
    /** "Set as variable" for this node (wave 7). */
    onpick?: (path: string, value: unknown) => void;
  } = $props();

  const PAGE = 200;

  // Initial state only; the user toggles from there.
  // svelte-ignore state_referenced_locally
  let open = $state(expanded);
  let shown = $state(PAGE);

  const isArray = $derived(Array.isArray(value));
  const isContainer = $derived(value !== null && typeof value === "object");
  const entries = $derived.by((): [string, unknown][] => {
    if (!open || !isContainer) return [];
    const all: [string, unknown][] = isArray
      ? (value as unknown[]).map((v, i) => [String(i), v])
      : Object.entries(value as Record<string, unknown>);
    return all.slice(0, shown);
  });
  const total = $derived(
    isContainer ? (isArray ? (value as unknown[]).length : Object.keys(value as object).length) : 0,
  );

  const IDENT = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
  function childPath(key: string): string {
    if (isArray) return `${path}[${key}]`;
    return IDENT.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`;
  }

  const kind = $derived(
    value === null ? "null" : typeof value === "string" ? "string" : typeof value === "number" ? "number" : "boolean",
  );
</script>

<li data-path={path}>
  {#if isContainer}
    <button type="button" class="toggle" aria-expanded={open} onclick={() => (open = !open)}>
      <span class="caret" aria-hidden="true">{open ? "▾" : "▸"}</span>
      {#if name !== null}<span class="key">{name}</span>{/if}
      <span class="summary">{isArray ? `[${total}]` : `{${total}}`}</span>
    </button>
    {#if onpick}{@render pick()}{/if}
    {#if open}
      <ul>
        {#each entries as [key, child] (key)}
          <JsonNode name={key} value={child} path={childPath(key)} {onpick} />
        {/each}
        {#if total > shown}
          <li>
            <button type="button" class="more" onclick={() => (shown += PAGE)}>
              Show {Math.min(PAGE, total - shown)} more of {total - shown}
            </button>
          </li>
        {/if}
      </ul>
    {/if}
  {:else}
    <span class="leaf">
      {#if name !== null}<span class="key">{name}</span><span class="colon">:</span>{/if}
      <span class={kind}>{JSON.stringify(value)}</span>
    </span>
    {#if onpick}{@render pick()}{/if}
  {/if}
</li>

{#snippet pick()}
  <button
    type="button"
    class="pick"
    data-testid="json-set-variable"
    data-path={path}
    aria-label="Set {path} as variable"
    title="Set as variable"
    onclick={() => onpick?.(path, value)}>{"{{x}}"}</button
  >
{/snippet}

<style>
  li {
    list-style: none;
    white-space: nowrap;
  }
  ul {
    margin: 0;
    padding-left: 16px;
  }
  .toggle,
  .more {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0 4px 0 0;
    cursor: pointer;
    border-radius: var(--rt-radius);
  }
  .toggle:hover,
  .more:hover {
    background: var(--rt-bg-hover);
  }
  .more {
    color: var(--rt-accent);
  }
  .pick {
    margin-left: 6px;
    padding: 0 4px;
    font: inherit;
    font-size: 0.9em;
    color: var(--rt-accent);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
    opacity: 0;
  }
  .toggle:hover + .pick,
  .leaf:hover + .pick,
  .pick:hover,
  .pick:focus-visible {
    opacity: 1;
  }
  .pick:hover {
    background: var(--rt-bg-hover);
  }
  .caret {
    display: inline-block;
    width: 12px;
    color: var(--rt-fg-muted);
  }
  .leaf {
    padding-left: 12px;
  }
  .key {
    color: var(--rt-fg);
  }
  .colon {
    margin-right: 4px;
    color: var(--rt-fg-muted);
  }
  .summary {
    color: var(--rt-fg-muted);
    margin-left: 4px;
  }
  .string {
    color: var(--rt-ok);
  }
  .number {
    color: var(--rt-accent);
  }
  .boolean,
  .null {
    color: var(--rt-warn);
  }
</style>
