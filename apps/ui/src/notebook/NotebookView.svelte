<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- API notebooks (wave 19, strand W): a Markdown file next to the requests with prose and
     request blocks that run one after another and pass variables on. Prose is shown as text
     nodes only (never HTML). Outputs live in this view and are never saved. A writing block runs
     only with "Allow writes", never against production. -->
<script lang="ts">
  import { onMount } from "svelte";

  import { Markdown } from "@tailor-kit/ai-ui";
  import type { BlockResult, Notebook, NotebookEntry } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, Checkbox, EmptyState, Select, Spinner } from "../ui";
  import { bodyText, blockLabel, carriedVars, chartPoints, failed, isRequest, newPath, showLabel, starter, statusLine } from "./model";
  import { presetHolder } from "./open.svelte";

  let { app }: ToolProps = $props();

  let entries = $state<NotebookEntry[]>([]);
  let path = $state("");
  let notebook = $state<Notebook | null>(null);
  let results = $state<Record<string, BlockResult>>({});
  let environment = $state("");
  let allowWrites = $state(false);
  let running = $state<string | null>(null);
  let error = $state<string | null>(null);

  const names = $derived(app.environments.map((e) => e.name));
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  onMount(async () => {
    environment = app.environment ?? "";
    await reload(presetHolder.take());
  });

  async function reload(want: string | null) {
    try {
      entries = await app.backend.notebook.list();
      const pick = want ?? (entries.some((e) => e.path === path) ? path : (entries[0]?.path ?? ""));
      await open(pick);
    } catch (e) {
      error = message(e);
    }
  }

  async function open(p: string) {
    path = p;
    results = {};
    notebook = null;
    if (!p) return;
    try {
      notebook = await app.backend.notebook.read(p);
      error = null;
    } catch (e) {
      error = message(e);
    }
  }

  async function create() {
    const name = newPath("notebook", entries.map((e) => e.path));
    try {
      await app.backend.notebook.stage(name, starter(name.replace(".notebook.md", "")));
      await app.refreshDirty();
      await reload(name);
    } catch (e) {
      error = message(e);
    }
  }

  async function run(block: string | null) {
    if (!path || running) return;
    running = block ?? "*";
    error = null;
    try {
      await app.settled();
      const out = await app.backend.notebook.run({
        path,
        block: block ?? undefined,
        environment: environment || undefined,
        allow_writes: allowWrites,
        vars: block ? carriedVars(Object.values(results)) : [],
      });
      const next = block ? { ...results } : {};
      for (const r of out.results) next[r.block] = r;
      results = next;
    } catch (e) {
      error = message(e);
    } finally {
      running = null;
    }
  }
</script>

