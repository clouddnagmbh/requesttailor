<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One request of a comparison (wave 14, strand H): what changed in its definition, field by
     field side by side, and how its answers differ, side by side; breaking changes of the API
     contract that name it. -->
<script lang="ts">
  import type { CompareRow } from "./rows";
  import DefinitionSides from "./DefinitionSides.svelte";
  import ResponseSides from "./ResponseSides.svelte";

  let {
    row,
    ran,
    running,
    onrun,
    onopen,
    onhistory,
  }: {
    row: CompareRow;
    ran: boolean;
    running: boolean;
    onrun: () => void;
    onopen: (path: string) => void;
    onhistory: (path: string) => void;
  } = $props();

  const STATE = { new: "New", removed: "Removed", changed: "Changed", same: "Same" } as const;
  const definitionFields = $derived(row.definition?.fields.length ?? 0);
</script>

<article class="detail" data-testid="compare-detail" data-path={row.path} aria-label={row.name}>
  <header>
    <div class="title">
      <span class="state {row.state}">{STATE[row.state]}</span>
      <h3>{row.name}</h3>
    </div>
    <div class="path">
      {#if row.oldPath}<span class="mono">{row.oldPath}</span> <span aria-hidden="true">→</span>{/if}
      <span class="mono">{row.path}</span>
    </div>
    <div class="actions">
      {#if row.state !== "removed"}
        <button type="button" class="link" data-testid="compare-open-request" onclick={() => onopen(row.path)}>Open request</button>
        <button type="button" class="link" onclick={() => onhistory(row.path)}>History</button>
      {/if}
    </div>
  </header>

  {#if row.breaking.length > 0}
    <aside class="breaking" data-testid="compare-breaking">
      <strong>Breaking change of the API</strong>
      <ul>
        {#each row.breaking as c, i (i)}
          <li>{#if c.operation}<code>{c.operation}</code> {/if}{c.message}</li>
        {/each}
      </ul>
    </aside>
  {/if}

  <section>
    <h4>Definition</h4>
    {#if row.state === "new"}
      <p class="muted">New at the newer revision.</p>
    {:else if row.state === "removed"}
      <p class="muted">Not in the newer revision any more.</p>
    {/if}
    {#if row.definition && definitionFields > 0}
      <DefinitionSides change={row.definition} />
    {:else if row.state !== "new" && row.state !== "removed"}
      <p class="muted" data-testid="compare-definition-same">{row.oldPath ? "Only moved; the fields are the same." : "The request is the same at both revisions."}</p>
    {/if}
  </section>

  <section>
    <h4>Response</h4>
    {#if row.response}
      {#if row.response.sides}
        <ResponseSides response={row.response} />
      {:else}
        <p class="muted">This comparison has no answers to show side by side.</p>
      {/if}
    {:else if running}
      <p class="muted">Sending the requests at both revisions…</p>
    {:else}
      <div class="empty" data-testid="compare-not-run">
        <p>{ran ? "This request was not sent in this run." : "Send the requests at both revisions to compare their answers, times and tests."}</p>
        {#if !ran}<button type="button" class="button" data-testid="compare-run" onclick={onrun}>Send and compare answers</button>{/if}
      </div>
    {/if}
  </section>
</article>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 12px 16px 24px;
    min-width: 0;
  }
  header {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 12px;
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: 15px;
    overflow-wrap: anywhere;
  }
  .path {
    grid-column: 1;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    overflow-wrap: anywhere;
  }
  .actions {
    grid-column: 2;
    grid-row: 1 / span 2;
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .mono,
  code {
    font-family: var(--rt-font-mono);
  }
  .state {
    font-size: var(--rt-size-small);
    font-weight: 600;
    padding: 0 6px;
    border-radius: var(--rt-radius);
    border: 1px solid currentcolor;
    color: var(--rt-fg-muted);
  }
  .state.changed {
    color: var(--rt-warn);
  }
  .state.new {
    color: var(--rt-ok);
  }
  .state.removed {
    color: var(--rt-danger);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h4 {
    margin: 0;
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .breaking {
    padding: 8px 10px;
    border: 1px solid var(--rt-danger);
    border-radius: var(--rt-radius);
    font-size: var(--rt-size-small);
  }
  .breaking strong {
    color: var(--rt-danger);
  }
  .breaking ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }
  .empty {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 12px;
    border: 1px dashed var(--rt-border);
    border-radius: var(--rt-radius);
    color: var(--rt-fg-muted);
  }
  .empty p {
    margin: 0;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--rt-accent);
    cursor: pointer;
  }
  .button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
  }
</style>
