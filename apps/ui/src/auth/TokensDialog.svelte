<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The cached OAuth 2.0 tokens of the open collection (`backend.auth.tokens()`): provider,
     client and scope, when fetched, how long valid, refreshable. Never their values. Delete
     one or all. Owner: wave 5, strand C. -->
<script lang="ts">
  import type { TokenInfo } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { tokenState, when } from "./format";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  const app = getApp();
  let tokens = $state<TokenInfo[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      tokens = await app.backend.auth.tokens();
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function clear(key: string | null) {
    try {
      await app.backend.auth.clearTokens(key);
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    await load();
  }

  $effect(() => {
    if (open) void load();
  });

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if open}
  <dialog
    data-testid="tokens-dialog"
    aria-labelledby="tokens-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <header>
      <h2 id="tokens-title">OAuth 2.0 tokens</h2>
      <button data-testid="tokens-clear-all" disabled={tokens.length === 0} onclick={() => clear(null)}>Delete all</button>
      <button data-testid="tokens-close" onclick={onclose}>Close</button>
    </header>
    <div class="body">
      {#if error}
        <p class="error" role="alert" data-testid="tokens-error">{error}</p>
      {/if}
      {#if tokens.length === 0}
        <p class="hint" data-testid="tokens-empty">{loading ? "Loading…" : "No cached tokens for this collection."}</p>
      {:else}
        <table>
          <thead>
            <tr><th>Token</th><th>Fetched</th><th>Status</th><th><span class="sr">Actions</span></th></tr>
          </thead>
          <tbody>
            {#each tokens as token (token.key)}
              {@const status = tokenState(token, Date.now())}
              <tr data-testid="tokens-row">
                <td title={token.key}>{token.label}</td>
                <td>{when(token.obtained_at)}</td>
                <td class={status.kind}
                  >{status.text}{#if token.refreshable && status.kind === "valid"}<span class="hint"> · refreshable</span>{/if}</td
                >
                <td><button data-testid="tokens-delete" aria-label={`Delete token ${token.label}`} onclick={() => clear(token.key)}>Delete</button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <p class="hint">Tokens are stored like secrets (OS keychain, or encrypted in the browser), never in collection files.</p>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(720px, calc(100vw - 32px));
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
  .body {
    padding: calc(var(--rt-gap) * 2);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    text-align: left;
    padding: 3px 6px;
    border-bottom: 1px solid var(--rt-border);
  }
  th {
    color: var(--rt-fg-muted);
    font-weight: normal;
  }
  td.valid {
    color: var(--rt-ok);
  }
  td.expired {
    color: var(--rt-warn);
  }
  .hint {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 8px;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
</style>
