<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- `SendReport.notes` of a send: hints from auth (`source: "auth"`) and plugins
     (`plugin:<id>`), shown above the response. Owner: wave 5, strand A. -->
<script lang="ts">
  import type { Note } from "../commands/types";

  let { notes }: { notes: Note[] } = $props();

  const label = (source: string) => (source.startsWith("plugin:") ? `Plugin ${source.slice(7)}` : source === "auth" ? "Auth" : source);
</script>

{#if notes.length > 0}
  <ul class="notes" data-testid="send-notes" aria-label="Notes">
    {#each notes as n, i (i)}
      <li class={n.level} data-testid="send-note" data-source={n.source} data-level={n.level}>
        <span class="source">{label(n.source)}</span>
        <span class="text">{n.text}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .notes {
    margin: 0;
    padding: 4px var(--rt-gap);
    list-style: none;
    border-bottom: 1px solid var(--rt-border);
    font-size: var(--rt-size-small);
  }
  li {
    display: flex;
    gap: var(--rt-gap);
    padding: 1px 0 1px var(--rt-gap);
    border-left: 3px solid var(--rt-accent);
  }
  li.warning {
    border-color: var(--rt-warn);
  }
  li.error {
    border-color: var(--rt-danger);
  }
  li.error .text {
    color: var(--rt-danger);
  }
  .source {
    color: var(--rt-fg-muted);
    white-space: nowrap;
  }
  .text {
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