<section class="nb" data-testid="notebook-panel" aria-label="Notebooks">
  <header class="head">
    <h2>Notebooks</h2>
    <p class="note">A notebook is a Markdown file next to your requests. Outputs are shown here and never saved.</p>
  </header>

  {#if entries.length === 0 && !error}
    <EmptyState icon="file-text" title="No notebooks yet" text="Create one, or add a file ending in .notebook.md to the collection.">
      <Button variant="primary" icon="add" onclick={create} data-testid="notebook-create">New notebook</Button>
    </EmptyState>
  {:else}
    <div class="bar">
      <div class="field">
        <Select label="Notebook" value={path} options={entries.map((e) => ({ value: e.path, label: `${e.title} (${e.blocks})` }))} onchange={(e) => open(e.currentTarget.value)} data-testid="notebook-select" />
      </div>
      <div class="field">
        <Select label="Environment" bind:value={environment} options={[{ value: "", label: "(none)" }, ...names.map((n) => ({ value: n, label: n }))]} data-testid="notebook-env" />
      </div>
      <Checkbox label="Allow writes" bind:checked={allowWrites} data-testid="notebook-writes" />
      <Button variant="primary" icon="play" loading={running === "*"} disabled={!notebook || running !== null} onclick={() => run(null)} data-testid="notebook-run-all">Run all</Button>
      <Button variant="ghost" icon="add" onclick={create} data-testid="notebook-create">New</Button>
    </div>
  {/if}

  {#if error}
    <div data-testid="notebook-error"><Banner tone="danger" title="Notebook">{error}</Banner></div>
  {/if}

  {#if notebook}
    <div class="blocks">
      {#each notebook.blocks as block, i (i)}
        {#if block.kind === "text"}
          <div class="prose" data-testid="notebook-text"><Markdown text={block.markdown} /></div>
        {:else if isRequest(block)}
          {@const r = results[block.id]}
          <article class="req" data-testid="notebook-block" data-block={block.id} data-failed={failed(r?.report)}>
            <div class="top">
              <Badge kind="method" value={block.embedded?.method ?? "REF"} />
              <span class="mono label">{blockLabel(block)}</span>
              <span class="id">{block.id}</span>
              <Button size="sm" icon="play" loading={running === block.id} disabled={running !== null} onclick={() => run(block.id)} data-testid="notebook-run-{block.id}">Run</Button>
            </div>
            {#if r?.skipped}
              <p class="skip" data-testid="notebook-skipped">{r.skipped}</p>
            {/if}
            {#if r?.report}
              <p class="status mono" data-testid="notebook-status">{statusLine(r.report)}</p>
            {/if}
            {#each r?.set ?? [] as [n, v] (n)}
              <p class="var mono" data-testid="notebook-set">{n} = {v}</p>
            {/each}
            {#each r?.notes ?? [] as n (n)}<p class="skip">{n}</p>{/each}
            {#if r?.report && (block.show ?? []).some((s) => s.kind === "response")}
              {@const b = bodyText(r.report)}
              <pre class="body mono" data-testid="notebook-body">{b.text}{b.cut ? "\n… (cut)" : ""}</pre>
            {/if}
            {#each r?.views ?? [] as v, j (j)}
              <div class="view" data-testid="notebook-view" data-kind={v.show.kind}>
                <p class="cap">{showLabel(v.show)}</p>
                {#if v.result.notice}<p class="skip">{v.result.notice}</p>{/if}
                {#if v.show.kind === "table" && v.result.columns?.length}
                  <table>
                    <thead><tr>{#each v.result.columns as c (c)}<th>{c}</th>{/each}</tr></thead>
                    <tbody>
                      {#each v.result.rows ?? [] as row, k (k)}
                        <tr>{#each row as cell, m (m)}<td class="mono">{typeof cell === "string" ? cell : JSON.stringify(cell)}</td>{/each}</tr>
                      {/each}
                    </tbody>
                  </table>
                {:else if v.show.kind === "chart"}
                  {@const pts = chartPoints(v.result.values, 320, 90)}
                  {#if pts.length}
                    <svg viewBox="0 0 320 90" role="img" aria-label="Chart of {v.show.path}" class="chart" data-testid="notebook-chart">
                      <polyline points={pts.map((p) => `${p.x},${p.y}`).join(" ")} fill="none" stroke="currentColor" stroke-width="2" />
                      {#each pts as p, k (k)}<circle cx={p.x} cy={p.y} r="3" fill="currentColor"><title>{p.value}</title></circle>{/each}
                    </svg>
                  {/if}
                {/if}
              </div>
            {/each}
          </article>
        {/if}
      {/each}
    </div>
    {#if running}<div class="loading"><Spinner label="Running" /> Running…</div>{/if}
  {/if}
</section>

<style>
  .nb { display: flex; flex-direction: column; gap: var(--rt-space-5); flex: 1; min-height: 0; overflow: auto; box-sizing: border-box; padding: var(--rt-space-6); }
  h2 { margin: 0; font: var(--rt-font-title); }
  .note, .cap { margin: 0; color: var(--rt-fg-muted); font: var(--rt-font-small); }
  .bar { display: flex; flex-wrap: wrap; align-items: flex-end; gap: var(--rt-space-5); }
  .field { flex: 0 1 240px; min-width: 150px; }
  .blocks { display: flex; flex-direction: column; gap: var(--rt-space-4); }
  .req { border: 1px solid var(--rt-border); border-radius: var(--rt-radius-md); padding: var(--rt-space-4); display: flex; flex-direction: column; gap: var(--rt-space-3); min-width: 0; }
  .req[data-failed="true"] { border-color: var(--rt-danger-border); }
  .top { display: flex; flex-wrap: wrap; align-items: center; gap: var(--rt-space-3); }
  .label { overflow-wrap: anywhere; flex: 1; min-width: 0; }
  .id { color: var(--rt-fg-muted); font: var(--rt-font-small); }
  .skip { margin: 0; color: var(--rt-fg-muted); }
  .status, .var { margin: 0; }
  .body { margin: 0; max-height: 280px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--rt-surface-sunken); padding: var(--rt-space-3); border-radius: var(--rt-radius-sm); }
  table { border-collapse: collapse; width: 100%; }
  th, td { text-align: left; padding: var(--rt-space-1) var(--rt-space-3); border-bottom: 1px solid var(--rt-border); overflow-wrap: anywhere; }
  .chart { width: 100%; max-width: 420px; color: var(--rt-accent, currentColor); }
  .loading { display: flex; align-items: center; gap: var(--rt-space-3); color: var(--rt-fg-muted); }
  .mono { font-family: var(--rt-font-mono); }
</style>
