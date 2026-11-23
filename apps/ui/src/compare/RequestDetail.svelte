<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One request of a comparison (wave 14, strand H): what changed in its definition, field by
     field side by side, and how its answers differ, side by side; breaking changes of the API
     contract that name it. -->
<script lang="ts">
  import { Badge, Banner, Button, EmptyState, Spinner } from "../ui";
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
  const STATUS = { new: "A", removed: "D", changed: "M", same: "" } as const;
  const definitionFields = $derived(row.definition?.fields.length ?? 0);
</script>

<article class="detail" data-testid="compare-detail" data-path={row.path} aria-label={row.name}>
  <header>
    <div class="title">
      {#if row.state !== "same"}<Badge kind="status" value={STATUS[row.state]} />{/if}
      <h3>{row.name}</h3>
      {#if row.state === "same"}<span class="state">{STATE[row.state]}</span>{/if}
    </div>
    <div class="path">
      {#if row.oldPath}<span class="mono">{row.oldPath}</span> <span aria-hidden="true">→</span>{/if}
      <span class="mono">{row.path}</span>
    </div>
    <div class="actions">
      {#if row.state !== "removed"}
        <Button size="sm" variant="ghost" icon="history" onclick={() => onhistory(row.path)}>History</Button>
        <Button size="sm" icon="external-link" data-testid="compare-open-request" onclick={() => onopen(row.path)}>Open request</Button>
      {/if}
    </div>
  </header>

  {#if row.breaking.length > 0}
    <div data-testid="compare-breaking">
      <Banner tone="danger" title="Breaking change of the API">
        <ul class="breaking">
          {#each row.breaking as c, i (i)}
            <li>{#if c.operation}<code>{c.operation}</code> {/if}{c.message}</li>
          {/each}
        </ul>
      </Banner>
    </div>
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
      <p class="muted running"><Spinner size="sm" label="Sending" /> Sending the requests at both revisions…</p>
    {:else}
      <div class="not-run" data-testid="compare-not-run">
        <EmptyState size="sm" icon="send" title={ran ? "Not sent" : "Answers not compared yet"} text={ran ? "This request was not sent in this run." : "Send the requests at both revisions to compare their answers, times and tests."}>
          {#snippet actions()}
            {#if !ran}<Button variant="primary" icon="send" data-testid="compare-run" onclick={onrun}>Send and compare answers</Button>{/if}
          {/snippet}
        </EmptyState>
      </div>
    {/if}
  </section>
</article>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-7);
    padding: var(--rt-space-6) var(--rt-space-6) var(--rt-space-7);
    min-width: 0;
  }
  header {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--rt-space-2) var(--rt-space-5);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    min-width: 0;
  }
  h3 {
    margin: 0;
    font: var(--rt-font-heading);
    overflow-wrap: anywhere;
  }
  .state {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .path {
    grid-column: 1;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    overflow-wrap: anywhere;
  }
  .actions {
    grid-column: 2;
    grid-row: 1 / span 2;
    display: flex;
    gap: var(--rt-space-4);
    align-items: center;
  }
  .mono,
  code {
    font-family: var(--rt-font-mono);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  h4 {
    margin: 0;
    padding-bottom: var(--rt-space-3);
    border-bottom: 1px solid var(--rt-border);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .running {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .breaking {
    margin: 0;
    padding-left: var(--rt-space-6);
  }
  .not-run {
    padding: var(--rt-space-6);
    border: 1px dashed var(--rt-border-strong);
    border-radius: var(--rt-radius-lg);
  }
</style>
