<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The local audit log of coding agents (wave 8, strand C): one entry per `rt mcp` tool call,
     newest first, with the masked URL, the outcome and who approved. The log stays on this
     machine (`agents/audit.jsonl` in the data folder); the web app has none. -->
<script lang="ts">
  import type { AuditEntry } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { OUTCOME_LABELS } from "./policy";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();
  const app = getApp();

  const LIMIT = 200;
  let entries = $state<AuditEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  function refresh() {
    loading = true;
    error = null;
    return app.backend.agents
      .audit(LIMIT)
      .then((e) => (entries = e))
      .catch((e) => (error = e instanceof Error ? e.message : String(e)))
      .finally(() => (loading = false));
  }

  $effect(() => {
    if (open) void refresh();
  });

  const time = (at: number) => new Date(at).toLocaleString();

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if open}
  <dialog
    data-testid="agent-audit"
    aria-labelledby="agent-audit-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <header>
      <h2 id="agent-audit-title">Agent audit log</h2>
      <button data-testid="agent-audit-refresh" disabled={loading} onclick={refresh}>Refresh</button>
      <button data-testid="agent-audit-close" onclick={onclose}>Close</button>
    </header>
    <div class="body">
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if entries.length === 0}
        <p class="hint" data-testid="agent-audit-empty">
          {#if loading}
            Loading…
          {:else if app.backend.kind === "web"}
            Coding agents run through <code>rt mcp</code> on your computer; its audit log stays there
            (<code>rt approvals</code> shows what waits).
          {:else}
            No agent has used <code>rt mcp</code> on this computer yet.
          {/if}
        </p>
      {:else}
        <table data-testid="agent-audit-list">
          <thead>
            <tr>
              <th>Time</th>
              <th>Agent</th>
              <th>Tool</th>
              <th>Request</th>
              <th>Outcome</th>
            </tr>
          </thead>
          <tbody>
            {#each entries as e, i (i)}
              <tr data-testid="agent-audit-row">
                <td class="nowrap">{time(e.at)}</td>
                <td>{e.agent}</td>
                <td><code>{e.tool}</code></td>
                <td class="request">
                  {#if e.method || e.url}
                    <code>{e.method ?? ""} {e.url ?? ""}</code>
                  {/if}
                  <span class="muted">
                    {[e.request_path, e.environment].filter(Boolean).join(" · ")}
                    {#if e.detail}<br />{e.detail}{/if}
                  </span>
                </td>
                <td class="outcome {e.outcome}" data-testid="agent-audit-outcome">
                  {OUTCOME_LABELS[e.outcome]}{e.status != null ? ` ${e.status}` : ""}
                  {#if e.approved_by}<span class="muted">approved by {e.approved_by}</span>{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(960px, calc(100vw - 32px));
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
  .body {
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
  .nowrap {
    white-space: nowrap;
  }
  .request code {
    overflow-wrap: anywhere;
  }
  .request .muted,
  .outcome .muted {
    display: block;
    font-size: var(--rt-size-small);
  }
  .muted,
  .hint {
    color: var(--rt-fg-muted);
  }
  .hint,
  .error {
    margin: 0;
  }
  .error,
  .outcome.refused,
  .outcome.denied,
  .outcome.failed,
  .outcome.timed-out {
    color: var(--rt-danger);
  }
  code {
    font-family: var(--rt-font-mono);
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
    opacity: 0.6;
  }
</style>
