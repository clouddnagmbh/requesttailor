<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- What coding agents (`rt mcp`) may do in the open collection, per environment (wave 8,
     strand C). Stored in `extensions.requesttailor.agents` of opencollection.yml: every change
     goes to the backend at once (in memory) and is written by the global Save, like any edit.
     A running `rt mcp` keeps the policy it started with and only follows changes that
     restrict it. -->
<script lang="ts">
  import type { AgentAccess, AgentPolicy } from "../commands/types";
  import { isProductionName } from "../environments/ops";
  import { getApp } from "../state/app.svelte";
  import { ACCESS_LABELS, effective, policyOf, rowsOf, type PolicyRow } from "./policy";

  let { open, onclose, onaudit }: { open: boolean; onclose: () => void; onaudit?: () => void } = $props();
  const app = getApp();

  let access = $state<AgentAccess>("read");
  let rows = $state<PolicyRow[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  const ACCESS: AgentAccess[] = ["none", "read", "write"];
  const names = $derived(app.environments.map((e) => e.name));

  function show(policy: AgentPolicy) {
    access = policy.access;
    rows = rowsOf(policy, names);
  }

  $effect(() => {
    if (!open) return;
    void names;
    loaded = false;
    error = null;
    let stale = false;
    app.backend.agents
      .policy()
      .then((p) => {
        if (stale) return;
        show(p);
        loaded = true;
      })
      .catch((e) => {
        if (!stale) error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      stale = true;
    };
  });

  // Changes go out one after another, each with the then-current rows.
  let queue: Promise<void> = Promise.resolve();
  function commit() {
    const policy = policyOf(access, $state.snapshot(rows) as PolicyRow[]);
    queue = queue.then(async () => {
      try {
        error = null;
        await app.backend.agents.setPolicy(policy);
        await app.reloadEnvironments();
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    });
  }

  function remove(row: PolicyRow) {
    rows = rows.filter((r) => r !== row);
    commit();
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }
</script>

{#if open}
  <dialog
    data-testid="agent-policy"
    aria-labelledby="agent-policy-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <header>
      <h2 id="agent-policy-title">Agent access</h2>
      <span class="scope">{app.collection?.name ?? ""}</span>
      {#if onaudit}<button data-testid="agent-policy-audit" onclick={onaudit}>Audit log</button>{/if}
      <button data-testid="agent-policy-close" onclick={onclose}>Close</button>
    </header>
    <div class="body">
      <p class="hint">
        Coding agents connect through <code>rt mcp</code>. They never see secret values. Writing requests against
        a production environment wait for your approval here or with <code>rt approve</code>.
      </p>
      {#if error}<p class="error" role="alert" data-testid="agent-policy-error">{error}</p>{/if}
      {#if loaded}
        <label class="default">
          Default for this collection
          <select data-testid="agent-policy-default" bind:value={access} onchange={commit}>
            {#each ACCESS as a (a)}<option value={a}>{ACCESS_LABELS[a]}</option>{/each}
          </select>
        </label>
        {#if rows.length === 0}
          <p class="hint" data-testid="agent-policy-empty">This collection has no environments.</p>
        {:else}
          <table data-testid="agent-policy-rows">
            <thead>
              <tr>
                <th>Environment</th>
                <th>Access</th>
                <th>Production</th>
                <th>Agents may</th>
                <th><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each rows as row, i (row.environment)}
                <tr data-testid="agent-policy-row" data-environment={row.environment}>
                  <td>
                    {row.environment}
                    {#if row.missing}<span class="muted">(not in this collection)</span>{/if}
                  </td>
                  <td>
                    <select
                      data-testid="agent-policy-access"
                      aria-label={`Agent access to ${row.environment}`}
                      bind:value={rows[i].access}
                      onchange={commit}
                    >
                      <option value="default">Default ({ACCESS_LABELS[access]})</option>
                      {#each ACCESS as a (a)}<option value={a}>{ACCESS_LABELS[a]}</option>{/each}
                    </select>
                  </td>
                  <td>
                    <label class="flag">
                      <input
                        type="checkbox"
                        data-testid="agent-policy-production"
                        aria-label={`${row.environment} is a production environment`}
                        bind:checked={rows[i].production}
                        onchange={commit}
                      />
                      {#if !row.production && isProductionName(row.environment)}
                        <span class="warn" data-testid="agent-policy-looks-production">looks like production</span>
                      {/if}
                    </label>
                  </td>
                  <td class="muted" data-testid="agent-policy-effective">{effective(access, row)}</td>
                  <td>
                    {#if row.missing}
                      <button data-testid="agent-policy-remove" aria-label={`Remove ${row.environment}`} onclick={() => remove(row)}
                        >Remove</button
                      >
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
        <p class="hint">Changes are saved with the collection (Save).</p>
      {:else if !error}
        <p class="hint">Loading…</p>
      {/if}
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(760px, calc(100vw - 32px));
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
    margin: 0;
    font-size: 1em;
  }
  .scope {
    flex: 1;
    color: var(--rt-fg-muted);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    overflow: auto;
  }
  .default {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
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
    vertical-align: middle;
  }
  th {
    color: var(--rt-fg-muted);
    font-weight: normal;
  }
  .flag {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .hint,
  .muted {
    color: var(--rt-fg-muted);
  }
  .hint {
    margin: 0;
  }
  .warn {
    color: var(--rt-warn);
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
  }
  code {
    font-family: var(--rt-font-mono);
  }
  select,
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  button {
    cursor: pointer;
  }
  button:hover {
    background: var(--rt-bg-hover);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
