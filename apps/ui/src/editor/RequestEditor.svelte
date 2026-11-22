<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Request editor for one request: method and URL, params, headers, body, auth, variables,
     scripts and assertions (wave 7, strand C). Owner: wave 3, strand B.
     Reads the spec from `app.drafts[path]`; every change goes through `app.edit` (typing is
     debounced and flushed before anything else can use the backend's state). Variable
     highlighting comes from `backend.preview`; the editor never resolves a variable itself.
     Wave 8 (strand A): WebSocket and gRPC requests have "Messages" instead of "Body" (gRPC:
     the method picker above it) and gRPC calls its headers "Metadata"; Enter in the URL
     connects a session request instead of sending it. -->
<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from "svelte";

  import type { Auth, Body, Param, Preview, RequestSpec, Variable } from "../commands/types";
  import { tabsFor } from "../plugins/permissions";
  import PluginTab from "../plugins/PluginTab.svelte";
  import { pluginsState } from "../plugins/store.svelte";
  import AssertionsTab from "../scripts/AssertionsTab.svelte";
  import { countScripts } from "../scripts/edit";
  import ScriptsTab from "../scripts/ScriptsTab.svelte";
  import { sessionVars } from "../scripts/session.svelte";
  import { getApp } from "../state/app.svelte";
  import GrpcMethodPicker from "../grpc/GrpcMethodPicker.svelte";
  import { sessionKind } from "../stream/kind";
  import MessageEditor from "../stream/MessageEditor.svelte";
  import { getSessions } from "../stream/sessions.svelte";
  import AuthEditor from "./AuthEditor.svelte";
  import BodyEditor from "./BodyEditor.svelte";
  import CodeField from "./CodeField.svelte";
  import { canFormat, describeFormat, formatBody, onFormatRequest } from "./format-body";
  import KeyValueTable, { type Row } from "./KeyValueTable.svelte";
  import { paramsFromUrl, urlFromParams } from "./url";
  import { lookupFromPreview } from "./vars";

  /** `actions`: rendered at the end of the URL row (the shell puts the Send button there). */
  let { path, actions }: { path: string; actions?: Snippet } = $props();
  const app = getApp();

  /** Typing is sent to the backend after this pause. */
  const TYPING_MS = 250;
  /** The preview is refreshed after this pause once an edit reached the backend. */
  const PREVIEW_MS = 120;
  const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

  // ---- edits ----

  /** An edit not yet handed to `app.edit`. */
  let pending = $state.raw<{ path: string; spec: RequestSpec } | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Bumped when an edit reached the backend, to refresh the preview. */
  let revision = $state(0);
  let section: HTMLElement;

  const spec = $derived<RequestSpec | undefined>(pending?.path === path ? pending.spec : app.drafts[path]);

  function flush() {
    clearTimeout(timer);
    timer = undefined;
    const p = pending;
    if (!p) return;
    pending = null;
    // `app.edit` sets `drafts` synchronously, so the editor never shows an older state.
    inflight = app.edit(p.path, p.spec).then(() => revision++);
  }
  /** The last edit handed to the backend (plugin tabs wait for it). */
  let inflight: Promise<unknown> = Promise.resolve();

  /** Applies `change` to a copy of the current spec. Typing is debounced, the rest is not. */
  function update(change: (s: RequestSpec) => void, typing = false) {
    if (!spec) return;
    if (pending && pending.path !== path) flush();
    const next = $state.snapshot(spec) as RequestSpec;
    change(next);
    pending = { path, spec: next };
    clearTimeout(timer);
    if (typing) timer = setTimeout(flush, TYPING_MS);
    else flush();
  }

  // Flush before the user does anything else with this request: send, save, switch tabs,
  // keyboard shortcuts, leaving the editor.
  onMount(() => {
    const onPointer = (e: PointerEvent) => {
      if (pending && !section.contains(e.target as Node)) flush();
    };
    const onKey = (e: KeyboardEvent) => {
      if (pending && (e.metaKey || e.ctrlKey || e.altKey)) flush();
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
      flush();
    };
  });
  $effect(() => {
    void path;
    return () => flush();
  });

  // ---- kept data while switching body/auth types (until saved) ----

  const stashes = new Map<string, Map<string, unknown>>();
  const stash = $derived.by(() => {
    let s = stashes.get(path);
    if (!s) stashes.set(path, (s = new Map()));
    return s;
  });
  $effect(() => {
    const dirty = app.dirty;
    for (const p of [...stashes.keys()]) if (!dirty.includes(p) && !untrack(() => pending)) stashes.get(p)!.clear();
  });

  // ---- preview (variable provenance) ----

  let preview = $state.raw<{ path: string; value: Preview } | null>(null);
  let previewSeq = 0;
  // Session variables (wave 7) are part of the preview: re-read when they change.
  const session = sessionVars(app);
  $effect(() => {
    const p = path;
    const input = { environment: app.environment, vars: [] };
    void revision;
    void app.environments;
    void app.sendCount;
    void session.revision;
    if (!untrack(() => app.drafts[p])) return;
    const t = setTimeout(async () => {
      const seq = ++previewSeq;
      try {
        const value = await app.backend.preview(p, input);
        if (seq === previewSeq) preview = { path: p, value };
      } catch {
        if (seq === previewSeq) preview = null;
      }
    }, PREVIEW_MS);
    return () => clearTimeout(t);
  });
  const shown = $derived(preview?.path === path ? preview.value : null);
  const lookup = $derived(lookupFromPreview(shown));

  // ---- method and URL ----

  /** Path whose method is being typed freely. */
  let customFor = $state<string | null>(null);
  const isCustom = $derived(customFor === path || (spec !== undefined && !METHODS.includes(spec.method)));

  function setUrl(url: string) {
    update((s) => {
      s.url = url;
      s.params = paramsFromUrl(url, s.params);
    }, true);
  }

  // ---- params ----

  const indexes = (params: Param[], kind: Param["kind"]) =>
    params.flatMap((p, i) => (p.kind === kind ? [i] : []));
  const queryIdx = $derived(spec ? indexes(spec.params, "query") : []);
  const pathIdx = $derived(spec ? indexes(spec.params, "path") : []);

  function setParam(at: number, patch: Partial<Row>) {
    update((s) => {
      s.params[at] = { ...s.params[at], ...patch };
      if (s.params[at].kind === "query") s.url = urlFromParams(s.url, s.params);
    }, patch.enabled === undefined);
  }
  function addQuery() {
    update((s) => {
      const last = indexes(s.params, "query").at(-1);
      const firstPath = indexes(s.params, "path")[0];
      const at = last !== undefined ? last + 1 : (firstPath ?? s.params.length);
      s.params.splice(at, 0, { name: "", value: "", kind: "query", enabled: true });
    });
  }
  function removeParam(at: number) {
    update((s) => {
      s.params.splice(at, 1);
      s.url = urlFromParams(s.url, s.params);
    });
  }

  // ---- request variables ----

  const varRows = $derived<Row[]>(
    (spec?.vars ?? []).map((v) => ({
      name: v.name,
      value: v.value ?? "",
      enabled: v.enabled,
      lockedValue: v.secret ? "secret" : undefined,
    })),
  );
  function setVar(i: number, patch: Partial<Row>) {
    update((s) => {
      const v: Variable = s.vars[i];
      if (patch.name !== undefined) v.name = patch.name;
      if (patch.value !== undefined && !v.secret) v.value = patch.value;
      if (patch.enabled !== undefined) v.enabled = patch.enabled;
    }, patch.enabled === undefined);
  }

  // ---- tabs ----

  /** Built-in tabs, or `plugin:<plugin>/<tab>` (wave 5). */
  type Tab = "params" | "headers" | "body" | "messages" | "auth" | "vars" | "scripts" | "assertions" | `plugin:${string}`;
  let chosen = $state<Tab>("params");
  // Wave 8: session requests have messages, not a body.
  const protocol = $derived(spec?.protocol?.type === "websocket" || spec?.protocol?.type === "grpc" ? spec.protocol : null);
  const tab = $derived<Tab>(protocol && chosen === "body" ? "messages" : !protocol && chosen === "messages" ? "body" : chosen);
  const plugins = pluginsState(app.backend);
  const pluginTabs = $derived(tabsFor(plugins.list, "request"));
  const pluginTab = $derived(tab.startsWith("plugin:") ? (pluginTabs.find((t) => `plugin:${t.key}` === tab) ?? null) : null);
  const tabs = $derived<[Tab, string, string][]>(
    spec
      ? [
          ["params", "Params", count(spec.params.filter((p) => p.enabled).length)],
          ["headers", protocol?.type === "grpc" ? "Metadata" : "Headers", count(spec.headers.filter((h) => h.enabled).length)],
          protocol
            ? ["messages", "Messages", count(protocol.messages.length)]
            : ["body", "Body", spec.body.type === "none" ? "" : spec.body.type === "raw" ? spec.body.kind : spec.body.type],
          ["auth", "Auth", spec.auth.type === "inherit" ? "" : spec.auth.type === "unsupported" ? spec.auth.kind : spec.auth.type],
          ["vars", "Vars", count(spec.vars.length)],
          ["scripts", "Scripts", count(countScripts(spec.scripts))],
          ["assertions", "Assertions", count(spec.assertions.filter((a) => a.enabled).length + spec.actions.filter((a) => a.enabled).length)],
        ]
      : [],
  );
  function count(n: number) {
    return n === 0 ? "" : String(n);
  }

  const sessions = getSessions();
  function sendNow() {
    flush();
    if (sessions && sessionKind(spec)) void sessions.primary(path);
    else void app.send(path);
  }

  // ---- format body (wave 8): button, Shift+Alt+F, palette ----

  let formatStatus = $state<string | null>(null);
  let formatTimer: ReturnType<typeof setTimeout> | undefined;
  async function formatNow() {
    if (!spec || !canFormat(spec.body)) return;
    // Show the body first, so the change goes through its editor (and its undo history).
    if (tab !== "body") {
      tab = "body";
      await tick();
    }
    const result = spec && formatBody(spec.body);
    if (!result) return;
    if (result.kind === "formatted") update((s) => (s.body = result.body));
    formatStatus = describeFormat(result);
    clearTimeout(formatTimer);
    formatTimer = setTimeout(() => (formatStatus = null), 3000);
  }
  onMount(() => onFormatRequest({ available: () => spec !== undefined && canFormat(spec.body), format: () => void formatNow() }));
  $effect(() => () => clearTimeout(formatTimer));
</script>

<section data-testid="request-editor" aria-label="Request editor" bind:this={section}>
  {#if spec}
    {#key path}
      <div class="urlbar">
        {#if protocol}
          <span class="method protocol" data-testid="method-protocol" title={protocol.type === "grpc" ? "gRPC" : "WebSocket"}>{spec.method}</span>
        {:else}
        <select
          class="method"
          data-testid="method-select"
          aria-label="Method"
          value={isCustom ? "custom" : spec.method}
          onchange={(e) => {
            const v = e.currentTarget.value;
            customFor = v === "custom" ? path : null;
            if (v !== "custom") update((s) => (s.method = v));
          }}
        >
          {#each METHODS as m (m)}<option value={m}>{m}</option>{/each}
          <option value="custom">{isCustom ? spec.method || "Custom" : "Custom…"}</option>
        </select>
        {/if}
        {#if isCustom && !protocol}
          <input
            class="custom"
            data-testid="method-custom"
            aria-label="Custom method"
            placeholder="METHOD"
            value={spec.method}
            oninput={(e) => {
              const v = e.currentTarget.value;
              update((s) => (s.method = v), true);
            }}
          />
        {/if}
        <CodeField
          testid="url-input"
          label="URL"
          placeholder={"{{baseUrl}}/path"}
          value={spec.url}
          {lookup}
          onchange={setUrl}
          onenter={sendNow}
        />
        {#if actions}
          <!-- Actions (Send) act on the request: pending typing reaches the backend first,
               like a click outside the editor. -->
          <div
            class="actions"
            role="presentation"
            onpointerdowncapture={() => pending && flush()}
            onkeydowncapture={() => pending && flush()}
          >
            {@render actions()}
          </div>
        {/if}
      </div>
      {#if shown?.error}
        <p class="resolved error" data-testid="url-preview-error">{shown.error}</p>
      {:else if shown?.url}
        <p class="resolved" data-testid="url-preview" title="URL as it would be sent (secrets masked)">→ {shown.url}</p>
      {/if}

      <div class="tabs" role="tablist" aria-label="Request parts">
        {#each tabs as [id, text, badge] (id)}
          <button
            role="tab"
            id={`rt-tab-${id}`}
            data-testid={`tab-${id}`}
            aria-selected={tab === id}
            aria-controls="rt-request-panel"
            onclick={() => (chosen = id)}
          >
            {text}{#if badge}<span class="badge">{badge}</span>{/if}
          </button>
        {/each}
        {#each pluginTabs as t (t.key)}
          <button
            role="tab"
            id={`rt-tab-plugin-${t.key}`}
            data-testid={`tab-plugin-${t.key}`}
            title="Plugin {t.pluginName}"
            aria-selected={tab === `plugin:${t.key}`}
            aria-controls="rt-request-panel"
            onclick={() => (chosen = `plugin:${t.key}`)}
          >
            {t.tab.title}
          </button>
        {/each}
      </div>

      <div class="panel" id="rt-request-panel" role="tabpanel" aria-labelledby={`rt-tab-${tab}`}>
        {#if pluginTab}
          <PluginTab
            backend={app.backend}
            plugin={pluginTab.plugin}
            tab={pluginTab.tab.id}
            target={{ request_path: path, response_id: null }}
            before={async () => {
              flush();
              await inflight;
            }}
            onupdate={(next) => app.edit(path, next)}
          />
        {:else if tab.startsWith("plugin:")}
          <p class="hint">This plugin tab is no longer available.</p>
        {:else if tab === "params"}
          <h3>Query</h3>
          <KeyValueTable
            testid="params-table"
            label="Query param"
            rows={queryIdx.map((i) => spec.params[i])}
            {lookup}
            onchange={(j, patch) => setParam(queryIdx[j], patch)}
            onadd={addQuery}
            onremove={(j) => removeParam(queryIdx[j])}
          />
          {#if pathIdx.length > 0}
            <h3>Path</h3>
            <KeyValueTable
              testid="path-params-table"
              label="Path param"
              fixedNames
              rows={pathIdx.map((i) => spec.params[i])}
              {lookup}
              onchange={(j, patch) => setParam(pathIdx[j], patch)}
            />
          {/if}
          <p class="hint">The URL is the truth: query rows mirror its query string. Path params fill <code>:name</code> segments.</p>
        {:else if tab === "headers"}
          <KeyValueTable
            testid="headers-table"
            label="Header"
            rows={spec.headers}
            {lookup}
            onchange={(i, patch) => update((s) => (s.headers[i] = { ...s.headers[i], ...patch }), patch.enabled === undefined)}
            onadd={() => update((s) => s.headers.push({ name: "", value: "", enabled: true }))}
            onremove={(i) => update((s) => s.headers.splice(i, 1))}
          />
        {:else if tab === "body"}
          <BodyEditor
            body={spec.body}
            {lookup}
            {stash}
            onchange={(body: Body, typing: boolean) => update((s) => (s.body = body), typing)}
            onformat={formatNow}
            {formatStatus}
          />
        {:else if tab === "messages" && protocol}
          {#if protocol.type === "grpc"}
            <GrpcMethodPicker
              {path}
              spec={protocol}
              onchange={(grpc) => update((s) => (s.protocol = { ...grpc, type: "grpc" }))}
            />
          {/if}
          <MessageEditor
            messages={protocol.messages}
            {lookup}
            kinds={protocol.type === "grpc" ? ["json"] : undefined}
            onchange={(messages, typing) =>
              update((s) => {
                if (s.protocol && s.protocol.type !== "http") s.protocol.messages = messages;
              }, typing)}
          />
        {:else if tab === "auth"}
          <AuthEditor
            auth={spec.auth}
            {lookup}
            {stash}
            onchange={(auth: Auth, typing: boolean) => update((s) => (s.auth = auth), typing)}
          />
        {:else if tab === "vars"}
          <KeyValueTable
            testid="vars-table"
            label="Request variable"
            rows={varRows}
            {lookup}
            onchange={setVar}
            onadd={() => update((s) => s.vars.push({ name: "", value: "", enabled: true, secret: false }))}
            onremove={(i) => update((s) => s.vars.splice(i, 1))}
          />
          <p class="hint">Request variables override collection, environment and folder variables for this request.</p>
        {:else if tab === "scripts"}
          <ScriptsTab scripts={spec.scripts} onchange={(scripts) => update((s) => (s.scripts = scripts), true)} />
        {:else if tab === "assertions"}
          <AssertionsTab
            assertions={spec.assertions}
            actions={spec.actions}
            onassertions={(next, typing) => update((s) => (s.assertions = next), typing)}
            onactions={(next, typing) => update((s) => (s.actions = next), typing)}
          />
        {/if}
      </div>
    {/key}
  {/if}
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    min-width: 0;
  }
  .urlbar {
    display: flex;
    gap: 6px;
    align-items: stretch;
  }
  .actions {
    display: contents;
  }
  select,
  input,
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .method {
    font-weight: 600;
    padding: 2px 4px;
  }
  .protocol {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
  }
  .custom {
    width: 8em;
    padding: 2px 6px;
    font-family: var(--rt-font-mono);
    text-transform: uppercase;
  }
  .resolved {
    margin: 0;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .resolved.error {
    color: var(--rt-danger);
  }
  .tabs {
    display: flex;
    gap: 2px;
    border-bottom: 1px solid var(--rt-border);
  }
  .tabs button {
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: none;
    padding: 4px 10px;
    cursor: pointer;
    color: var(--rt-fg-muted);
  }
  .tabs button:hover {
    color: var(--rt-fg);
    background: var(--rt-bg-hover);
  }
  .tabs button[aria-selected="true"] {
    color: var(--rt-fg);
    border-bottom-color: var(--rt-accent);
  }
  .badge {
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--rt-bg-selected);
    font-size: var(--rt-size-small);
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  h3 {
    margin: 0;
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
    text-transform: uppercase;
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
</style>
