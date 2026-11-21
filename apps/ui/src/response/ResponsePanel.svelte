<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One received response: summary bar, body (pretty / raw / tree / hex, JSONPath filter),
     headers and timing. Created per response (keyed by its id), so all state here belongs to
     that response. The full body stays in the backend; the UI only fetches what it shows. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { ReceivedResponse, StoredResponse } from "../commands/types";
  import {
    formatDuration,
    formatSize,
    JSON_VIEW_LIMIT,
    looksLikeJson,
    reasonPhrase,
    statusClass,
    waterfall,
  } from "./format";
  import HeadersTable from "./HeadersTable.svelte";
  import JsonNode from "./JsonNode.svelte";
  import { arraySource, hexSource, type LineSource } from "./lines";
  import TimingWaterfall from "./TimingWaterfall.svelte";
  import VirtualLines from "./VirtualLines.svelte";

  let {
    result,
    response,
    backend,
  }: { result: StoredResponse; response: ReceivedResponse; backend: Backend } = $props();

  /** Bytes of the preview that is the complete body (small bodies need no round trip). */
  const PREVIEW_BYTES = 64 * 1024;

  type Tab = "body" | "headers" | "timing";
  type Mode = "pretty" | "raw" | "tree" | "hex";

  // The props are fixed for the lifetime of this component (it is keyed by response id).
  // svelte-ignore state_referenced_locally
  const id = result.response_id!;
  // svelte-ignore state_referenced_locally
  const jsonCandidate = result.is_text && looksLikeJson(response);
  // svelte-ignore state_referenced_locally
  const jsonViewable = jsonCandidate && result.size <= JSON_VIEW_LIMIT;

  let tab = $state<Tab>("body");
  // svelte-ignore state_referenced_locally
  let mode = $state<Mode>(jsonViewable ? "pretty" : result.is_text ? "raw" : "hex");

  // ---- JSON (pretty view and tree), parsed in the UI for bodies up to JSON_VIEW_LIMIT ----

  let json = $state.raw<
    | { state: "loading" }
    | { state: "ok"; value: unknown; pretty: LineSource }
    | { state: "error"; message: string }
    | { state: "off" }
  >({ state: jsonViewable ? "loading" : "off" });

  async function bodyText(): Promise<string> {
    if (!response.truncated && result.size <= PREVIEW_BYTES && response.body.encoding === "utf8") {
      return response.body.data;
    }
    return new TextDecoder().decode(await backend.responseBytes(id, 0, result.size));
  }

  if (jsonViewable) {
    bodyText()
      .then((text) => {
        const value: unknown = JSON.parse(text);
        // `$state.raw`: the parsed value is never mutated, so it stays out of proxies.
        json = { state: "ok", value, pretty: arraySource(JSON.stringify(value, null, 2).split("\n")) };
      })
      .catch((e: unknown) => {
        json = { state: "error", message: e instanceof Error ? e.message : String(e) };
        if (mode === "pretty" || mode === "tree") mode = "raw";
      });
  }

  const raw: LineSource = {
    // svelte-ignore state_referenced_locally
    count: result.line_count,
    get: (start, count) => backend.responseLines(id, start, count),
  };
  // svelte-ignore state_referenced_locally
  const hex = hexSource(result.size, (offset, length) => backend.responseBytes(id, offset, length));

  const jsonUnavailable = $derived(
    !jsonCandidate
      ? "The body is not JSON"
      : !jsonViewable
        ? `Pretty view and tree are available for JSON up to ${formatSize(JSON_VIEW_LIMIT)}`
        : json.state === "error"
          ? `The body is not valid JSON: ${json.message}`
          : null,
  );

  // ---- JSONPath filter (in the backend, so it works for bodies of any size it accepts) ----

  let expression = $state("");
  let filtered = $state.raw<{ expression: string; source: LineSource } | null>(null);
  let filterError = $state<string | null>(null);
  let filterSeq = 0;
  let filterTimer: ReturnType<typeof setTimeout> | undefined;

  async function runFilter(expr: string) {
    clearTimeout(filterTimer);
    const seq = ++filterSeq;
    if (expr.trim() === "") {
      filtered = null;
      filterError = null;
      return;
    }
    try {
      const text = await backend.filterJson(id, expr.trim());
      if (seq !== filterSeq) return;
      filtered = { expression: expr, source: arraySource(text.split("\n")) };
      filterError = null;
    } catch (e) {
      if (seq !== filterSeq) return;
      filtered = null;
      filterError = e instanceof Error ? e.message : String(e);
    }
  }

  function oninput() {
    clearTimeout(filterTimer);
    const expr = expression;
    filterTimer = setTimeout(() => runFilter(expr), 250);
  }

  function onfilterkey(e: KeyboardEvent) {
    if (e.key === "Enter") runFilter(expression);
    if (e.key === "Escape") {
      expression = "";
      runFilter("");
    }
  }

  $effect(() => () => clearTimeout(filterTimer));

  // ---- copy ----

  let copied = $state(false);
  // svelte-ignore state_referenced_locally
  const copyable = result.is_text && result.size <= JSON_VIEW_LIMIT;

  async function copy() {
    try {
      await navigator.clipboard.writeText(await bodyText());
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      copied = false;
    }
  }

  // ---- summary ----

  const cls = $derived(statusClass(response.status));
  const timing = $derived(waterfall(response.timing, response.total, backend.transport.mode(), response.url));

  const MODES: { mode: Mode; label: string }[] = [
    { mode: "pretty", label: "Pretty" },
    { mode: "raw", label: "Raw" },
    { mode: "tree", label: "Tree" },
    { mode: "hex", label: "Hex" },
  ];

  function disabledReason(m: Mode): string | null {
    if (m === "pretty" || m === "tree") return jsonUnavailable;
    if (m === "raw" && !result.is_text) return "The body is binary";
    return null;
  }
