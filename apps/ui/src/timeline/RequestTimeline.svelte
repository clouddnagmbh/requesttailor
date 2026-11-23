<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- History of one request (wave 12, strand D): every version of its file over the commits, also
     across renames, what each commit changed field by field, who last changed each field
     (blame), and the test results recorded under each commit. An old version can be looked at,
     sent, or restored; both go through the editor (`app.edit`), so nothing new is written
     until the user saves. `path` is the request's path today. -->
<script lang="ts">
  import { fieldLabel, fieldValueText, isMultiline } from "../diff/format";
  import { ago } from "../git/format";
  import type { DiffValue, FieldBlame, FieldPath, RequestSpec, RequestTimeline, TestRecord, TimelineEntry } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Avatar, Badge, Banner, Button, EmptyState, Icon, SegmentedControl, Skeleton } from "../ui";
  import { headline, restoreNote, shortOid, testSummary } from "./format";

  let { path }: { path: string } = $props();
  const app = getApp();
  const api = app.backend.timeline;

  /** Versions read for the history; the blame always reads the whole history it can. */
  const LIMIT = 100;

  let mode = $state("history");
  /** Git status letters of a file change, shown as the word by the status Badge. */
  const CHANGE_STATUS = { added: "A", deleted: "D", modified: "M", renamed: "R" } as const;
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
  const text = (v: DiffValue | null, path: FieldPath) => fieldValueText(path, v) ?? "";
</script>

