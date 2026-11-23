<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Both answers of a request side by side (wave 14, strand H, `RequestComparison.sides`):
     status, a time bar each, size, headers (only those that differ, or all), the body as a
     JSON tree diff or a text diff with the words that differ marked, and the tests whose
     outcome changed. Secrets arrive masked. -->
<script lang="ts">
  import type { RequestComparison, ResponseSnapshot } from "../commands/types";
  import { Badge, Icon, Switch } from "../ui";
  import JsonTreeDiff from "./JsonTreeDiff.svelte";
  import { parseJson } from "./jsontree";
  import { formatBytes, headerRows, statusClass, timeBars } from "./sides";
  import TextSides from "./TextSides.svelte";
  import Words from "./Words.svelte";
  import { wordDiff } from "./worddiff";

  let { response }: { response: RequestComparison } = $props();

  const old = $derived(response.sides?.old ?? null);
  const now = $derived(response.sides?.new ?? null);
  const bars = $derived(timeBars(old?.duration_ms ?? null, now?.duration_ms ?? null));
  let allHeaders = $state(false);
  const headers = $derived(headerRows(old?.headers ?? null, now?.headers ?? null));
  const shownHeaders = $derived(allHeaders ? headers : headers.filter((h) => h.kind !== "same"));
  const jsonOld = $derived(parseJson(old?.body));
  const jsonNew = $derived(parseJson(now?.body));
  const asJson = $derived((!old?.body || jsonOld) && (!now?.body || jsonNew) && (jsonOld || jsonNew));

  const side = (s: ResponseSnapshot | null) => s;
  const TONE = { ok: "ok", warn: "warn", bad: "danger" } as const;
  const testTone = (v: boolean | null) => (v === null ? "neutral" : v ? "ok" : "danger");
  const testText = (v: boolean | null) => (v === null ? "not run" : v ? "passed" : "failed");
</script>

