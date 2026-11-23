<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Breaking changes of the OpenAPI spec between two revisions, as hints (wave 12, strand F).
     Shown by the semantic diff (strand A) and the PR review (strand E); renders nothing when
     there are none, when the repository has no spec on either side, or when the answer cannot
     be had (a hint is never an error). -->
<script lang="ts">
  import type { SpecChange } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Banner } from "../ui";

  let { from, to }: { from: string; to: string } = $props();
  const app = getApp();

  let breaking = $state<SpecChange[]>([]);

  $effect(() => {
    const [a, b] = [from, to];
    let current = true;
    app.backend.contract
      .changes(a, b)
      .then((c) => {
        if (current) breaking = c.changes.filter((x) => x.breaking);
      })
      .catch(() => {
        if (current) breaking = [];
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
        {#each breaking as c, i (i)}
          <li data-testid="contract-note">
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
  .affects {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
</style>