<div class="view" data-testid="request-timeline" data-path={path}>
  <div class="bar">
    <div class="heading">
      <span class="title" data-testid="tl-title">{path}</span>
      {#if timeline}<span class="summary" data-testid="tl-summary">{headline(timeline.entries.length)}</span>{/if}
    </div>
    <SegmentedControl
      size="sm"
      label="Show"
      bind:value={mode}
      options={[
        { value: "history", label: "History", icon: "history", testid: "tl-mode-history" },
        { value: "blame", label: "Blame", icon: "user", testid: "tl-mode-blame" },
      ]}
    />
  </div>

  {#if notice}<div class="banner" data-testid="tl-notice"><Banner tone="ok">{notice}</Banner></div>{/if}

  {#if error}
    <div class="banner" data-testid="tl-error"><Banner tone="danger">{error}</Banner></div>
  {:else if !timeline || !blame}
    <div class="loading" aria-busy="true">
      <Skeleton height="64px" />
      <Skeleton height="64px" />
    </div>
  {:else if timeline.entries.length === 0}
    <div data-testid="tl-empty">
      <EmptyState icon="history" title="No history yet" text="No commit has touched this request yet. Commit it to start its history." />
    </div>
  {:else if mode === "blame"}
    <div class="blame-wrap">
      <table class="blame" data-testid="tl-blame">
        <thead><tr><th>Field</th><th>Value</th><th>Last changed</th></tr></thead>
        <tbody>
          {#each blame as b (b.field.join("\u0000"))}
            <tr data-testid="tl-blame-row" data-path={b.field.join("/")}>
              <td class="path" data-testid="tl-blame-field">{fieldLabel(b.field)}</td>
              <td class="value">
                {#if b.value?.kind === "secret"}<span class="secret"><Icon name="lock" size="sm" /> secret value hidden</span>{:else}{text(b.value, b.field) || "(empty)"}{/if}
              </td>
              <td class="who" data-testid="tl-blame-commit" data-oid={b.commit.oid}>
                <span class="summary-line">{b.commit.summary}</span>
                <span class="meta"><Avatar name={b.commit.author} size="sm" /> {b.commit.author} · <code>{shortOid(b.commit.oid)}</code> · {ago(b.commit.time)}</span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <ol class="entries">
      {#each timeline.entries as entry, i (entry.commit.commit.oid)}
        {@const record = latest(entry)}
        <li class="entry" data-testid="tl-entry" data-oid={entry.commit.commit.oid} data-change={entry.commit.change}>
          <span class="dot {entry.commit.change}" aria-hidden="true"></span>
          <div class="card">
            <div class="head">
              <span class="change" data-testid="tl-change"><Badge kind="status" long value={CHANGE_STATUS[entry.commit.change] ?? entry.commit.change} /></span>
              <span class="commit" data-testid="tl-commit">{entry.commit.commit.summary}</span>
              {#if i === 0}<Badge value="latest" tone="accent" />{/if}
            </div>
            <div class="byline">
              <Avatar name={entry.commit.commit.author} size="sm" />
              <span>{entry.commit.commit.author}</span>
              <span aria-hidden="true">·</span>
              <span>{ago(entry.commit.commit.time)}</span>
              <code class="oid">{shortOid(entry.commit.commit.oid)}</code>
            </div>
            {#if entry.commit.from}
              <div class="rename" data-testid="tl-rename">moved from <code>{entry.commit.from}</code> to <code>{entry.commit.path}</code></div>
            {/if}
            {#if record}
              <details class="tests" data-testid="tl-tests" data-passed={record.passed}>
                <summary>
                  <span data-testid="tl-result"><Badge value={record.passed ? "Passed" : "Failed"} tone={record.passed ? "ok" : "danger"} /></span>
                  <span class="meta">{testSummary(record)}{entry.tests.length > 1 ? ` · ${entry.tests.length} runs recorded` : ""}</span>
                </summary>
                <ul>
                  {#each entry.tests as r, n (n)}
                    <li data-testid="tl-run">
                      <Badge value={r.passed ? "Passed" : "Failed"} tone={r.passed ? "ok" : "danger"} />
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
                      <span class="secret"><Icon name="lock" size="sm" /> secret value hidden</span>
                    {:else if isMultiline(f.old, f.new)}
                      <span class="note">{f.old && f.new ? "changed" : f.new ? "added" : "removed"} (multi-line)</span>
                    {:else}
                      {#if f.old}<del class="old">{text(f.old, f.path) || "(empty)"}</del>{/if}
                      {#if f.old && f.new}<span class="arrow" aria-hidden="true"><Icon name="chevron-right" size="sm" /></span>{/if}
                      {#if f.new}<ins class="new">{text(f.new, f.path) || "(empty)"}</ins>{/if}
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
                  <Button size="sm" variant="danger" data-testid="tl-confirm-yes" onclick={() => use(entry, confirming!.action, true)}>Replace</Button>
                  <Button size="sm" variant="ghost" data-testid="tl-confirm-no" onclick={() => (confirming = null)}>Cancel</Button>
                </div>
              {:else}
                <div class="actions">
                  <Button size="sm" variant="ghost" icon="eye" data-testid="tl-view" disabled={busy} onclick={() => view(entry)}>View</Button>
                  <Button size="sm" variant="secondary" icon="send" data-testid="tl-send" disabled={busy} onclick={() => use(entry, "send")}>Send this version</Button>
                  <Button size="sm" variant="secondary" icon="undo" data-testid="tl-restore" disabled={busy} onclick={() => use(entry, "restore")}>Restore</Button>
                </div>
              {/if}
            {/if}
            {#if shown?.entry === entry}
              <div class="version" data-testid="tl-version">
                <div class="request-line"><Badge kind="method" value={shown.spec.method} /> <span data-testid="tl-version-url">{shown.spec.url}</span></div>
                {#each shown.spec.headers as h (h.name + h.value)}
                  <div class="header"><span class="path">{h.name}</span> {h.enabled ? "" : "(off) "}{h.value}</div>
                {/each}
                {#if shown.spec.body.type !== "none"}<div class="note">Body: {shown.spec.body.type}</div>{/if}
                <Button size="sm" variant="ghost" icon="close" data-testid="tl-version-close" onclick={() => (shown = null)}>Hide</Button>
              </div>
            {/if}
          </div>
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
    justify-content: space-between;
    gap: var(--rt-space-5) var(--rt-space-6);
    padding: var(--rt-space-5) var(--rt-space-6);
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-surface-1);
  }
  .heading {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-space-2) var(--rt-space-5);
    min-width: 0;
  }
  .title {
    font: var(--rt-font-title);
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .summary {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .meta,
  .note {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .banner,
  .loading {
    padding: var(--rt-space-6) var(--rt-space-6) 0;
  }
  .loading {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
  }
  .entries {
    list-style: none;
    margin: 0;
    padding: var(--rt-space-6);
    display: flex;
    flex-direction: column;
  }
  .entry {
    position: relative;
    padding: 0 0 var(--rt-space-6) var(--rt-space-7);
  }
  .entry:last-child {
    padding-bottom: 0;
  }
  /* The rail: one line through the dots of every commit. */
  .entry::before {
    content: "";
    position: absolute;
    left: 5px;
    top: var(--rt-space-6);
    bottom: calc(-1 * var(--rt-space-2));
    width: 2px;
    background: var(--rt-border-strong);
  }
  .entry:last-child::before {
    display: none;
  }
  .dot {
    position: absolute;
    left: 0;
    top: var(--rt-space-5);
    box-sizing: border-box;
    width: 12px;
    height: 12px;
    border: 2px solid var(--rt-accent);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-surface-1);
  }
  .dot.added {
    border-color: var(--rt-ok-fg);
  }
  .dot.deleted {
    border-color: var(--rt-danger-fg);
  }
  .dot.modified {
    border-color: var(--rt-warn-fg);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    padding: var(--rt-space-5) var(--rt-space-6);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
    min-width: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .change {
    text-transform: capitalize;
  }
  .commit {
    font: var(--rt-font-title);
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .byline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .oid {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-sm);
    background: var(--rt-surface-sunken);
  }
  code,
  .path,
  .value,
  .header,
  .request-line {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    overflow-wrap: anywhere;
  }
  .path {
    font-weight: var(--rt-weight-semibold);
  }
  .rename,
  .tests,
  .fields,
  .actions,
  .confirm,
  .version {
    padding-top: var(--rt-space-4);
    border-top: 1px solid var(--rt-border);
  }
  .note {
    margin: 0;
  }
  .fields {
    list-style: none;
    margin: 0;
    padding-left: 0;
    padding-bottom: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .fields li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .fields .path {
    min-width: 8ch;
  }
  del,
  ins {
    text-decoration: none;
    padding: var(--rt-space-1) var(--rt-space-4);
    border-radius: var(--rt-radius-sm);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    overflow-wrap: anywhere;
    min-width: 0;
  }
  del {
    background: var(--rt-diff-del-bg);
    color: var(--rt-danger-fg);
    text-decoration: line-through;
    text-decoration-color: var(--rt-danger-fg);
  }
  ins {
    background: var(--rt-diff-add-bg);
    color: var(--rt-ok-fg);
  }
  .arrow {
    display: inline-flex;
    color: var(--rt-fg-subtle);
  }
  .secret {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-3);
    color: var(--rt-fg-muted);
    font-style: italic;
  }
  .tests summary {
    cursor: pointer;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .tests ul {
    list-style: none;
    margin: var(--rt-space-4) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .tests li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .names {
    flex-basis: 100%;
    margin: 0;
    padding-left: var(--rt-space-5);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
  }
  .names .ok {
    color: var(--rt-ok-fg);
  }
  .names .bad {
    color: var(--rt-danger-fg);
  }
  .actions,
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .version {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    align-items: flex-start;
  }
  .request-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .blame-wrap {
    padding: var(--rt-space-6);
  }
  .blame {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
    overflow: hidden;
  }
  .blame th,
  .blame td {
    text-align: left;
    padding: var(--rt-space-4) var(--rt-space-6);
    border-bottom: 1px solid var(--rt-border);
    vertical-align: top;
  }
  .blame tr:last-child td {
    border-bottom: none;
  }
  .blame th {
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .summary-line {
    display: block;
    margin-bottom: var(--rt-space-2);
    font: var(--rt-font-body);
  }
  .who .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-3);
  }
</style>
