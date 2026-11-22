<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The cookie jar of the open collection per environment: list, delete, clear. Owner: wave 5, strand D.
     Cookies come from `Set-Cookie` of responses and are sent with the next matching request. They are
     session state, often credentials: kept by the shell, never in collection files or the history. -->
<script lang="ts">
  import type { CookieInfo } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { cookieFlags, cookieKey, formatExpires, sortCookies } from "./format";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();
  const app = getApp();

  let cookies = $state<CookieInfo[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let confirmClear = $state(false);

  const scope = $derived(app.environment ?? "");

  async function act(op: () => Promise<void>) {
    try {
      error = null;
      await op();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function refresh() {
    const env = scope;
    loading = true;
    return act(async () => {
      cookies = sortCookies(await app.backend.cookies.list(env));
    }).finally(() => (loading = false));
  }

  function remove(c: CookieInfo) {
    return act(async () => {
      await app.backend.cookies.delete(scope, c.domain, c.path, c.name);
      await refresh();
    });
  }

  function clearAll() {
    confirmClear = false;
    return act(async () => {
      await app.backend.cookies.clear(scope);
      await refresh();
    });
  }

  // Load when opened and whenever the environment changes while open.
  $effect(() => {
    if (!open) return;
    void scope;
    confirmClear = false;
    void refresh();
  });

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
      <span class="scope" data-testid="cookies-environment">{app.environment ?? "No environment"}</span>
      <button data-testid="cookies-close" onclick={onclose}>Close</button>
    </header>
    <div class="body">
      {#if error}
        <p class="error" role="alert" data-testid="cookies-error">{error}</p>
      {/if}
      {#if cookies.length === 0}
        <p class="hint" data-testid="cookies-empty">
          {loading ? "Loading…" : "No cookies stored for this environment."}
        </p>
      {:else}
        <table data-testid="cookies-list">
          <thead>
            <tr>
              <th>Name</th>
              <th>Value</th>
              <th>Domain</th>
              <th>Path</th>
              <th>Expires</th>
              <th>Flags</th>
              <th><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each cookies as c (cookieKey(c))}
              <tr data-testid="cookie-row">
                <td data-testid="cookie-name">{c.name}</td>
                <td class="value" data-testid="cookie-value" title={c.value}>{c.value}</td>
                <td data-testid="cookie-domain">{c.domain}</td>
                <td>{c.path}</td>
                <td>{formatExpires(c.expires)}</td>
                <td class="flags">{cookieFlags(c).join(" ")}</td>
                <td>
                  <button
                    data-testid="cookie-delete"
                    aria-label={`Delete cookie ${c.name}`}
                    onclick={() => remove(c)}>Delete</button
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <footer>
        <p class="hint">
          Cookies from responses are stored per environment and sent with the next matching request
          {app.backend.kind === "web" ? "(in the browser only through the companion)" : ""}. They never go into
          collection files or the history.
        </p>
        {#if confirmClear}
          <span class="confirm">
            Delete all cookies of this environment?
            <button data-testid="cookies-clear-confirm" class="danger" onclick={clearAll}>Delete all</button>
            <button data-testid="cookies-clear-cancel" onclick={() => (confirmClear = false)}>Cancel</button>
          </span>
        {:else}
          <button data-testid="cookies-clear" disabled={cookies.length === 0} onclick={() => (confirmClear = true)}>
            Clear all
          </button>
        {/if}
      </footer>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(860px, calc(100vw - 32px));
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
    vertical-align: top;
  }
  th {
    color: var(--rt-fg-muted);
    font-weight: normal;
  }
  .value {
    max-width: 16em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--rt-font-mono);
  }
  .flags {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
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
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button:disabled {
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
    clip: rect(0 0 0 0);
  }
</style>
