<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The cookie jar of the open collection per environment: list, delete, clear. Owner: wave 5, strand D. Interface commit: an empty dialog. -->
<script lang="ts">
  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if open}
  <dialog
    data-testid="cookies-dialog"
    aria-labelledby="cookies-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <header>
      <h2 id="cookies-title">Cookies</h2>
      <button data-testid="cookies-close" onclick={onclose}>Close</button>
    </header>
  </dialog>
{/if}

<style>
  dialog {
    width: min(640px, calc(100vw - 32px));
    padding: 0;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }
  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 1em;
  }
</style>
