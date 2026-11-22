<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Confirms what a plugin wants to add to the collection (`create_requests`,
     `add_environments`, wave 6) before anything changes. The additions become unsaved
     changes; Escape or "Cancel" adds nothing. Owner: wave 6, strand F. -->
<script lang="ts">
  import type { AdditionPlan } from "./additions";

  let {
    plugin,
    plan,
    busy = false,
    error = null,
    onconfirm,
    oncancel,
  }: {
    /** The plugin's display name. */
    plugin: string;
    plan: AdditionPlan;
    busy?: boolean;
    error?: string | null;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

<dialog
  data-testid="additions-dialog"
  aria-labelledby="additions-title"
  {@attach opened}
  oncancel={(e) => {
    e.preventDefault();
    if (!busy) oncancel();
  }}
>
  <header>
    <h2 id="additions-title">Add to the collection?</h2>
  </header>
  <div class="body">
    <p>{plugin} wants to add the following. Nothing is saved until you save.</p>
    {#if plan.requests.length > 0}
      <section aria-label="Requests">
        <h3>Requests</h3>
        <ul data-testid="additions-requests">
          {#each plan.requests as r, i (i)}
            <li>
              <span class="method">{r.spec.method}</span>
              {r.folder ? `${r.folder} / ` : ""}{r.name}
              {#if r.renamed}<span class="muted">(name taken, renamed)</span>{/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if plan.environments.length > 0}
      <section aria-label="Environments">
        <h3>Environments</h3>
        <ul data-testid="additions-environments">
          {#each plan.environments as e (e.name)}
            <li>
              {e.name}
              <span class="muted">{e.created ? "new" : "adds to the existing one"}:</span>
              {e.variables.map((v) => (v.secret ? `${v.name} (secret, set it yourself)` : v.name)).join(", ") || "no variables"}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if error}<p class="error" role="alert" data-testid="additions-error">{error}</p>{/if}
    <div class="row">
      <button class="primary" data-testid="additions-confirm" disabled={busy} onclick={onconfirm}>Add</button>
      <button data-testid="additions-cancel" disabled={busy} onclick={oncancel}>Cancel</button>
    </div>
  </div>
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 32px));
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
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }
  h2,
  h3 {
    margin: 0;
    font-size: 1em;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) * 2);
    max-height: 70vh;
    overflow: auto;
  }
  p {
    margin: 0;
  }
  ul {
    margin: 4px 0 0;
    padding-left: 1.4em;
  }
  .method {
    font-family: var(--rt-font-mono, monospace);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
  }
  .row {
    display: flex;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
    cursor: pointer;
  }
  button.primary {
    color: var(--rt-accent-fg);
    background: var(--rt-accent);
    border-color: var(--rt-accent);
  }
</style>
