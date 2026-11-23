<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Both answers of a request side by side (wave 14, strand H, `RequestComparison.sides`):
     status, a time bar each, size, headers (only those that differ, or all), the body as a
     JSON tree diff or a text diff with the words that differ marked, and the tests whose
     outcome changed. Secrets arrive masked. -->
<script lang="ts">
  import type { RequestComparison, ResponseSnapshot } from "../commands/types";
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
</script>

<div class="sides" data-testid="cmp-response">
  <div class="cols head">
    {#each [side(old), side(now)] as s, i (i)}
      <div class="col" data-testid={i === 0 ? "cmp-old-side" : "cmp-new-side"}>
        <span class="which">{i === 0 ? "Before" : "After"}</span>
        {#if s}
          <span class="status {statusClass(s.status)}" data-testid="cmp-status">{s.status}</span>
          <span class="meta">{formatBytes(s.size)}{s.content_type ? ` · ${s.content_type.split(";")[0]}` : ""}</span>
        {:else}
          <span class="meta">{i === 0 ? (response.error?.old ?? "Not sent at this revision") : (response.error?.new ?? "Not sent at this revision")}</span>
        {/if}
      </div>
    {/each}
  </div>

  {#if old || now}
    <div class="cols time" data-testid="cmp-time" aria-label="Response time">
      {#each [bars.old, bars.new] as b, i (i)}
        <div class="col">
          {#if b}
            <span class="bar" class:slower={i === 1 && bars.slower} class:faster={i === 1 && bars.faster} style="width: {b.percent}%"></span>
            <span class="ms">{b.ms} ms</span>
          {/if}
        </div>
      {/each}
    </div>
    {#if bars.delta}<p class="delta" class:slower={bars.slower} class:faster={bars.faster} data-testid="cmp-delta">{bars.delta}</p>{/if}
  {/if}

  {#if response.error && (response.error.old || response.error.new) && old && now}
    <p class="error">{response.error.new ?? response.error.old}</p>
  {/if}

  {#if response.tests.length > 0}
    <section>
      <h4>Tests that changed</h4>
      <ul class="tests" data-testid="cmp-tests">
        {#each response.tests as t (t.name)}
          <li>
            <span class="t {t.old === null ? 'none' : t.old ? 'pass' : 'fail'}">{t.old === null ? "not run" : t.old ? "passed" : "failed"}</span>
            <span class="arrow">→</span>
            <span class="t {t.new === null ? 'none' : t.new ? 'pass' : 'fail'}">{t.new === null ? "not run" : t.new ? "passed" : "failed"}</span>
            <span class="tname">{t.name}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if headers.length > 0}
    <section>
      <h4>
        Headers
        <label class="toggle"><input type="checkbox" bind:checked={allHeaders} data-testid="cmp-all-headers" /> all headers</label>
      </h4>
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
      <h4>Body</h4>
      {#if (old && old.body === undefined) || (now && now.body === undefined)}
        <p class="muted" data-testid="cmp-binary">Binary body: before {old ? formatBytes(old.size) : "none"}, after {now ? formatBytes(now.size) : "none"}.</p>
      {:else if asJson}
        <JsonTreeDiff old={jsonOld?.value} now={jsonNew?.value} hasOld={!!jsonOld} hasNew={!!jsonNew} />
      {:else if (old?.body ?? "") === "" && (now?.body ?? "") === ""}
        <p class="muted">No body on either side.</p>
      {:else}
        <TextSides old={old?.body ?? ""} now={now?.body ?? ""} hasOld={!!old} hasNew={!!now} />
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
    gap: 10px;
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 12px;
  }
  .col {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .which {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    width: 3.6em;
  }
  .status {
    font-family: var(--rt-font-mono);
    font-weight: 600;
    padding: 0 6px;
    border-radius: var(--rt-radius);
  }
  .status.ok {
    color: var(--rt-ok);
    background: var(--rt-diff-add-bg);
  }
  .status.warn {
    color: var(--rt-warn);
  }
  .status.bad {
    color: var(--rt-danger);
    background: var(--rt-diff-del-bg);
  }
  .meta {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time .col {
    gap: 6px;
  }
  .bar {
    height: 6px;
    min-width: 2px;
    border-radius: 3px;
    background: var(--rt-fg-muted);
  }
  .bar.slower {
    background: var(--rt-warn);
  }
  .bar.faster {
    background: var(--rt-ok);
  }
  .ms {
    font-size: var(--rt-size-small);
    font-variant-numeric: tabular-nums;
    color: var(--rt-fg-muted);
  }
  .delta {
    margin: -4px 0 0;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .delta.slower {
    color: var(--rt-warn);
  }
  .delta.faster {
    color: var(--rt-ok);
  }
  .error {
    margin: 0;
    color: var(--rt-danger);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h4 {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0;
    font-size: var(--rt-size-small);
    font-weight: 600;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 400;
    color: var(--rt-fg-muted);
  }
  .muted {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .tests {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tests li {
    display: flex;
    gap: 6px;
    align-items: baseline;
    font-size: var(--rt-size-small);
  }
  .t.pass {
    color: var(--rt-ok);
  }
  .t.fail {
    color: var(--rt-danger);
    font-weight: 600;
  }
  .t.none,
  .arrow {
    color: var(--rt-fg-muted);
  }
  .headers {
    display: grid;
    grid-template-columns: minmax(80px, max-content) minmax(0, 1fr) minmax(0, 1fr);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-size: var(--rt-size-small);
    overflow: hidden;
  }
  .hrow {
    display: contents;
  }
  .hrow > * {
    padding: 2px 8px;
    border-bottom: 1px solid var(--rt-border);
    min-width: 0;
  }
  .hrow:last-child > * {
    border-bottom: 0;
  }
  .hname {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .vol {
    margin-left: 6px;
    font-weight: 400;
    color: var(--rt-fg-muted);
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
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-ui);
  }
</style>