<div class="sides" data-testid="cmp-response">
  <div class="summary">
    <div class="cols head">
      {#each [side(old), side(now)] as s, i (i)}
        <div class="col" data-testid={i === 0 ? "cmp-old-side" : "cmp-new-side"}>
          <span class="which">{i === 0 ? "Before" : "After"}</span>
          {#if s}
            <span class="status" data-testid="cmp-status"><Badge tone={TONE[statusClass(s.status)]} value={s.status} /></span>
            <span class="meta">{formatBytes(s.size)}{s.content_type ? ` · ${s.content_type.split(";")[0]}` : ""}</span>
          {:else}
            <span class="meta none">{i === 0 ? (response.error?.old ?? "Not sent at this revision") : (response.error?.new ?? "Not sent at this revision")}</span>
          {/if}
        </div>
      {/each}
    </div>

    {#if old || now}
      <div class="cols time" data-testid="cmp-time" aria-label="Response time">
        {#each [bars.old, bars.new] as b, i (i)}
          <div class="col">
            {#if b}
              <span class="track"><span class="bar" class:slower={i === 1 && bars.slower} class:faster={i === 1 && bars.faster} style="width: {b.percent}%"></span></span>
              <span class="ms">{b.ms} ms</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
  {#if (old || now) && bars.delta}
    <p class="delta" class:slower={bars.slower} class:faster={bars.faster} data-testid="cmp-delta">
      <Icon name={bars.slower ? "warning" : bars.faster ? "zap" : "info"} size="sm" />
      {bars.delta}
    </p>
  {/if}

  {#if response.error && (response.error.old || response.error.new) && old && now}
    <p class="error"><Icon name="error" size="sm" /> {response.error.new ?? response.error.old}</p>
  {/if}

  {#if response.tests.length > 0}
    <section>
      <h5>Tests that changed</h5>
      <ul class="tests" data-testid="cmp-tests">
        {#each response.tests as t (t.name)}
          <li>
            <Badge tone={testTone(t.old)} value={testText(t.old)} />
            <span class="arrow" aria-label="to"><Icon name="chevron-right" size="sm" /></span>
            <Badge tone={testTone(t.new)} value={testText(t.new)} solid={t.new === false && t.old === true} />
            <span class="tname">{t.name}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if headers.length > 0}
    <section>
      <div class="sub">
        <h5>Headers</h5>
        <Switch size="sm" label="All headers" bind:checked={allHeaders} data-testid="cmp-all-headers" />
      </div>
      {#if shownHeaders.length === 0}
        <p class="muted" data-testid="cmp-headers-same">The headers are the same (left out: {headers.length}).</p>
      {:else}
        <div class="headers" data-testid="cmp-headers" role="table" aria-label="Headers, before and after">
          {#each shownHeaders as h (h.key)}
            <div class="hrow {h.kind}" role="row" data-name={h.name}>
              <span class="hname" role="cell">{h.name}{#if h.volatile}<span class="vol" title="Changes on every response">varies</span>{/if}</span>
              {#if h.old !== null && h.new !== null && h.kind === "changed"}
                {@const w = wordDiff(h.old, h.new)}
                <span class="hval old" role="cell"><Words segments={w.old} side="old" /></span>
                <span class="hval new" role="cell"><Words segments={w.new} side="new" /></span>
              {:else}
                <span class="hval old" class:none={h.old === null} role="cell">{h.old ?? "not sent"}</span>
                <span class="hval new" class:none={h.new === null} role="cell">{h.new ?? "not sent"}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#if old || now}
    <section>
      <h5>Body</h5>
      {#if (old && old.body === undefined) || (now && now.body === undefined)}
        <p class="muted" data-testid="cmp-binary">Binary body: before {old ? formatBytes(old.size) : "none"}, after {now ? formatBytes(now.size) : "none"}.</p>
      {:else if asJson}
        <JsonTreeDiff old={jsonOld?.value} now={jsonNew?.value} hasOld={!!jsonOld} hasNew={!!jsonNew} />
      {:else if (old?.body ?? "") === "" && (now?.body ?? "") === ""}
        <p class="muted">No body on either side.</p>
      {:else}
        <div class="text"><TextSides old={old?.body ?? ""} now={now?.body ?? ""} hasOld={!!old} hasNew={!!now} /></div>
      {/if}
      {#if old?.truncated || now?.truncated}
        <p class="muted">Long bodies are compared by their first 256 KB.</p>
      {/if}
    </section>
  {/if}
</div>

<style>
  .sides {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
  }
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    padding: var(--rt-space-5);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: var(--rt-space-6);
  }
  .col {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    min-width: 0;
  }
  .col + .col {
    padding-left: var(--rt-space-6);
    border-left: 1px solid var(--rt-border);
  }
  .cols.head .col {
    min-height: var(--rt-control-sm);
  }
  .which {
    flex: none;
    width: 4.5em;
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .status {
    display: inline-flex;
    flex: none;
    font-family: var(--rt-font-mono);
  }
  .meta {
    min-width: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta.none {
    font-style: italic;
  }
  .track {
    flex: 1;
    min-width: 0;
    height: var(--rt-space-3);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-sunken);
    overflow: hidden;
  }
  .bar {
    display: block;
    height: 100%;
    min-width: var(--rt-space-1);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-fg-subtle);
    transition: width var(--rt-dur-fast) var(--rt-ease);
  }
  .bar.slower {
    background: var(--rt-warn);
  }
  .bar.faster {
    background: var(--rt-ok);
  }
  .ms {
    flex: none;
    min-width: 7ch;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    font-variant-numeric: tabular-nums;
    text-align: right;
    white-space: nowrap;
  }
  .delta,
  .error {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    margin: calc(-1 * var(--rt-space-4)) 0 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .delta.slower {
    color: var(--rt-warn-fg);
  }
  .delta.faster {
    color: var(--rt-ok-fg);
  }
  .error {
    margin: 0;
    color: var(--rt-danger-fg);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .sub {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-5);
  }
  h5 {
    margin: 0;
    font: var(--rt-font-body);
    font-weight: var(--rt-weight-semibold);
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .tests {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .tests li {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    min-width: 0;
  }
  .arrow {
    display: inline-flex;
    color: var(--rt-fg-subtle);
  }
  .tname {
    margin-left: var(--rt-space-3);
    min-width: 0;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    overflow-wrap: anywhere;
  }
  .headers {
    display: grid;
    grid-template-columns: minmax(96px, max-content) minmax(0, 1fr) minmax(0, 1fr);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    font: var(--rt-font-small);
    overflow: hidden;
  }
  .hrow {
    display: contents;
  }
  .hrow > * {
    min-width: 0;
    padding: var(--rt-space-2) var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
  }
  .hrow:last-child > * {
    border-bottom: 0;
  }
  .hname {
    font-weight: var(--rt-weight-medium);
    overflow-wrap: anywhere;
  }
  .vol {
    margin-left: var(--rt-space-3);
    color: var(--rt-fg-subtle);
    font-weight: var(--rt-weight-regular);
  }
  .hval {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .hrow.changed .old,
  .hrow.removed .old {
    background: var(--rt-diff-del-bg);
  }
  .hrow.changed .new,
  .hrow.added .new {
    background: var(--rt-diff-add-bg);
  }
  .hval.none {
    color: var(--rt-fg-subtle);
    font-family: var(--rt-font-ui);
    font-style: italic;
  }
  .text {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    overflow: hidden;
  }
</style>
