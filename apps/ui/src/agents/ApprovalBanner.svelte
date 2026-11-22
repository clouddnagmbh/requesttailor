<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Writing requests of coding agents (`rt mcp`) against production environments wait here
     for the user's answer (wave 8, strand C). Shown on every screen while something waits,
     for every collection on this machine. A banner, not a dialog: it takes no focus, so the
     user's own work goes on; both buttons are ordinary buttons (Tab, Enter, Space). The URL
     is the masked one `rt mcp` wrote; secret values never reach the app this way.
     The first answer wins: `rt approve`/`rt deny` and the agent's client may answer too. -->
<script lang="ts">
  import type { ApprovalDecision, PendingApproval } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { approvalTitle, since } from "./policy";

  const app = getApp();

  let pending = $state<PendingApproval[]>([]);
  let busy = $state<Record<string, true>>({});
  let problem = $state<string | null>(null);
  let now = $state(Date.now());

  $effect(() => {
    const stop = app.backend.agents.onPending((p) => (pending = p));
    void app.backend.agents
      .pending()
      .then((p) => (pending = p))
      .catch(() => undefined);
    return stop;
  });

  // Ages ("12s ago") stay current while something waits.
  $effect(() => {
    if (pending.length === 0) return;
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  async function decide(p: PendingApproval, decision: ApprovalDecision): Promise<void> {
    busy[p.id] = true;
    problem = null;
    try {
      await app.backend.agents.decide(p.id, decision);
      pending = pending.filter((q) => q.id !== p.id);
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
      pending = await app.backend.agents.pending().catch(() => pending);
    } finally {
      delete busy[p.id];
    }
  }
</script>

<!-- The live region always exists, so screen readers announce approvals as they arrive. -->
<div aria-live="assertive" aria-relevant="additions">
{#if pending.length > 0}
  <section class="approvals" data-testid="agent-approvals" aria-label="Agent requests waiting for approval">
    {#each pending as p (p.id)}
      <article data-testid="agent-approval" aria-labelledby="approval-{p.id}">
        <div class="what">
          <p class="title" id="approval-{p.id}">
            <strong data-testid="agent-approval-agent">{p.agent}</strong> wants to send
            <span class="method" data-testid="agent-approval-method">{p.method}</span>
            <code data-testid="agent-approval-url">{p.url}</code>
            {#if p.environment}in <strong data-testid="agent-approval-environment">{p.environment}</strong>{/if}
          </p>
          <p class="detail">
            {p.collection_name} · {p.request_path} · {p.reason} · {since(p.at, now)}
          </p>
        </div>
        <div class="actions">
          <button
            class="approve"
            data-testid="agent-approve"
            aria-label={`Approve ${approvalTitle(p)}`}
            disabled={busy[p.id]}
            onclick={() => decide(p, "approved")}>Approve</button
          >
          <button
            data-testid="agent-deny"
            aria-label={`Deny ${approvalTitle(p)}`}
            disabled={busy[p.id]}
            onclick={() => decide(p, "denied")}>Deny</button
          >
        </div>
      </article>
    {/each}
    {#if problem}<p class="problem" role="alert" data-testid="agent-approval-error">{problem}</p>{/if}
  </section>
{/if}
</div>

<style>
  .approvals {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    margin: var(--rt-gap);
  }
  article {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
    align-items: center;
    justify-content: space-between;
    padding: var(--rt-gap) calc(var(--rt-gap) * 1.5);
    border: 1px solid var(--rt-danger);
    border-left-width: 4px;
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    color: var(--rt-fg);
  }
  .what {
    min-width: 0;
    flex: 1;
  }
  p {
    margin: 0;
  }
  .title code {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .method {
    font-weight: bold;
  }
  .detail {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .actions {
    display: flex;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    padding: 4px 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: 1px;
  }
  button.approve {
    border-color: var(--rt-danger);
    color: var(--rt-danger);
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .problem {
    color: var(--rt-danger);
  }
</style>
