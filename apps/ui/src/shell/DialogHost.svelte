<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The dialog of an extension (`AppState.showDialog`, wave 14, strand C): title bar, close
     button, Escape, focus kept inside, light and dark. Interface stub: a plain modal. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { getExtensions } from "./extension-tabs";

  const app = getApp();
  const extensions = getExtensions();
  const open = $derived(app.dialog);

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && app.dialog) {
      e.preventDefault();
      app.closeDialog();
    }
  }
</script>

<svelte:window {onkeydown} />

{#if open && extensions}
  {#key open.seq}
    <div class="backdrop">
      <div class="dialog" role="dialog" aria-modal="true" aria-label={open.dialog.title} data-testid="extension-dialog">
        <header>
          <h2>{open.dialog.title}</h2>
          <button class="close" aria-label="Close" onclick={() => app.closeDialog()}>×</button>
        </header>
        <open.dialog.component ctx={extensions.ctx} close={(r?: unknown) => app.closeDialog(r)} props={open.dialog.props ?? {}} />
      </div>
    </div>
  {/key}
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--rt-bg-overlay);
    z-index: 50;
  }
  .dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow: auto;
    background: var(--rt-bg);
    color: var(--rt-fg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    box-shadow: var(--rt-shadow);
    padding: 12px 16px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h2 {
    margin: 0;
    font-size: var(--rt-size);
  }
  .close {
    background: none;
    border: 0;
    font-size: 1.2rem;
    cursor: pointer;
    color: inherit;
  }
</style>
