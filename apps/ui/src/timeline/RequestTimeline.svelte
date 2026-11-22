<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- History of one request (wave 12, strand D): every version of its file over the commits, also
     across renames, what each commit changed field by field, who last changed each field
     (blame), and the test results recorded under each commit. An old version can be looked at,
     sent, or restored; both go through the editor (`app.edit`), so nothing new is written
     until the user saves. `path` is the request's path today. -->
<script lang="ts">
  import { fieldLabel, isMultiline, valueText } from "../diff/format";
  import { ago } from "../git/format";
  import type { DiffValue, FieldBlame, RequestSpec, RequestTimeline, TestRecord, TimelineEntry } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { headline, restoreNote, shortOid, testSummary } from "./format";

  let { path }: { path: string } = $props();
  const app = getApp();
  const api = app.backend.timeline;

  /** Versions read for the history; the blame always reads the whole history it can. */
  const LIMIT = 100;

  let mode = $state<"history" | "blame">("history");
  let timeline = $state<RequestTimeline | null>(null);
  let blame = $state<FieldBlame[] | null>(null);
  let error = $state<string | null>(null);
  /** An old version shown in full. */
  let shown = $state<{ entry: TimelineEntry; spec: RequestSpec } | null>(null);
  let notice = $state<string | null>(null);
  let busy = $state(false);
  /** Waiting for the user to agree to replace unsaved edits of this request. */
  let confirming = $state<{ entry: TimelineEntry; action: "send" | "restore" } | null>(null);

  const describe = (e: unknown) => (e instanceof Error ? e.message : String(e));

  let generation = 0;
  async function load(): Promise<void> {
    const mine = ++generation;
    error = null;
    try {
      const [t, b] = await Promise.all([api.history(path, LIMIT), api.blame(path)]);
      if (mine !== generation) return;
      timeline = t;
      blame = b;
    } catch (e) {
      if (mine === generation) error = describe(e);
    }
  }

  $effect(() => {
    void path;
    void app.loadCount;
    timeline = null;
    blame = null;
    shown = null;
    notice = null;
    confirming = null;
    void load();
  });

  async function view(entry: TimelineEntry): Promise<void> {
    notice = null;
    try {
      shown = { entry, spec: await api.version(entry.commit.commit.oid, entry.commit.path) };
    } catch (e) {
      error = describe(e);
    }
  }

  /** Puts the version into the editor, then sends it or saves it. */
  async function use(entry: TimelineEntry, action: "send" | "restore", agreed = false): Promise<void> {
    if (busy) return;
    if (!agreed && app.dirty.includes(path)) {
      confirming = { entry, action };
      return;
    }
    confirming = null;
    busy = true;
    notice = null;
    try {
      const spec = await api.version(entry.commit.commit.oid, entry.commit.path);
      await app.select(path);
      await app.edit(path, spec);
      const label = `${shortOid(entry.commit.commit.oid)} "${entry.commit.commit.summary}"`;
      if (action === "send") {
        await app.send(path);
        const status = app.results[path]?.report.response?.status;
        notice = `Sent version ${label}${status ? ` (HTTP ${status})` : ""}. It is in the editor, unsaved.`;
      } else if (app.dirty.every((p) => p === path)) {
        await app.save();
        notice = `Restored version ${label} and saved it. Commit it to keep it in the history.`;
        await load();
      } else {
        notice = restoreNote(label);
      }
    } catch (e) {
      error = describe(e);
    } finally {
      busy = false;
    }
  }

  const latest = (entry: TimelineEntry): TestRecord | undefined => entry.tests[0];
  const text = (v: DiffValue | null) => valueText(v) ?? "";
</script>

