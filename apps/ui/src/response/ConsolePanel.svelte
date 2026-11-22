<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The response's "Console" tab (wave 7, strand C): what scripts logged (`console.*`), with
     level and origin, in order. Secret values are masked by the core. -->
<script lang="ts">
  import type { ScriptLog } from "../commands/types";

  let { logs }: { logs: ScriptLog[] } = $props();
</script>

<div class="console" data-testid="console-panel">
  {#if logs.length === 0}
    <p class="empty" data-testid="console-empty">No script output.</p>
  {:else}
    <ol aria-label="Script output">
      {#each logs as l, i (i)}
        <li class={l.level} data-testid="console-line" data-level={l.level}>
          <span class="level">{l.level}</span>
          <span class="text">{l.text}</span>
          {#if l.origin !== "request"}<span class="origin">{l.origin}</span>{/if}
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .console {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 4px var(--rt-gap);
    font: var(--rt-size-small) / 18px var(--rt-font-mono);
  }
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    gap: var(--rt-gap);
    padding: 1px 0;
    border-bottom: 1px solid var(--rt-border);
  }
  .level {
    flex: 0 0 3.5em;
    color: var(--rt-fg-muted);
  }
  .text {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .origin {
    color: var(--rt-fg-muted);
  }
  .warn .text,
  .warn .level {
    color: var(--rt-warn);
  }
  .error .text,
  .error .level {
    color: var(--rt-danger);
  }
  .empty {
    margin: 0;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-ui);
  }
</style>
