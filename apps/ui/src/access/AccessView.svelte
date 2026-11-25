<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The two-user access check (wave 19, strand D): one request is sent as user A (an environment)
     and, if the answer holds an object, as user B with the ids A saw. Reads go without a
     question; a writing request needs "Allow writes for this check" and is never sent against
     production. What is shown is evidence only: status, header names, JSON paths with types and
     ids that read as ids, never a body, a secret or a personal value. Use it only with accounts
     you own. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { AccessCheck, AgentPolicy, Effect, RequestSpec } from "../commands/types";
  import { isProduction } from "../environments/production";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, Checkbox, Dialog, DialogFooter, EmptyState, Select, Spinner } from "../ui";
  import { WRITE_QUESTION, headerLine, headline, isBroken, otherEnvironments, skippedHint, tone, valueRows } from "./model";
  import { presetHolder } from "./open.svelte";

  let { app }: ToolProps = $props();

  let path = $state<string | null>(null);
  let spec = $state<RequestSpec | null>(null);
  let effect = $state<Effect | null>(null);
  let policy = $state<AgentPolicy | null>(null);
  let a = $state("");
  let b = $state("");
  let mustBeDenied = $state(false);
  let allowWrites = $state(false);

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<AccessCheck | null>(null);
  let asking = $state(false);
  let seq = 0;

  const names = $derived(app.environments.map((e) => e.name));
  const writes = $derived(effect === "write");
  const productionA = $derived(isProduction(policy, a));
  const productionB = $derived(isProduction(policy, b));
  const blocked = $derived(writes && (productionA || productionB));
  const ready = $derived(a !== "" && b !== "" && a !== b);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  onMount(() => {
    path = presetHolder.take()?.path ?? app.active;
    const first = app.environment ?? names[0] ?? "";
    a = first;
    b = otherEnvironments(names, first)[0] ?? "";
    void prepare();
  });

  async function prepare() {
    const p = path;
    if (!p) return;
    try {
      spec = await app.backend.getRequest(p);
    } catch (e) {
      error = message(e);
      return;
    }
    policy = await app.backend.agents.policy().catch(() => null);
    effect = await app.backend.agents
      .check(p, a)
      .then((c) => c.effect)
      .catch(() => null);
  }

  function start() {
    if (!path || running || !ready || blocked) return;
    error = null;
    if (writes && !allowWrites) {
      asking = true;
      return;
    }
    void run();
  }

  async function run() {
    asking = false;
    const p = path;
    if (!p) return;
    const mine = ++seq;
    running = true;
    error = null;
    result = null;
    try {
      // The unsaved edits of the request go with the check: it checks what is on screen.
      await app.settled();
      const r = await app.backend.access.check(
        p,
        { a, b },
        { allow_writes: writes && allowWrites, b_must_be_denied: mustBeDenied },
      );
      if (mine === seq) result = r;
    } catch (e) {
      if (mine === seq) error = message(e);
    } finally {
      if (mine === seq) running = false;
    }
  }
</script>

