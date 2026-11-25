<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The webhook inbox (wave 18, strand E): a local receiver on loopback that answers with a fixed
     reply and shows what arrived. It starts only here (Start), never with the app, and keeps
     requests in memory. "Make request" proposes a request that replays the capture against
     {{baseUrl}}, secrets as variables without a value (AdditionsDialog, unsaved). -->
<script lang="ts">
  import { onMount, untrack } from "svelte";

  import type { CapturedRequest, InboxSettings, InboxStatus } from "../commands/types";
  import AdditionsDialog from "../plugins/AdditionsDialog.svelte";
  import { applyAdditions, isEmptyPlan, planAdditions, type AdditionPlan } from "../plugins/additions";
  import CodeView from "../response/CodeView.svelte";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, EmptyState, Icon, IconButton, TextArea, TextField } from "../ui";
  import {
    ROW_HEIGHT,
    applyEvents,
    bodyView,
    clock,
    cleanSettings,
    matches,
    proposal,
    sizeLabel,
    trimTo,
    visibleRange,
  } from "./model";

  let { app }: ToolProps = $props();

  const POLL_MS = 250;

  let status = $state<InboxStatus>({ running: false, held: 0, dropped: 0 });
  let settings = $state<InboxSettings>({ port: 47855, status: 200, body: "", delay_ms: 0 });
  let list = $state.raw<CapturedRequest[]>([]);
  let since = 0;
  let selectedId = $state<number | null>(null);
  let query = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let copied = $state(false);
  let showReply = $state(false);

  let plan = $state.raw<AdditionPlan | null>(null);
  let adding = $state(false);
  let addError = $state<string | null>(null);

  // The newest first.
  const shown = $derived(list.filter((r) => matches(r, query)).reverse());
  const selected = $derived(list.find((r) => r.id === selectedId) ?? null);
  const body = $derived(selected ? bodyView(selected) : null);

  let scrollTop = $state(0);
  let viewport = $state(400);
  const range = $derived(visibleRange(scrollTop, viewport, shown.length));
  const rows = $derived(shown.slice(range[0], range[1]).map((r, i) => ({ r, index: range[0] + i })));

  const fail = (e: unknown) => {
    error = e instanceof Error ? e.message : String(e);
  };

  async function poll() {
    try {
      const page = await app.backend.inbox.events(since);
      since = page.next;
      if (page.events.length > 0) list = applyEvents(list, page.events);
      status = await app.backend.inbox.status();
      list = trimTo(list, status.held);
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => {
    void (async () => {
      try {
        settings = await app.backend.inbox.settings();
        status = await app.backend.inbox.status();
        const page = await app.backend.inbox.events(0);
        since = page.next;
        list = applyEvents([], page.events);
        list = trimTo(list, status.held);
      } catch (e) {
        fail(e);
      }
    })();
    const timer = setInterval(() => void poll(), POLL_MS);
    return () => clearInterval(timer);
  });

  // A request that fell out of the buffer is no longer selected.
  $effect(() => {
    if (selectedId !== null && !list.some((r) => r.id === selectedId)) untrack(() => (selectedId = null));
  });

  async function toggle() {
    busy = true;
    error = null;
    try {
      if (status.running) {
        status = await app.backend.inbox.stop();
      } else {
        const clean = cleanSettings($state.snapshot(settings));
        settings = clean;
        await app.backend.inbox.setSettings(clean);
        status = await app.backend.inbox.start();
      }
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function saveReply() {
    error = null;
    try {
      const clean = cleanSettings($state.snapshot(settings));
      settings = clean;
      await app.backend.inbox.setSettings(clean);
    } catch (e) {
      fail(e);
    }
  }

  async function clear() {
    try {
      await app.backend.inbox.clear();
      await poll();
    } catch (e) {
      fail(e);
    }
  }

  async function copyUrl() {
    if (!status.url) return;
    try {
      await navigator.clipboard.writeText(status.url);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      fail(e);
    }
  }

  async function makeRequest(r: CapturedRequest) {
    error = null;
    try {
      const draft = await app.backend.inbox.makeRequest(r.id);
      const p = planAdditions(proposal(draft, app.environment), app.tree, app.environments);
      if (isEmptyPlan(p)) return;
      addError = null;
      plan = p;
    } catch (e) {
      fail(e);
    }
  }

  async function confirmPlan() {
    if (!plan) return;
    adding = true;
    addError = null;
    try {
      await applyAdditions(app.backend, plan);
      app.tree = await app.backend.tree();
      await app.reloadEnvironments();
      plan = null;
    } catch (e) {
      addError = e instanceof Error ? e.message : String(e);
    } finally {
      adding = false;
    }
  }

  const port = $derived(status.port ?? settings.port);
</script>

<div class="inbox" data-testid="inbox">
  <section class="bar" aria-label="Receiver">
    <Button
      variant={status.running ? "secondary" : "primary"}
      icon={status.running ? "close" : "play"}
      loading={busy}
      onclick={() => void toggle()}
      data-testid="inbox-toggle"
    >
      {status.running ? "Stop" : "Start"}
    </Button>
    {#if status.running}
      <Badge kind="label" tone="ok" value="Listening" />
      <code class="url" data-testid="inbox-url">{status.url}</code>
      <Button variant="ghost" size="sm" icon={copied ? "check" : "copy"} onclick={() => void copyUrl()} data-testid="inbox-copy-url">
        {copied ? "Copied" : "Copy URL"}
      </Button>
    {:else}
      <span class="muted" data-testid="inbox-off">Not listening. It starts only when you press Start, on 127.0.0.1 port {port}.</span>
    {/if}
    <span class="grow"></span>
    <Button variant="ghost" size="sm" icon="settings" onclick={() => (showReply = !showReply)} aria-expanded={showReply} data-testid="inbox-reply-toggle">
      Reply
    </Button>
    <Button variant="ghost" size="sm" icon="delete" disabled={list.length === 0} onclick={() => void clear()} data-testid="inbox-clear">Clear</Button>
  </section>

  {#if error}
    <Banner tone="danger" data-testid="inbox-error">{error}</Banner>
  {/if}
  {#if status.dropped > 0}
    <p class="note" data-testid="inbox-dropped">{status.dropped} older {status.dropped === 1 ? "request" : "requests"} dropped (the inbox keeps the newest 500).</p>
  {/if}

  {#if showReply}
    <section class="reply" aria-label="Fixed reply" data-testid="inbox-reply">
      <p class="muted">
        The inbox answers every request with this reply, so services that expect a receipt are satisfied. A new port takes effect at the next start. Nothing leaves this
        computer: there is no tunnel (to receive calls from the internet, point your own tool at this port).
      </p>
      <div class="fields">
        <TextField label="Port" type="number" size="sm" bind:value={() => String(settings.port), (v) => (settings.port = Number(v))} data-testid="inbox-port" hint="0 picks a free port" />
        <TextField label="Status" type="number" size="sm" bind:value={() => String(settings.status), (v) => (settings.status = Number(v))} data-testid="inbox-status-code" />
        <TextField label="Delay (ms, at most 5000)" type="number" size="sm" bind:value={() => String(settings.delay_ms), (v) => (settings.delay_ms = Number(v))} data-testid="inbox-delay" />
        <TextField label="Content type" size="sm" bind:value={() => settings.content_type ?? "", (v) => (settings.content_type = v)} data-testid="inbox-content-type" />
      </div>
      <TextArea label="Body" rows={3} mono bind:value={settings.body} data-testid="inbox-body" />
      <div><Button size="sm" onclick={() => void saveReply()} data-testid="inbox-reply-save">Apply</Button></div>
    </section>
  {/if}

  <div class="split">
    <section class="left" aria-label="Requests received">
      <TextField icon="search" size="sm" placeholder="Search method, path, headers, body" aria-label="Search the inbox" bind:value={query} data-testid="inbox-search" />
      {#if list.length === 0}
        <EmptyState
          icon="inbox"
          title={status.running ? "Waiting for a request" : "Nothing received yet"}
          text={status.running ? `Send a request to ${status.url ?? ""} and it shows up here.` : "Press Start, then point a service or curl at the address."}
          data-testid="inbox-empty"
        />
      {:else if shown.length === 0}
        <EmptyState icon="search" title="No match" text="No request matches this search." />
      {:else}
        <div
          class="list"
          role="listbox"
          aria-label="Requests"
          tabindex="0"
          bind:clientHeight={viewport}
          onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
          data-testid="inbox-list"
        >
          <div class="spacer" style:height="{shown.length * ROW_HEIGHT}px">
            {#each rows as { r, index } (r.id)}
              <button
                class="row"
                class:selected={r.id === selectedId}
                role="option"
                aria-selected={r.id === selectedId}
                style:top="{index * ROW_HEIGHT}px"
                onclick={() => (selectedId = r.id)}
                data-testid="inbox-row"
              >
                <span class="time">{clock(r.at)}</span>
                <Badge kind="method" value={r.method} />
                <span class="target" title={r.target}>{r.target}</span>
                <span class="size">{sizeLabel(r.size)}</span>
              </button>
            {/each}
          </div>
        </div>
        <p class="count muted" data-testid="inbox-count">{shown.length === list.length ? `${list.length} held` : `${shown.length} of ${list.length}`}</p>
      {/if}
    </section>

    <section class="right" aria-label="Request detail" data-testid="inbox-detail">
      {#if selected}
        <header>
          <Badge kind="method" value={selected.method} />
          <code class="where">{selected.target}</code>
          <span class="grow"></span>
          <Button variant="primary" size="sm" icon="add" disabled={!app.collection} onclick={() => void makeRequest(selected)} data-testid="inbox-make-request">
            Make request
          </Button>
        </header>
        {#if !app.collection}
          <p class="note">Open a collection to turn this into a request.</p>
        {/if}
        <h4>Headers</h4>
        <table class="headers" data-testid="inbox-headers">
          <tbody>
            {#each selected.headers as [name, value], i (i)}
              <tr><th scope="row">{name}</th><td>{value}</td></tr>
            {/each}
          </tbody>
        </table>
        <h4>Body <span class="muted">{sizeLabel(selected.size)}</span></h4>
        {#if body}
          {#key selected.id}
            <div class="code"><CodeView text={body.text} lang={body.lang} label="Request body" testid="inbox-body-view" /></div>
          {/key}
        {:else if selected.size > 0}
          <p class="muted" data-testid="inbox-binary">The body is not text ({sizeLabel(selected.size)}); it is not kept.</p>
        {:else}
          <p class="muted">No body.</p>
        {/if}
      {:else}
        <EmptyState icon="inbox" title="Select a request" text="Headers and body show here. “Make request” replays it against {'{{baseUrl}}'}." />
      {/if}
    </section>
  </div>
</div>

{#if plan}
  <AdditionsDialog
    plugin="The webhook inbox"
    {plan}
    busy={adding}
    error={addError}
    onconfirm={() => void confirmPlan()}
    oncancel={() => (plan = null)}
  />
{/if}

<style>
  .inbox {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-3) var(--rt-space-4);
    min-width: 0;
    min-height: 0;
    flex: 1;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
  }
  .url {
    font: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--rt-text-muted);
    font: var(--rt-font-small);
  }
  .note {
    margin: 0;
    color: var(--rt-text-muted);
    font: var(--rt-font-small);
  }
  .reply {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .reply p {
    margin: 0;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    gap: var(--rt-space-3);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(14rem, 2fr) minmax(0, 3fr);
    gap: var(--rt-space-4);
    min-height: 0;
    flex: 1;
  }
  @media (max-width: 800px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .left,
  .right {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    min-width: 0;
    min-height: 12rem;
  }
  .list {
    position: relative;
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
    min-height: 8rem;
    max-height: 60vh;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    height: 32px;
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--rt-space-2);
    padding: 0 var(--rt-space-3);
    border: 0;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
    font: var(--rt-font-small);
  }
  .row:hover {
    background: var(--rt-surface-2);
  }
  .row.selected {
    background: var(--rt-accent-soft, var(--rt-surface-3));
  }
  .row:focus-visible {
    outline: none;
    box-shadow: var(--rt-focus-ring);
  }
  .time,
  .size {
    color: var(--rt-text-muted);
    font-variant-numeric: tabular-nums;
  }
  .target {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--rt-font-mono-family, monospace);
  }
  .count {
    margin: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .where {
    overflow-wrap: anywhere;
    font: var(--rt-font-mono);
  }
  h4 {
    margin: var(--rt-space-2) 0 0;
    font: var(--rt-font-title);
  }
  .headers {
    border-collapse: collapse;
    width: 100%;
    font: var(--rt-font-small);
  }
  .headers th {
    text-align: left;
    font-weight: 600;
    padding: 2px var(--rt-space-3) 2px 0;
    white-space: nowrap;
    vertical-align: top;
  }
  .headers td {
    overflow-wrap: anywhere;
    font-family: var(--rt-font-mono-family, monospace);
  }
  .code {
    position: relative;
    height: 16rem;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    overflow: hidden;
    display: flex;
  }
</style>
