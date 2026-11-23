<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One received response: summary bar, body (pretty / raw / tree / hex, JSONPath filter;
     pretty is highlighted and foldable by the body's language, up to PRETTY_LIMIT),
     headers, tests and console of scripts (wave 7), and timing. Created per response (keyed by its id), so all state here belongs to
     that response. The full body stays in the backend; the UI only fetches what it shows.
     Wave 13: response tabs of a distribution's extensions (`AppExtension.tabs`) after the
     plugin tabs. -->
<script lang="ts">
  import ExplainButton from "../ai/ExplainButton.svelte";
  import type { Backend } from "../commands/backend";
  import type { ReceivedResponse, StoredResponse } from "../commands/types";
  import { bodyLanguage, prettyBody, type BodyLanguage } from "../pretty/language";
  import CodeView from "./CodeView.svelte";
  import { formatDuration, formatSize, header, PRETTY_LIMIT, reasonPhrase, statusClass, waterfall } from "./format";
  import { contentTypeOf, tabsFor } from "../plugins/permissions";
  import PluginTab from "../plugins/PluginTab.svelte";
  import SendNotes from "../plugins/SendNotes.svelte";
  import { pluginsState } from "../plugins/store.svelte";
  import { extensionTabsFor, getExtensions } from "../shell/extension-tabs";
  import SpecBadge from "../openapi/SpecBadge.svelte";
  import HeadersTable from "./HeadersTable.svelte";
  import ConsolePanel from "./ConsolePanel.svelte";
  import JsonNode from "./JsonNode.svelte";
  import { arraySource, hexSource, type LineSource } from "./lines";
  import TestsPanel from "./TestsPanel.svelte";
  import { testSummary, testsLabel } from "./tests";
  import TimingWaterfall from "./TimingWaterfall.svelte";
  import VirtualLines from "./VirtualLines.svelte";

  let {
    result,
    response,
    backend,
    onpick,
    path = null,
  }: {
    result: StoredResponse;
    response: ReceivedResponse;
    backend: Backend;
    /** The request's path, for the spec check (`SpecBadge`); `null`: no check. */
    path?: string | null;
    /** "Set as variable" from the JSON tree (wave 7); absent: no such button. */
    onpick?: (path: string, value: unknown) => void;
  } = $props();

  /** Bytes of the preview that is the complete body (small bodies need no round trip). */
  const PREVIEW_BYTES = 64 * 1024;

  /** Built-in tabs, `plugin:<plugin>/<tab>` (wave 5) or `extension:<extension>/<tab>` (wave 13). */
  type Tab = "body" | "headers" | "tests" | "console" | "timing" | `plugin:${string}` | `extension:${string}`;
  type Mode = "pretty" | "raw" | "tree" | "hex";

  // The props are fixed for the lifetime of this component (it is keyed by response id).
  // svelte-ignore state_referenced_locally
  const id = result.response_id!;
  // svelte-ignore state_referenced_locally
  const lang: BodyLanguage = result.is_text
    ? bodyLanguage(header(response.headers, "content-type"), response.body.encoding === "utf8" ? response.body.data.slice(0, 1024) : "")
    : "text";
  // svelte-ignore state_referenced_locally
  const tooLarge = result.size > PRETTY_LIMIT;
  /** Pretty view: highlighted (and re-indented for JSON and XML) in the UI. */
  const prettyViewable = lang !== "text" && !tooLarge;
  const jsonCandidate = lang === "json";
  const jsonViewable = jsonCandidate && !tooLarge;

  let tab = $state<Tab>("body");

  // ---- tests and console (wave 7) ----
  const tests = $derived(testSummary(result.report));
  const logs = $derived(result.report.logs ?? []);
  const builtInTabs = $derived<[Tab, string][]>([
    ["body", "Body"],
    ["headers", `Headers (${response.headers.length})`],
    ["tests", testsLabel(tests)],
    ["console", logs.length > 0 ? `Console (${logs.length})` : "Console"],
    ["timing", "Timing"],
  ]);

  // ---- plugin tabs (wave 5, strand A) ----
  // svelte-ignore state_referenced_locally
  const plugins = pluginsState(backend);
  const pluginTabs = $derived(tabsFor(plugins.list, "response", contentTypeOf(response.headers)));
  const pluginTab = $derived(tab.startsWith("plugin:") ? (pluginTabs.find((t) => `plugin:${t.key}` === tab) ?? null) : null);
  // ---- extension tabs (wave 13) ----
  const extensions = getExtensions();
  const extensionTabs = $derived(extensionTabsFor(extensions, "response", contentTypeOf(response.headers)));
  const extensionTab = $derived(
    tab.startsWith("extension:") ? (extensionTabs.find((t) => `extension:${t.key}` === tab) ?? null) : null,
  );
  const settle = () => Promise.resolve();
  // svelte-ignore state_referenced_locally
  let mode = $state<Mode>(prettyViewable ? "pretty" : result.is_text ? "raw" : "hex");

  // ---- pretty view and JSON tree, in the UI for bodies up to PRETTY_LIMIT ----

  let pretty = $state.raw<
    | { state: "loading" }
    | { state: "ok"; text: string; formatted: boolean }
    | { state: "error"; message: string }
    | { state: "off" }
  >({ state: prettyViewable ? "loading" : "off" });
  let json = $state.raw<
    { state: "loading" } | { state: "ok"; value: unknown } | { state: "error"; message: string } | { state: "off" }
  >({ state: jsonViewable ? "loading" : "off" });

  async function bodyText(): Promise<string> {
    if (!response.truncated && result.size <= PREVIEW_BYTES && response.body.encoding === "utf8") {
      return response.body.data;
    }
    return new TextDecoder().decode(await backend.responseBytes(id, 0, result.size));
  }

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  if (prettyViewable) {
    bodyText()
      .then((text) => {
        // Token-level: numbers and strings stay as received (no parse/stringify round trip).
        pretty = { state: "ok", ...prettyBody(text, lang) };
        if (!jsonViewable) return;
        try {
          // `$state.raw`: the parsed value is never mutated, so it stays out of proxies.
          json = { state: "ok", value: JSON.parse(text) };
        } catch (e) {
          json = { state: "error", message: message(e) };
          if (mode === "tree") mode = "pretty";
        }
      })
      .catch((e: unknown) => {
        pretty = { state: "error", message: message(e) };
        json = { state: "error", message: message(e) };
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

  const LANGUAGE_NAMES: Record<BodyLanguage, string> = {
    json: "JSON",
    xml: "XML",
    html: "HTML",
    javascript: "JavaScript",
    css: "CSS",
    text: "text",
  };
  // svelte-ignore state_referenced_locally
  const prettyUnavailable = !result.is_text
    ? "The body is binary"
    : lang === "text"
      ? "No pretty view for this content type"
      : tooLarge
        ? `Pretty view and highlighting are available up to ${formatSize(PRETTY_LIMIT)}`
        : null;
  const prettyError = $derived(pretty.state === "error" ? `The body could not be loaded: ${pretty.message}` : null);
  const treeUnavailable = $derived(
    !jsonCandidate
      ? "The body is not JSON"
      : tooLarge
        ? `The tree is available for JSON up to ${formatSize(PRETTY_LIMIT)}`
        : json.state === "error"
          ? `The body is not valid JSON: ${json.message}`
          : null,
  );
  /** Pretty view and highlighting would be there, only the body is too large (shown above the raw lines). */
  const largeHint = lang !== "text" && tooLarge ? prettyUnavailable : null;

  // ---- JSONPath filter (in the backend, so it works for bodies of any size it accepts) ----

  let expression = $state("");
  let filtered = $state.raw<
    { expression: string; text: string; source: null } | { expression: string; text: null; source: LineSource } | null
  >(null);
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
      // JSONPath results are JSON: highlighted like the pretty view while they are small enough.
      filtered =
        text.length <= PRETTY_LIMIT
          ? { expression: expr, text: prettyBody(text, "json").text, source: null }
          : { expression: expr, text: null, source: arraySource(text.split("\n")) };
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
  const copyable = result.is_text && result.size <= PRETTY_LIMIT;

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
    if (m === "pretty") return prettyUnavailable ?? prettyError;
    if (m === "tree") return treeUnavailable;
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
    {#if path}<SpecBadge {backend} {path} {result} />{/if}
    <ExplainButton {result} {backend} />
  </div>

  <SendNotes notes={result.report.notes ?? []} />

  <div class="bar">
    <div class="tabs" role="tablist" aria-label="Response parts">
      {#each builtInTabs as [t, label] (t)}
        <button
          type="button"
          role="tab"
          aria-selected={tab === t}
          data-testid="response-tab-{t}"
          class:failed={t === "tests" && tests.passed < tests.total}
          class:passed={t === "tests" && tests.total > 0 && tests.passed === tests.total}
          onclick={() => (tab = t)}>{label}</button
        >
      {/each}
      {#each pluginTabs as t (t.key)}
        <button
          type="button"
          role="tab"
          aria-selected={tab === `plugin:${t.key}`}
          title="Plugin {t.pluginName}"
          data-testid="response-tab-plugin-{t.key}"
          onclick={() => (tab = `plugin:${t.key}`)}>{t.tab.title}</button
        >
      {/each}
      {#each extensionTabs as t (t.key)}
        <button
          type="button"
          role="tab"
          aria-selected={tab === `extension:${t.key}`}
          data-testid="response-tab-extension-{t.key}"
          onclick={() => (tab = `extension:${t.key}`)}>{t.tab.title}</button
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
    {#if pluginTab}
      <PluginTab {backend} plugin={pluginTab.plugin} tab={pluginTab.tab.id} target={{ request_path: result.report.request_path, response_id: id }} />
    {:else if tab.startsWith("plugin:")}
      <p class="note">This plugin tab is no longer available.</p>
    {:else if extensionTab && extensions}
      {@const Tab = extensionTab.tab.component}
      <Tab ctx={extensions.ctx} requestPath={result.report.request_path} response={result} {settle} />
    {:else if tab.startsWith("extension:")}
      <p class="note">This tab is no longer available.</p>
    {:else if tab === "headers"}
      <HeadersTable response={response.headers} request={result.report.request?.headers ?? null} />
    {:else if tab === "timing"}
      <TimingWaterfall data={timing} />
    {:else if tab === "tests"}
      <TestsPanel report={result.report} />
    {:else if tab === "console"}
      <ConsolePanel {logs} />
    {:else if filtered}
      <div class="fill" data-testid="response-filter-result">
        {#key filtered}
          {#if filtered.text !== null}
            <CodeView text={filtered.text} lang="json" label="JSONPath result" testid="response-filter-code" />
          {:else}
            <VirtualLines source={filtered.source} label="JSONPath result" />
          {/if}
        {/key}
      </div>
    {:else if mode === "pretty"}
      {#if pretty.state === "ok"}
        {#if !pretty.formatted && (lang === "json" || lang === "xml")}
          <p class="hint" data-testid="response-pretty-fallback">
            Shown as received: the body is not well-formed {LANGUAGE_NAMES[lang]}.
          </p>
        {/if}
        <CodeView text={pretty.text} {lang} label="Response body (pretty)" testid="response-pretty" />
      {:else}
        <p class="note">Loading…</p>
      {/if}
    {:else if mode === "tree"}
      {#if json.state === "ok"}
        <div class="tree" data-testid="response-tree">
          <ul><JsonNode name={null} value={json.value} path="$" expanded {onpick} /></ul>
        </div>
      {:else}
        <p class="note">Loading…</p>
      {/if}
    {:else if mode === "raw"}
      {#if result.line_count === 0}
        <p class="note">Empty body</p>
      {:else}
        {#if largeHint}<p class="hint" data-testid="response-pretty-hint">{largeHint}.</p>{/if}
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
  button.failed {
    color: var(--rt-danger);
  }
  button.passed {
    color: var(--rt-ok);
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
  .hint {
    margin: 0;
    padding: 2px var(--rt-gap);
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    border-bottom: 1px solid var(--rt-border);
  }
</style>