<div class="view" data-testid="request-timeline" data-path={path}>
  <div class="bar">
    <span class="title" data-testid="tl-title">{path}</span>
    {#if timeline}<span class="summary" data-testid="tl-summary">{headline(timeline.entries.length)}</span>{/if}
    <div class="toggle" role="group" aria-label="Show">
      <button class="button" data-testid="tl-mode-history" aria-pressed={mode === "history"} onclick={() => (mode = "history")}>History</button>
      <button class="button" data-testid="tl-mode-blame" aria-pressed={mode === "blame"} onclick={() => (mode = "blame")}>Blame</button>
    </div>
  </div>

  {#if notice}<p class="notice" role="status" data-testid="tl-notice">{notice}</p>{/if}

  {#if error}
    <p class="error" role="alert" data-testid="tl-error">{error}</p>
  {:else if !timeline || !blame}
    <p class="hint">Reading the history…</p>
  {:else if timeline.entries.length === 0}
    <p class="hint" data-testid="tl-empty">No commit has touched this request yet. Commit it to start its history.</p>
  {:else if mode === "blame"}
    <table class="blame" data-testid="tl-blame">
      <thead><tr><th>Field</th><th>Value</th><th>Last changed</th></tr></thead>
      <tbody>
        {#each blame as b (b.field.join("\u0000"))}
          <tr data-testid="tl-blame-row" data-path={b.field.join("/")}>
            <td class="path" data-testid="tl-blame-field">{fieldLabel(b.field)}</td>
            <td class="value">
              {#if b.value?.kind === "secret"}<span class="secret">secret value hidden</span>{:else}{text(b.value) || "(empty)"}{/if}
            </td>
            <td class="who" data-testid="tl-blame-commit" data-oid={b.commit.oid}>
              <span>{b.commit.summary}</span>
              <span class="meta">{shortOid(b.commit.oid)} · {b.commit.author} · {ago(b.commit.time)}</span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <ol class="entries">
      {#each timeline.entries as entry, i (entry.commit.commit.oid)}
        {@const record = latest(entry)}
        <li class="entry" data-testid="tl-entry" data-oid={entry.commit.commit.oid} data-change={entry.commit.change}>
          <div class="head">
            <span class="badge {entry.commit.change}" data-testid="tl-change">{entry.commit.change}</span>
            <span class="commit" data-testid="tl-commit">{entry.commit.commit.summary}</span>
            {#if i === 0}<span class="current">latest</span>{/if}
            <span class="meta">{shortOid(entry.commit.commit.oid)} · {entry.commit.commit.author} · {ago(entry.commit.commit.time)}</span>
          </div>
          {#if entry.commit.from}
            <div class="rename" data-testid="tl-rename">moved from <code>{entry.commit.from}</code> to <code>{entry.commit.path}</code></div>
          {/if}
          {#if record}
            <details class="tests" data-testid="tl-tests" data-passed={record.passed}>
              <summary>
                <span class="result {record.passed ? 'ok' : 'bad'}" data-testid="tl-result">{record.passed ? "Passed" : "Failed"}</span>
                <span class="meta">{testSummary(record)}{entry.tests.length > 1 ? ` · ${entry.tests.length} runs recorded` : ""}</span>
              </summary>
              <ul>
                {#each entry.tests as r, n (n)}
                  <li data-testid="tl-run">
                    <span class="result {r.passed ? 'ok' : 'bad'}">{r.passed ? "Passed" : "Failed"}</span>
                    <span class="meta">{ago(r.time)}{r.environment ? ` · ${r.environment}` : ""}{r.status ? ` · HTTP ${r.status}` : ""} · {r.source}</span>
                    <ul class="names">
                      {#each r.tests as t, k (k)}
                        <li class={t.passed ? "ok" : "bad"}>{t.passed ? "pass" : "fail"}: {t.name}</li>
                      {/each}
                    </ul>
                  </li>
                {/each}
              </ul>
            </details>
          {/if}
          {#if entry.fields.length > 0}
            <ul class="fields">
              {#each entry.fields as f (f.path.join("\u0000"))}
                <li data-testid="tl-field" data-path={f.path.join("/")}>
                  <span class="path">{fieldLabel(f.path)}</span>
                  {#if f.old?.kind === "secret" || f.new?.kind === "secret"}
                    <span class="secret">secret value hidden</span>
                  {:else if isMultiline(f.old, f.new)}
                    <span class="note">{f.old && f.new ? "changed" : f.new ? "added" : "removed"} (multi-line)</span>
                  {:else}
                    {#if f.old}<del class="old">{text(f.old) || "(empty)"}</del>{/if}
                    {#if f.old && f.new}<span class="arrow" aria-hidden="true">→</span>{/if}
                    {#if f.new}<ins class="new">{text(f.new) || "(empty)"}</ins>{/if}
                  {/if}
                </li>
              {/each}
            </ul>
          {:else if entry.commit.change !== "deleted"}
            <p class="note">No field changed.</p>
          {/if}
          {#if entry.commit.change !== "deleted"}
            {#if confirming?.entry === entry}
              <div class="confirm" role="alertdialog" aria-label="Replace unsaved edits" data-testid="tl-confirm">
                <span>This request has unsaved edits. Replace them with this version?</span>
                <button class="button" data-testid="tl-confirm-yes" onclick={() => use(entry, confirming!.action, true)}>Replace</button>
                <button class="button" data-testid="tl-confirm-no" onclick={() => (confirming = null)}>Cancel</button>
              </div>
            {:else}
              <div class="actions">
                <button class="button" data-testid="tl-view" disabled={busy} onclick={() => view(entry)}>View</button>
                <button class="button" data-testid="tl-send" disabled={busy} onclick={() => use(entry, "send")}>Send this version</button>
                <button class="button" data-testid="tl-restore" disabled={busy} onclick={() => use(entry, "restore")}>Restore</button>
              </div>
            {/if}
          {/if}
          {#if shown?.entry === entry}
            <div class="version" data-testid="tl-version">
              <div class="request-line"><strong>{shown.spec.method}</strong> <span data-testid="tl-version-url">{shown.spec.url}</span></div>
              {#each shown.spec.headers as h (h.name + h.value)}
                <div class="header"><span class="path">{h.name}</span> {h.enabled ? "" : "(off) "}{h.value}</div>
              {/each}
              {#if shown.spec.body.type !== "none"}<div class="note">Body: {shown.spec.body.type}</div>{/if}
              <button class="button" data-testid="tl-version-close" onclick={() => (shown = null)}>Hide</button>
            </div>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    border-bottom: 1px solid var(--rt-border);
  }
  .title {
    font-weight: 600;
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .summary,
  .meta,
  .note,
  .hint {
    color: var(--rt-fg-muted);
  }
  .meta,
  .note {
    font-size: var(--rt-size-small);
  }
  .toggle {
    display: flex;
    gap: 2px;
    margin-left: auto;
  }
  .toggle button[aria-pressed="true"] {
    background: var(--rt-bg-selected);
    font-weight: 600;
  }
  .hint,
  .error,
  .notice {
    margin: 0;
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
  }
  .notice {
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
  }
  .error {
    color: var(--rt-danger);
  }
  .entries {
    list-style: none;
    margin: 0;
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .entry {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    overflow: hidden;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) / 2) var(--rt-gap);
    background: var(--rt-bg-panel);
  }
  .commit {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .current {
    font-size: var(--rt-size-small);
    color: var(--rt-accent);
  }
  .badge {
    padding: 0 6px;
    border-radius: var(--rt-radius);
    font-size: var(--rt-size-small);
    font-weight: 600;
    text-transform: capitalize;
    border: 1px solid currentcolor;
  }
  .badge.added {
    color: var(--rt-ok);
  }
  .badge.deleted {
    color: var(--rt-danger);
  }
  .badge.modified,
  .badge.renamed {
    color: var(--rt-warn);
  }
  .rename,
  .tests,
  .fields,
  .actions,
  .confirm,
  .version,
  .entry > .note {
    padding: calc(var(--rt-gap) / 2) var(--rt-gap);
    border-top: 1px solid var(--rt-border);
  }
  .entry > .note {
    margin: 0;
  }
  code,
  .path,
  .value,
  .header,
  .request-line {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    overflow-wrap: anywhere;
  }
  .path {
    font-weight: 600;
  }
  .fields {
    list-style: none;
    margin: 0;
  }
  .fields li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-gap);
    font-size: var(--rt-size-small);
  }
  del,
  ins {
    text-decoration: none;
    padding: 0 4px;
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  del {
    color: var(--rt-danger);
  }
  ins {
    color: var(--rt-ok);
  }
  .secret {
    color: var(--rt-var-secret);
    font-style: italic;
  }
  .result {
    font-size: var(--rt-size-small);
    font-weight: 600;
    padding: 0 6px;
    border-radius: var(--rt-radius);
    border: 1px solid currentcolor;
  }
  .result.ok,
  .names .ok {
    color: var(--rt-ok);
  }
  .result.bad,
  .names .bad {
    color: var(--rt-danger);
  }
  .tests summary {
    cursor: pointer;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-gap);
  }
  .tests ul {
    list-style: none;
    margin: calc(var(--rt-gap) / 2) 0 0;
    padding: 0;
  }
  .names {
    padding-left: var(--rt-gap);
    font-size: var(--rt-size-small);
  }
  .actions,
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-gap);
  }
  .version {
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-items: flex-start;
  }
  .blame {
    width: 100%;
    border-collapse: collapse;
  }
  .blame th,
  .blame td {
    text-align: left;
    padding: calc(var(--rt-gap) / 2) calc(var(--rt-gap) * 2);
    border-bottom: 1px solid var(--rt-border);
    vertical-align: top;
  }
  .blame th {
    color: var(--rt-fg-muted);
    font-weight: 600;
    font-size: var(--rt-size-small);
  }
  .who {
    display: flex;
    flex-direction: column;
  }
</style>
