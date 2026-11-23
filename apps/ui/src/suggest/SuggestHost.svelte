<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The note after a suggestion was applied (wave 16, strand C): what changed, "Undo", and where
     to discard what cannot be undone. Bottom of the window, away from the editor, never modal. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { Button, Icon, IconButton } from "../ui";
  import { suggestUndoFor } from "./undo.svelte";

  const app = getApp();
  const store = suggestUndoFor(app);
  const last = $derived(store.last);

  // The note goes away by itself; an undo result stays a little shorter.
  $effect(() => {
    const seq = last?.seq;
    const msg = store.message;
    if (seq === undefined && !msg) return;
    const t = setTimeout(() => store.dismiss(), msg ? 5000 : 20000);
    return () => clearTimeout(t);
  });
</script>

{#if last || store.message}
  <div class="toast" role="status" data-testid="suggest-toast">
    <span class="ok"><Icon name={last ? "success" : "info"} size="sm" /></span>
    <span class="text">
      {#if last}
        <strong>Suggestion applied.</strong> {last.title}{last.subtitle ? `: ${last.subtitle}` : ""}.
        {#if !last.undo}<span class="muted">Discard it with the other unsaved changes.</span>{/if}
      {:else}
        {store.message}
      {/if}
    </span>
    {#if last?.undo}
      <Button size="sm" variant="secondary" icon="undo" data-testid="suggest-undo" onclick={() => void store.run()}>Undo</Button>
    {/if}
    <IconButton icon="close" label="Dismiss" size="sm" onclick={() => store.dismiss()} />
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    left: 50%;
    bottom: var(--rt-space-6);
    z-index: 60;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    max-width: min(36rem, calc(100vw - 2 * var(--rt-space-6)));
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-border-strong);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-3);
    box-shadow: var(--rt-elev-3);
    color: var(--rt-fg);
    font: var(--rt-font-body);
  }
  .ok {
    color: var(--rt-ok-fg);
    display: inline-flex;
  }
  .text {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
</style>