</script>

<div class="panel">
  <div class="summary" aria-label="Response summary">
    <span class="status {cls}" data-testid="response-status" data-class={cls}>{response.status}</span>
    {#if reasonPhrase(response.status)}<span class="reason {cls}">{reasonPhrase(response.status)}</span>{/if}
    <span class="metric" title="Total time"><span class="label">Time</span><span data-testid="response-time">{formatDuration(response.total)}</span></span>
    <span class="metric" title="Body size"><span class="label">Size</span><span data-testid="response-size">{formatSize(result.size)}</span></span>
    {#if response.truncated}
      <span class="warn" title="The body was cut at the send limit">cut at the limit, {formatSize(response.size)} received</span>
    {/if}
    {#if response.version !== "unknown"}<span class="metric" data-testid="response-version">{response.version}</span>{/if}
    {#if response.redirects.length > 0}
      <span class="metric" title={response.redirects.map((r) => `${r.status} → ${r.location}`).join("\n")}>
        {response.redirects.length} redirect{response.redirects.length === 1 ? "" : "s"}
      </span>
    {/if}
  </div>

  <div class="bar">
    <div class="tabs" role="tablist" aria-label="Response parts">
      {#each [["body", "Body"], ["headers", `Headers (${response.headers.length})`], ["timing", "Timing"]] as [t, label] (t)}
        <button
          type="button"
          role="tab"
          aria-selected={tab === t}
          data-testid="response-tab-{t}"
          onclick={() => (tab = t as Tab)}>{label}</button
        >
      {/each}
    </div>
    {#if tab === "body"}
      <div class="modes" role="group" aria-label="Body view">
        {#each MODES as m (m.mode)}
          <button
            type="button"
            aria-pressed={mode === m.mode}
            disabled={disabledReason(m.mode) !== null}
            title={disabledReason(m.mode) ?? ""}
            data-testid="response-mode-{m.mode}"
            onclick={() => (mode = m.mode)}>{m.label}</button
          >
        {/each}
      </div>
      {#if jsonCandidate}
        <input
          class="filter"
          type="search"
          placeholder="JSONPath filter, e.g. $.items[*].id"
          aria-label="JSONPath filter"
          spellcheck="false"
          autocomplete="off"
          data-testid="response-filter"
          class:invalid={filterError !== null}
          bind:value={expression}
          {oninput}
          onkeydown={onfilterkey}
        />
      {/if}
      {#if copyable}
        <button type="button" class="copy" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
      {/if}
    {/if}
  </div>

  {#if tab === "body" && filterError}
    <p class="filter-error" role="alert" data-testid="response-filter-error">{filterError}</p>
  {/if}

  <div class="content">
    {#if tab === "headers"}
      <HeadersTable response={response.headers} request={result.report.request?.headers ?? null} />
    {:else if tab === "timing"}
      <TimingWaterfall data={timing} />
    {:else if filtered}
      <div class="fill" data-testid="response-filter-result">
        {#key filtered}
          <VirtualLines source={filtered.source} label="JSONPath result" />
        {/key}
      </div>
    {:else if mode === "pretty" || mode === "tree"}
      {#if json.state === "ok"}
        {#if mode === "pretty"}
          <VirtualLines source={json.pretty} label="Response body (pretty)" />
        {:else}
          <div class="tree" data-testid="response-tree">
            <ul><JsonNode name={null} value={json.value} path="$" expanded /></ul>
          </div>
        {/if}
      {:else}
        <p class="note">Loading…</p>
      {/if}
    {:else if mode === "raw"}
      {#if result.line_count === 0}
        <p class="note">Empty body</p>
      {:else}
        <VirtualLines source={raw} label="Response body" />
      {/if}
    {:else if result.size === 0}
      <p class="note">Empty body</p>
    {:else}
      <VirtualLines source={hex} label="Response body (hex)" />
    {/if}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: calc(2 * var(--rt-gap));
    padding: 4px var(--rt-gap);
    border-bottom: 1px solid var(--rt-border);
  }
  .status {
    font-weight: 700;
    font-family: var(--rt-font-mono);
  }
  .reason {
    margin-left: calc(-1.5 * var(--rt-gap));
  }
  .ok {
    color: var(--rt-ok);
  }
  .redirect {
    color: var(--rt-accent);
  }
  .client-error {
    color: var(--rt-warn);
  }
  .server-error {
    color: var(--rt-danger);
  }
  .info {
    color: var(--rt-fg-muted);
  }
  .metric {
    color: var(--rt-fg);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .label {
    margin-right: 4px;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-ui);
  }
  .warn {
    color: var(--rt-warn);
    font-size: var(--rt-size-small);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap);
    padding: 4px var(--rt-gap);
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }
  .tabs,
  .modes {
    display: flex;
    gap: 2px;
  }
  button {
    font: inherit;
    font-size: var(--rt-size-small);
    color: var(--rt-fg);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button[aria-selected="true"],
  button[aria-pressed="true"] {
    background: var(--rt-bg-selected);
    border-color: var(--rt-border);
  }
  button:disabled {
    color: var(--rt-fg-muted);
    cursor: default;
    opacity: 0.6;
  }
  .modes {
    margin-left: auto;
  }
  .filter {
    flex: 0 1 280px;
    min-width: 120px;
    font: var(--rt-size-small) var(--rt-font-mono);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .filter.invalid {
    border-color: var(--rt-danger);
  }
  .filter-error {
    margin: 0;
    padding: 4px var(--rt-gap);
    color: var(--rt-danger);
    font-size: var(--rt-size-small);
    border-bottom: 1px solid var(--rt-border);
  }
  .content {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 240px;
    position: relative;
  }
  .fill {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }
  .tree {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 4px var(--rt-gap);
    font: var(--rt-size-small) / 18px var(--rt-font-mono);
  }
  .tree > ul {
    margin: 0;
    padding: 0;
  }
  .note {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-fg-muted);
  }
</style>
