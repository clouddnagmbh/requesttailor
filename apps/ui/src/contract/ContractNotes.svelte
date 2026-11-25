<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Breaking changes of the OpenAPI spec between two revisions, as hints (wave 12, strand F),
     and of a GraphQL schema and Protobuf files (wave 18, strand H; each tagged with what it is).
     Shown by the semantic diff (strand A) and the PR review (strand E); renders nothing when
     there are none, when the repository has no spec or schema on either side, or when the
     answer cannot be had (a hint is never an error). -->
<script lang="ts">
  import type { SpecChange, SpecChanges } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Banner } from "../ui";

  let { from, to }: { from: string; to: string } = $props();
  const app = getApp();

  /** A breaking change and where it comes from: `null` for the OpenAPI spec. */
  interface Note {
    change: SpecChange;
    source: string | null;
  }

  let breaking = $state<Note[]>([]);

  /** The breaking changes of one answer; none when it cannot be had. */
  async function notes(source: string | null, answer: Promise<SpecChanges>): Promise<Note[]> {
    try {
      return (await answer).changes.filter((x) => x.breaking).map((change) => ({ change, source }));
    } catch {
      return [];
    }
  }

  $effect(() => {
    const [a, b] = [from, to];
    let current = true;
    const contract = app.backend.contract;
    Promise.all([
      notes(null, contract.changes(a, b)),
      notes("GraphQL", contract.schemaChanges("graphql", a, b)),
      notes("Protobuf", contract.schemaChanges("protobuf", a, b)),
    ]).then((all) => {
      if (current) breaking = all.flat();
    });
    return () => {
      current = false;
    };
  });
</script>

{#if breaking.length > 0}
  <aside class="notes" data-testid="contract-notes" aria-label="Breaking changes of the API">
    <Banner tone="danger" title="Breaking changes of the API ({breaking.length})">
      <ul>
        {#each breaking as { change: c, source }, i (i)}
          <li data-testid="contract-note" data-source={source ?? "OpenAPI"}>
            {#if source}<span class="source">{source}</span>{/if}
            {#if c.operation}<code>{c.operation}</code>{/if}
            <span>{c.message}</span>
            {#if c.requests.length > 0}
              <span class="affects">affects {c.requests.join(", ")}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </Banner>
  </aside>
{/if}

<style>
  ul {
    list-style: none;
    margin: var(--rt-space-3) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-space-2) var(--rt-space-4);
    overflow-wrap: anywhere;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    font-weight: var(--rt-weight-semibold);
  }
  .source {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-sm);
    border: 1px solid var(--rt-danger-border);
    font: var(--rt-font-small);
  }
  .affects {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
</style>