<section class="access" data-testid="access-panel" aria-label="Check access">
  {#if !path}
    <EmptyState icon="shield-secret" title="Pick a request" text="Open a request and choose “Check access…” in the command palette." />
  {:else}
    <header class="target">
      <h2>Check access</h2>
      <p class="what" data-testid="access-target">
        <Badge kind="method" value={spec?.method ?? ""} />
        <span class="mono">{spec?.url ?? path}</span>
      </p>
      <p class="note">
        Sends this request as user A, a profile of your own, and if the answer holds an object, the same access as user B with the id A saw. Use it only with accounts you own. Nothing is saved.
      </p>
    </header>

    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        start();
      }}
    >
      <div class="field">
        <Select label="User A (environment)" bind:value={a} options={names.map((n) => ({ value: n, label: n }))} disabled={running} data-testid="access-a" />
      </div>
      <div class="field">
        <Select label="User B (environment)" bind:value={b} options={names.filter((n) => n !== a).map((n) => ({ value: n, label: n }))} disabled={running} data-testid="access-b" />
      </div>
      <div class="go">
        <Button type="submit" variant="primary" icon="shield-secret" loading={running} disabled={!ready || blocked} data-testid="access-start">
          {running ? "Checking…" : "Check"}
        </Button>
      </div>
      <div class="checks">
        <Checkbox label="User B must be denied on this operation" bind:checked={mustBeDenied} disabled={running} data-testid="access-denied" />
        {#if writes}
          <Checkbox label="Allow writes for this check" bind:checked={allowWrites} disabled={running} data-testid="access-writes" />
        {/if}
      </div>
    </form>

    {#if names.length < 2}
      <div data-testid="access-need-two">
        <Banner tone="info" title="Two profiles needed">A profile is an environment. Add a second environment with its own token, cookie or secret.</Banner>
      </div>
    {/if}
    {#if blocked}
      <div data-testid="access-blocked">
        <Banner tone="warn" title="Not against production">This request writes, and a writing request is never checked against a production environment ({productionA ? a : b}).</Banner>
      </div>
    {:else if writes}
      <p class="note" data-testid="access-writes-note">This request writes. It runs only after “Allow writes for this check”, in environments that are not production.</p>
    {/if}

    {#if error}
      <div data-testid="access-error"><Banner tone="danger" title="Could not check">{error}</Banner></div>
    {/if}

    {#if running}
      <div class="loading" data-testid="access-running"><Spinner label="Sending" /> Sending as {a}, then as {b}.</div>
    {/if}

    {#if result}
      {@const t = tone(result.verdict)}
      <div class="result" data-testid="access-result" data-verdict={result.verdict.kind} data-broken={isBroken(result.verdict)}>
        <Banner tone={t === "danger" ? "danger" : t === "ok" ? "ok" : t === "warn" ? "warn" : "info"} title={headline(result, a, b)}>
          {#each result.findings as f (f.message)}<p class="line" data-testid="access-finding">{f.message}</p>{/each}
          {#if result.verdict.kind === "skipped" && skippedHint(result.verdict.reason)}
            <p class="line" data-testid="access-hint">{skippedHint(result.verdict.reason)}</p>
          {/if}
        </Banner>

        {#each result.findings.filter((f) => f.evidence.length > 0).slice(0, 1) as f (f.message)}
          <div class="cols" data-testid="access-evidence">
            {#each f.evidence as e (e.profile)}
              {@const rows = valueRows(e)}
              <div class="col" data-profile={e.profile}>
                <h3>User {e.profile} <span class="env">({e.profile === "A" ? a : b})</span></h3>
                <p class="line"><span class="label">Status</span> <span class="mono" data-testid="access-status-{e.profile}">{e.status ?? "no answer"}</span></p>
                {#if e.header_names.length > 0}
                  <p class="line"><span class="label">Headers</span> <span class="mono">{headerLine(e)}</span></p>
                {/if}
                {#if rows.rows.length > 0}
                  <table class="values">
                    <tbody>
                      {#each rows.rows as [p, v] (p)}
                        <tr><td class="mono">{p}</td><td class="mono">{v}</td></tr>
                      {/each}
                    </tbody>
                  </table>
                  {#if rows.more > 0}<p class="hint">and {rows.more} more</p>{/if}
                {/if}
              </div>
            {/each}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</section>

{#if asking}
  <Dialog title={WRITE_QUESTION} description="Send {spec?.method} {spec?.url} as {a} and as {b}?" icon="warning" size="sm" testid="access-confirm-writes" closeOnBackdrop={false} onclose={() => (asking = false)}>
    <p class="ask">The request changes data on the server, as {a} and then as {b}. Use a test system. It is never sent to production.</p>
    <DialogFooter>
      <Button variant="secondary" data-testid="access-confirm-writes-cancel" onclick={() => (asking = false)}>Cancel</Button>
      <Button
        variant="danger"
        data-testid="access-confirm-writes-yes"
        data-autofocus
        onclick={() => {
          allowWrites = true;
          void run();
        }}>Allow and check</Button
      >
    </DialogFooter>
  </Dialog>
{/if}

<style>
  .access {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
    overflow: auto;
    box-sizing: border-box;
    padding: var(--rt-space-6);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-title);
  }
  h3 {
    margin: 0 0 var(--rt-space-3);
    font: var(--rt-font-heading);
  }
  .what {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
    margin: var(--rt-space-3) 0;
    min-width: 0;
  }
  .what .mono {
    overflow-wrap: anywhere;
  }
  .env {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .note,
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .form {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-5) var(--rt-space-6);
  }
  .field {
    flex: 0 1 260px;
    min-width: 160px;
  }
  .go {
    display: flex;
    align-items: center;
    margin-left: auto;
    min-height: var(--rt-control-md);
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    flex: 1 0 100%;
  }
  .loading {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .line {
    margin: var(--rt-space-2) 0 0;
    overflow-wrap: anywhere;
  }
  .label {
    color: var(--rt-fg-muted);
    margin-right: var(--rt-space-3);
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: var(--rt-space-6);
  }
  .col {
    min-width: 0;
  }
  .values {
    margin-top: var(--rt-space-3);
    border-collapse: collapse;
    width: 100%;
  }
  .values td {
    padding: var(--rt-space-1) var(--rt-space-3) var(--rt-space-1) 0;
    overflow-wrap: anywhere;
    vertical-align: top;
  }
  .ask {
    margin: 0;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
</style>
