<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The test-data journal (wave 19, strand C): the successful writing requests of this session
     and earlier ones, newest first, and a way to put one back by hand. "Revert…" asks which
     request of the collection does the undoing; its variables {{journal.id}},
     {{journal.location}} and {{journal.etag}} are filled from the entry, the plan shows the
     request with its masked URL, its effect and the environment, and Enter sends it (send guard,
     production question, audit log). Nothing is recorded until "Track test data" is on, for
     this session. The journal keeps no bodies and no header values. -->
<script lang="ts">
  import { onMount, tick } from "svelte";

  import type { JournalEntry, JournalSettings, RevertOutcome, RevertPlan } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, Dialog, DialogFooter, EmptyState, Select, Spinner, Switch, TextField } from "../ui";
  import {
    entryTarget,
    outcomeTone,
    productionQuestion,
    requestChoices,
    resultLine,
    timeLabel,
  } from "./model";

  let { app }: ToolProps = $props();

  let settings = $state<JournalSettings>({});
  let idPath = $state("");
  let entries = $state<JournalEntry[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let onlyOpen = $state(false);

  /** The entry the revert dialog is for. */
  let reverting = $state<JournalEntry | null>(null);
  let picked = $state("");
  let plan = $state<RevertPlan | null>(null);
  let planning = $state(false);
  let sending = $state(false);
  let askProduction = $state(false);
  let planError = $state<string | null>(null);
  let outcome = $state<RevertOutcome | null>(null);

  const choices = $derived(requestChoices(app.tree));
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  async function load() {
    try {
      const [s, page] = await Promise.all([
        app.backend.journal.settings(),
        app.backend.journal.entries({ limit: 500, only_open: onlyOpen }),
      ]);
      settings = s;
      idPath = s.id_path ?? "";
      entries = page.entries;
      total = page.total;
      error = null;
    } catch (e) {
      error = message(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => void load());

  async function save(next: JournalSettings) {
    try {
      await app.backend.journal.setSettings(next);
      settings = next;
      error = null;
    } catch (e) {
      error = message(e);
      idPath = settings.id_path ?? "";
    }
  }

  async function clear() {
    try {
      await app.backend.journal.clear();
      await load();
    } catch (e) {
      error = message(e);
    }
  }

  // Once the plan is shown, the Send button takes the focus: Enter sends it.
  $effect(() => {
    if (!plan) return;
    void tick().then(() => document.querySelector<HTMLElement>('[data-testid="journal-send"]')?.focus());
  });

  function openRevert(entry: JournalEntry) {
    reverting = entry;
    picked = choices[0]?.path ?? "";
    plan = null;
    planError = null;
    outcome = null;
    askProduction = false;
  }

  function closeRevert() {
    reverting = null;
    plan = null;
    askProduction = false;
  }

  async function makePlan() {
    if (!reverting || !picked) return;
    planning = true;
    planError = null;
    try {
      plan = await app.backend.journal.planManual({ entry_id: reverting.id, request_path: picked });
    } catch (e) {
      plan = null;
      planError = message(e);
    } finally {
      planning = false;
    }
  }

  function send() {
    if (!plan || sending) return;
    if (plan.production) askProduction = true;
    else void run(false);
  }

  async function run(confirmed: boolean) {
    if (!plan) return;
    askProduction = false;
    sending = true;
    planError = null;
    try {
      outcome = await app.backend.journal.execute($state.snapshot(plan), { production_confirmed: confirmed });
      closeRevert();
      await load();
    } catch (e) {
      planError = message(e);
    } finally {
      sending = false;
    }
  }

  function newRequest() {
    closeRevert();
    app.openNew({ kind: "http" });
  }
</script>

<section class="journal" data-testid="journal-panel" aria-label="Test-data journal">
  <header class="bar">
    <Switch
      label="Track test data"
      description="Remember the writing requests this session sends, so you can put them back."
      checked={settings.enabled ?? false}
      data-testid="journal-track"
      onchange={(e) => void save({ ...settings, enabled: e.currentTarget.checked })}
    />
    <form
      class="idpath"
      onsubmit={(e) => {
        e.preventDefault();
        void save({ ...settings, id_path: idPath.trim() || undefined });
      }}
    >
      <TextField
        label="Id in the answer"
        hint="A JSONPath such as $.id, or var:name"
        mono
        bind:value={idPath}
        placeholder="$.id"
        data-testid="journal-id-path"
        onblur={() => void save({ ...settings, id_path: idPath.trim() || undefined })}
      />
    </form>
    <div class="actions">
      <Button size="sm" variant="secondary" icon="trash" data-testid="journal-clear" disabled={total === 0} onclick={() => void clear()}>Clear</Button>
    </div>
  </header>
  <p class="note">
    Only successful writing requests are kept: time, environment, request, masked URL, status, the
    <code>Location</code> and the id. No bodies, no header values, no secrets. It lives on this device,
    never in a collection file. Tracking starts off in every session.
  </p>

  {#if error}
    <div data-testid="journal-error"><Banner tone="danger" title="The journal is not available">{error}</Banner></div>
  {/if}
  {#if outcome}
    <div data-testid="journal-outcome">
      <Banner tone={outcomeTone(outcome)} title="Revert finished" dismissible ondismiss={() => (outcome = null)}>
        <ul class="results">
          {#each outcome.steps as s (s.entry_id)}
            <li data-result={s.result}>#{s.entry_id}: {resultLine(s.result, s.message)}</li>
          {/each}
        </ul>
      </Banner>
    </div>
  {/if}

  {#if loading}
    <div class="loading"><Spinner label="Loading" /> Loading…</div>
  {:else if entries.length === 0}
    <div data-testid="journal-empty">
      <EmptyState
        icon="history"
        title={settings.enabled ? "Nothing written yet" : "The journal is off"}
        text={settings.enabled
          ? "Send a request that creates, changes or deletes something. It will show up here."
          : "Switch on Track test data, then send requests that write. They are listed here so you can put them back."}
      />
    </div>
  {:else}
    <div class="list-head">
      <span data-testid="journal-count">{total} {total === 1 ? "entry" : "entries"}</span>
      <Switch size="sm" label="Only not reverted" checked={onlyOpen} data-testid="journal-only-open" onchange={(e) => { onlyOpen = e.currentTarget.checked; void load(); }} />
    </div>
    <ul class="entries" data-testid="journal-list">
      {#each entries as e (e.id)}
        {@const target = entryTarget(e)}
        <li class="entry" data-testid="journal-entry" data-id={e.id} data-reverted={e.reverted_at !== undefined}>
          <Badge kind="method" value={e.method} />
          <div class="what">
            <span class="url mono" title={e.url}>{e.url}</span>
            <span class="meta">
              {timeLabel(e.at)} · {e.status}{e.environment ? ` · ${e.environment}` : ""}{target ? ` · ${target}` : ""}
            </span>
          </div>
          {#if e.reverted_at !== undefined}
            <Badge tone="ok" value="reverted" />
          {:else}
            <Button size="sm" variant="secondary" icon="undo" data-testid="journal-revert" onclick={() => openRevert(e)}>Revert…</Button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if reverting}
  <Dialog
    title="Revert #{reverting.id}"
    description="{reverting.method} {reverting.url}"
    icon="undo"
    size="md"
    testid="journal-revert-dialog"
    closeOnBackdrop={false}
    onclose={() => {
      // Esc on the production question closes only the question.
      if (askProduction) askProduction = false;
      else closeRevert();
    }}
  >
    <div class="body">
      <p class="hint">
        Pick the request that puts it back. Its <code>{"{{journal.id}}"}</code>, <code>{"{{journal.location}}"}</code>
        and <code>{"{{journal.etag}}"}</code> are filled from this entry.
      </p>
      {#if choices.length === 0}
        <Banner tone="info">This collection has no request yet.</Banner>
      {:else}
        <Select
          label="Request"
          value={picked}
          options={choices.map((c) => ({ value: c.path, label: c.label }))}
          data-testid="journal-pick"
          onchange={(e) => {
            picked = e.currentTarget.value;
            plan = null;
          }}
        />
      {/if}
      {#if planError}
        <div data-testid="journal-plan-error"><Banner tone="danger">{planError}</Banner></div>
      {/if}
      {#if plan}
        <div class="plan" data-testid="journal-plan" data-production={plan.production}>
          {#each plan.steps as s (s.entry_id)}
            <div class="step">
              <Badge kind="method" value={s.method} />
              <span class="mono url" data-testid="journal-plan-url">{s.url}</span>
              <Badge tone={s.effect === "write" ? "warn" : "info"} value={s.effect === "write" ? "writes" : "reads"} data-testid="journal-plan-effect" />
            </div>
            {#if s.state === "not-reversible"}
              <p class="hint">Not reversible: {s.reason}</p>
            {/if}
          {/each}
          <p class="hint" data-testid="journal-plan-env">
            Environment: <strong>{plan.environment ?? "none"}</strong>
            {#if plan.production}<Badge tone="danger" value="PRODUCTION" />{/if}
          </p>
        </div>
      {/if}
    </div>
    <DialogFooter>
      <Button variant="secondary" data-testid="journal-cancel" onclick={closeRevert}>Cancel</Button>
      <Button variant="secondary" data-testid="journal-new-request" onclick={newRequest}>New request…</Button>
      {#if plan}
        <Button variant={plan.production ? "danger" : "primary"} icon="send" loading={sending} data-testid="journal-send" data-autofocus onclick={send}>Send</Button>
      {:else}
        <Button variant="primary" loading={planning} disabled={!picked} data-testid="journal-plan-button" data-autofocus onclick={() => void makePlan()}>Show plan</Button>
      {/if}
    </DialogFooter>
  </Dialog>
{/if}

{#if askProduction && plan}
  <Dialog
    title={productionQuestion(plan)}
    description="{plan.environment ?? ''} is a production environment. The request is sent as shown."
    icon="warning"
    size="sm"
    testid="journal-production"
    closeOnBackdrop={false}
    onclose={() => (askProduction = false)}
  >
    <DialogFooter>
      <Button variant="secondary" data-testid="journal-production-cancel" onclick={() => (askProduction = false)}>Cancel</Button>
      <Button variant="danger" data-testid="journal-production-yes" data-autofocus onclick={() => void run(true)}>Revert</Button>
    </DialogFooter>
  </Dialog>
{/if}

<style>
  .journal {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
    padding: var(--rt-space-6);
    overflow: auto;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-5) var(--rt-space-6);
  }
  .idpath {
    flex: 0 1 240px;
    min-width: 160px;
  }
  .actions {
    margin-left: auto;
  }
  .note,
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .loading {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .entries {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .entry {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    padding: var(--rt-space-3) var(--rt-space-5);
    border-bottom: 1px solid var(--rt-border);
  }
  .entry:last-child {
    border-bottom: 0;
  }
  .entry[data-reverted="true"] .url {
    text-decoration: line-through;
    color: var(--rt-fg-muted);
  }
  .what {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .url {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .mono {
    font-family: var(--rt-font-mono, monospace);
  }
  .results {
    margin: 0;
    padding-left: var(--rt-space-6);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .plan {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-sunken);
  }
  .plan[data-production="true"] {
    border-color: var(--rt-danger-border);
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
  }
</style>
