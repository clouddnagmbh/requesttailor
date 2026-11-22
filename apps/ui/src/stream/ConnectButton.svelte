<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Connect/Disconnect at the end of the URL row of a session request (wave 8, strand A), in
     place of Send. The Send shortcut connects too (see `SessionsState.primary`). -->
<script lang="ts">
  import type { SessionsState } from "./sessions.svelte";

  let { path, sessions, shortcut }: { path: string; sessions: SessionsState; shortcut: string } = $props();

  const entry = $derived(sessions.entries[path]);
  const live = $derived(entry?.live ?? false);
</script>

<button
  class="button primary connect"
  class:live
  data-testid="connect"
  title={live ? "Disconnect" : `Connect (${shortcut})`}
  disabled={entry?.status === "connecting" && entry.id === null}
  onclick={() => sessions.toggle(path)}
  >{live ? (entry?.status === "connecting" ? "Connecting…" : "Disconnect") : "Connect"}{#if !live}<kbd>{shortcut}</kbd>{/if}</button
>

<style>
  .connect {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding-inline: 16px;
    font: inherit;
    border: 1px solid var(--rt-accent);
    border-radius: var(--rt-radius);
    background: var(--rt-accent);
    color: var(--rt-accent-fg);
    cursor: pointer;
    white-space: nowrap;
  }
  .connect.live {
    background: var(--rt-bg);
    color: var(--rt-fg);
    border-color: var(--rt-border);
  }
  .connect:disabled {
    opacity: 0.6;
    cursor: default;
  }
  kbd {
    font-size: var(--rt-size-small);
    opacity: 0.75;
  }
</style>
