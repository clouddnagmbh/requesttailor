<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The minimal failing request (wave 19, strand E): pick what counts as failing (the failure of
     the last response by default), optionally what must stay, and let the core send variations
     of the request until the smallest one that still fails is found. Reads go without a
     question (production asks once); a writing request asks "This sends up to N requests that
     write." and is never shrunk against production. The result is a suggestion: it opens as an
     unsaved draft, copies as curl or as a link (the rules of sharing: no secret value). -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { AgentPolicy, Effect, RequestSpec, ShrinkPredicate, ShrinkResult } from "../commands/types";
  import { confirmEnabled, isProduction } from "../environments/production";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, Checkbox, Dialog, DialogFooter, EmptyState, Select, Spinner, TextField } from "../ui";
  import {
    MAX_PROBES_DEFAULT,
    MAX_PROBES_HARD,
    PREDICATE_CHOICES,
    type PredicateChoice,
    buildPredicate,
    choiceOf,
    describePredicate,
    failed,
    headline,
    needsValue,
    notes,
    outcomeOf,
    parseKeep,
    partName,
    removedGroups,
    valueLabel,
    writeQuestion,
  } from "./model";
  import { presetHolder } from "./open.svelte";

  let { app, close }: ToolProps = $props();

  let path = $state<string | null>(null);
  let spec = $state<RequestSpec | null>(null);
  let effect = $state<Effect | null>(null);
  let policy = $state<AgentPolicy | null>(null);
  let choice = $state<PredicateChoice>("server-error");
  let value = $state("");
  let keepText = $state("");
  let simplify = $state(false);
  let maxProbes = $state(String(MAX_PROBES_DEFAULT));
  /** What the last response failed with, when it did (the default choice). */
  let lastFailure = $state<ShrinkPredicate | null>(null);

  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<ShrinkResult | null>(null);
  let asking = $state<"writes" | "production" | null>(null);
  let done = $state<string | null>(null);
  let draftPath = $state<string | null>(null);
  let seq = 0;

  const environment = $derived(app.environment);
  const production = $derived(isProduction(policy, environment));
  const writes = $derived(effect === "write");
  const probes = $derived(limit());
  const blocked = $derived(writes && production);
  const groups = $derived(result ? removedGroups(result) : []);

  function limit(): number {
    const n = Math.floor(Number(maxProbes));
    if (!Number.isFinite(n) || n < 1) return MAX_PROBES_DEFAULT;
    return Math.min(n, MAX_PROBES_HARD);
  }

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  onMount(() => {
    path = presetHolder.take()?.path ?? app.active;
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
      .check(p, app.environment ?? "")
      .then((c) => c.effect)
      .catch(() => null);
    const last = app.results[p];
    if (failed(last) && last?.response_id != null) {
      try {
        lastFailure = await app.backend.shrink.predicate(p, last.response_id);
        choice = "last";
      } catch {
        lastFailure = null;
      }
    }
  }

  const choices = $derived(
    PREDICATE_CHOICES.map((c) => ({ ...c, disabled: c.value === "last" && lastFailure === null })).filter(
      (c) => c.value !== "last" || lastFailure !== null,
    ),
  );

  function start() {
    if (!path || running) return;
    const built = buildPredicate(choice, value);
    if (typeof built === "string") {
      error = built;
      return;
    }
    error = null;
    if (blocked) return;
    if (writes) asking = "writes";
    else if (production && confirmEnabled()) asking = "production";
    else void run(false);
  }

  async function run(confirmedWrites: boolean) {
    asking = null;
    const p = path;
    if (!p) return;
    const predicate = buildPredicate(choice, value);
    if (typeof predicate === "string") return;
    const mine = ++seq;
    running = true;
    error = null;
    result = null;
    done = null;
    draftPath = null;
    try {
      // The unsaved edits of the request go with the search: it shrinks what is on screen.
      await app.settled();
      const r = await app.backend.shrink.run({
        path: p,
        environment: environment ?? undefined,
        predicate,
        keep: parseKeep(keepText),
        max_probes: probes,
        simplify_values: simplify,
        confirmed_writes: confirmedWrites,
      });
      if (mine === seq) result = r;
    } catch (e) {
      if (mine === seq) error = message(e);
    } finally {
      if (mine === seq) running = false;
    }
  }

  async function cancel() {
    await app.backend.shrink.cancel().catch(() => {});
  }

  /** The smallest request as an unsaved draft next to the original (once). */
  async function ensureDraft(): Promise<string | null> {
    if (draftPath) return draftPath;
    const r = result;
    const p = path;
    if (!r || !p) return null;
    const slash = p.lastIndexOf("/");
    const folder = slash < 0 ? "" : p.slice(0, slash);
    await app.create(folder, `Minimal: ${r.request.name}`);
    const made = app.active;
    if (!made) return null;
    const created = await app.backend.getRequest(made);
    await app.applyExternal(made, { ...($state.snapshot(r.request) as RequestSpec), name: created.name, seq: created.seq });
    draftPath = made;
    return made;
  }

  async function openDraft() {
    done = null;
    try {
      if (await ensureDraft()) {
        done = "Opened as an unsaved request.";
        close();
      }
    } catch (e) {
      error = message(e);
    }
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      done = `${what} copied.`;
    } catch {
      error = "The browser did not allow copying.";
    }
  }

  /** Runs `f` on the request path with the smallest request in place of the original, in the
   *  backend only (no tab, no draft), and puts the original back. Sharing reads a request of the
   *  collection by path, so this is the way to hand it the shrunk one without leaving the view. */
  async function withShrunk<T>(f: (path: string) => Promise<T>): Promise<T | null> {
    const r = result;
    const p = path;
    if (!r || !p) return null;
    await app.settled();
    const original = await app.backend.getRequest(p);
    await app.backend.updateRequest(p, { ...($state.snapshot(r.request) as RequestSpec), name: original.name, seq: original.seq });
    try {
      return await f(p);
    } finally {
      await app.backend.updateRequest(p, original);
      await app.refreshDirty();
    }
  }

  async function copyCurl() {
    done = null;
    try {
      const c = await withShrunk((p) => app.backend.share.copyAs(p, "curl", { environment }));
      if (c) await copy(c.text, "curl command");
    } catch (e) {
      error = message(e);
    }
  }

  async function copyLink() {
    done = null;
    try {
      const l = await withShrunk((p) => app.backend.share.link(p, { environment }));
      if (l) await copy(l.text, "Link");
    } catch (e) {
      error = message(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && running && !asking) {
      e.preventDefault();
      void cancel();
    }
  }

  const headerNames = (r: RequestSpec) => r.headers.filter((h) => h.enabled).map((h) => h.name);
  const bodyText = (r: RequestSpec) => (r.body.type === "raw" ? r.body.data : null);
</script>

<svelte:window {onkeydown} />

<section class="shrink" data-testid="shrink-panel" aria-label="Minimal failing request">
  {#if !path}
    <EmptyState icon="filter" title="Pick a request" text="Open a request that fails and choose “Shrink failing request” in the command palette." />
  {:else}
    <header class="target">
      <h2>Minimal failing request</h2>
      <p class="what" data-testid="shrink-target">
        <Badge kind="method" value={spec?.method ?? ""} />
        <span class="mono">{spec?.url ?? path}</span>
        {#if environment}<span class="env">in {environment}</span>{/if}
        {#if production}<Badge tone="danger" value="PRODUCTION" />{/if}
      </p>
      <p class="note">
        Sends variations of this request, each with fewer headers, query parameters, cookies and body fields, and keeps the smallest one that still fails. Authorization headers stay. Nothing is saved.
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
        <Select label="What counts as failing" bind:value={choice} options={choices} disabled={running} data-testid="shrink-predicate" />
      </div>
      {#if needsValue(choice)}
        <div class="field">
          <TextField label={valueLabel(choice)} mono bind:value disabled={running} data-testid="shrink-value" />
        </div>
      {/if}
      {#if choice === "last" && lastFailure}
        <p class="hint" data-testid="shrink-last">Last response: {describePredicate(lastFailure)}.</p>
      {/if}
      <div class="field wide">
        <TextField
          label="Keep these parts"
          mono
          bind:value={keepText}
          placeholder="header:X-Tenant, query:key, body:$.id"
          hint="Separate with commas. Authorization headers always stay."
          disabled={running}
          data-testid="shrink-keep"
        />
      </div>
      <div class="field small">
        <TextField label="Most probes" type="number" min="1" max={MAX_PROBES_HARD} bind:value={maxProbes} disabled={running} data-testid="shrink-max" />
      </div>
      <div class="go">
        <Checkbox label="Also shorten values" bind:checked={simplify} disabled={running} data-testid="shrink-simplify" />
        {#if running}
          <Button variant="secondary" icon="close" data-testid="shrink-cancel" onclick={() => void cancel()}>Cancel</Button>
        {/if}
        <Button type="submit" variant="primary" icon="filter" loading={running} disabled={blocked} data-testid="shrink-start">
          {running ? "Sending…" : "Shrink"}
        </Button>
      </div>
    </form>

    {#if blocked}
      <div data-testid="shrink-blocked">
        <Banner tone="warn" title="Not against production">This request writes, and a writing request is never shrunk against {environment}. Pick another environment.</Banner>
      </div>
    {:else if writes}
      <p class="note" data-testid="shrink-writes">This request writes: {writeQuestion(probes)} You are asked before it starts.</p>
    {/if}

    {#if error}
      <div data-testid="shrink-error"><Banner tone="danger" title="Could not shrink">{error}</Banner></div>
    {/if}

    {#if running}
      <div class="loading" data-testid="shrink-running"><Spinner label="Sending" /> Sending variations, one at a time. Press Esc to stop.</div>
    {/if}

    {#if result}
      {@const outcome = outcomeOf(result)}
      <div class="result" data-testid="shrink-result" data-outcome={outcome} data-probes={result.probes} data-removed={result.removed.length}>
        <Banner tone={outcome === "found" ? "ok" : "warn"} title={headline(result)}>
          {#each notes(result) as n (n)}<p class="line" data-testid="shrink-note">{n}</p>{/each}
        </Banner>

        {#if outcome === "found" || outcome === "stopped"}
          <div class="cols">
            <div class="col" data-testid="shrink-removed">
              <h3>What was removed</h3>
              {#if groups.length === 0}
                <p class="hint">Nothing could go: every part is needed.</p>
              {/if}
              {#each groups as g (g.kind)}
                <h4>{g.title}</h4>
                <ul>
                  {#each g.steps as s (s.part)}<li class="mono" data-part={s.part}>{partName(s)}</li>{/each}
                </ul>
              {/each}
            </div>
            <div class="col" data-testid="shrink-smallest">
              <h3>The smallest request</h3>
              <p class="mono line"><strong>{result.request.method}</strong> {result.request.url}</p>
              {#if headerNames(result.request).length > 0}
                <p class="line"><span class="label">Headers</span> <span class="mono" data-testid="shrink-kept-headers">{headerNames(result.request).join(", ")}</span></p>
              {/if}
              {#if bodyText(result.request)}
                <pre class="body mono" data-testid="shrink-kept-body">{bodyText(result.request)}</pre>
              {/if}
            </div>
          </div>
          <div class="actions">
            <Button variant="primary" icon="add" data-testid="shrink-open-draft" onclick={() => void openDraft()}>Open as new request</Button>
            <Button icon="copy" data-testid="shrink-copy-curl" onclick={() => void copyCurl()}>Copy as curl</Button>
            <Button icon="link" data-testid="shrink-copy-link" onclick={() => void copyLink()}>Copy link</Button>
            {#if done}<span class="done" role="status" data-testid="shrink-done">{done}</span>{/if}
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</section>

{#if asking === "writes"}
  <Dialog title={writeQuestion(probes)} description="Send the variations of {spec?.method} {spec?.url}?" icon="warning" size="sm" testid="shrink-confirm-writes" closeOnBackdrop={false} onclose={() => (asking = null)}>
    <p class="ask">Each probe changes data on the server, up to {probes} times. Use a test system.</p>
    <DialogFooter>
      <Button variant="secondary" data-testid="shrink-confirm-writes-cancel" onclick={() => (asking = null)}>Cancel</Button>
      <Button variant="danger" data-testid="shrink-confirm-writes-yes" data-autofocus onclick={() => void run(true)}>Send up to {probes}</Button>
    </DialogFooter>
  </Dialog>
{:else if asking === "production"}
  <Dialog title="Send reads to PRODUCTION?" description="{environment} is a production environment." icon="warning" size="sm" testid="shrink-confirm-production" closeOnBackdrop={false} onclose={() => (asking = null)}>
    <p class="ask">Only a reading request is sent, up to {probes} times, one after the other.</p>
    <DialogFooter>
      <Button variant="secondary" data-testid="shrink-confirm-production-cancel" onclick={() => (asking = null)}>Cancel</Button>
      <Button variant="danger" data-testid="shrink-confirm-production-yes" data-autofocus onclick={() => void run(false)}>Send</Button>
    </DialogFooter>
  </Dialog>
{/if}

<style>
  .shrink {
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
  h4 {
    margin: var(--rt-space-4) 0 var(--rt-space-2);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    text-transform: uppercase;
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
  .field.wide {
    flex: 1 1 280px;
  }
  .field.small {
    flex: 0 0 110px;
    min-width: 90px;
  }
  .go {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-5);
    margin-left: auto;
    min-height: var(--rt-control-md);
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
  ul {
    margin: 0;
    padding-left: var(--rt-space-6);
  }
  li {
    overflow-wrap: anywhere;
  }
  .body {
    margin: var(--rt-space-3) 0 0;
    padding: var(--rt-space-4);
    max-height: 240px;
    overflow: auto;
    background: var(--rt-surface-sunken);
    border-radius: var(--rt-radius-md);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .done {
    color: var(--rt-ok-fg);
  }
  .ask {
    margin: 0;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
</style>
