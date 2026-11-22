<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Session variables of the active environment (wave 7, strand C), next to the environment
     switcher: a button with the count (only while there are some) that opens a list with
     remove and clear all. They come from scripts, actions and "set as variable", go into the
     runtime layer of every send and are never written to a file. -->
<script lang="ts">
  import { getApp } from "../state/app.svelte";
  import { sessionVars, track } from "./session.svelte";

  const app = getApp();
  const session = sessionVars(app);
  let open = $state(false);
  let error = $state<string | null>(null);

  $effect(() => track(app, session));

  async function act(op: () => Promise<void>) {
    try {
      error = null;
      await op();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if session.list.length > 0}
  <button
    type="button"
    class="button"
    data-testid="session-vars-button"
    title="Variables set in this session (scripts, actions, set as variable)"
    onclick={() => (open = true)}>Session <span class="badge" data-testid="session-vars-count">{session.list.length}</span></button
  >
{/if}

{#if open}
  <dialog
    data-testid="session-vars-dialog"
    aria-labelledby="rt-session-vars-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      open = false;
    }}
  >
    <header>
      <h2 id="rt-session-vars-title">Session variables</h2>
      <span class="scope">{app.environment ?? "No environment"}</span>
      <button type="button" data-testid="session-vars-close" onclick={() => (open = false)}>Close</button>
    </header>
    <div class="body">
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if session.list.length === 0}
        <p class="hint" data-testid="session-vars-empty">No session variables for this environment.</p>
      {:else}
        <table data-testid="session-vars-list">
          <thead>
            <tr><th>Name</th><th>Value</th><th>Set as</th><th><span class="sr-only">Remove</span></th></tr>
          </thead>
          <tbody>
            {#each session.list as v (v.name)}
              <tr data-testid="session-var-row">
                <td class="mono" data-testid="session-var-name">{v.name}</td>
                <td class="mono value" data-testid="session-var-value" title={v.value ?? ""}>{v.value}</td>
                <td class="scope">{v.scope}</td>
                <td>
                  <button
                    type="button"
                    data-testid="session-var-remove"
                    aria-label="Remove session variable {v.name}"
                    onclick={() => act(() => session.remove(v.name))}>Remove</button
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <footer>
        <p class="hint">
          Used by every send in this environment as runtime variables until cleared or the app is closed. Never written to a file.
        </p>
        <button type="button" class="danger" data-testid="session-vars-clear" disabled={session.list.length === 0} onclick={() => act(() => session.clear())}>
          Clear all
        </button>
      </footer>
    </div>
  </dialog>
{/if}

<style>
  .button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
    cursor: pointer;
  }
  .button:hover {
    background: var(--rt-bg-hover);
  }
  .badge {
    margin-left: 4px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--rt-bg-selected);
    font-size: var(--rt-size-small);
  }
  dialog {
    width: min(720px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
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
  .scope {
    color: var(--rt-fg-muted);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    overflow: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    text-align: left;
    padding: 2px 6px;
    border-bottom: 1px solid var(--rt-border);
  }
  th {
    color: var(--rt-fg-muted);
    font-weight: normal;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .value {
    max-width: 24em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  footer .hint {
    flex: 1;
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  dialog button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
    cursor: pointer;
  }
  dialog button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  dialog button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .danger {
    color: var(--rt-danger);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
