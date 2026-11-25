<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Collection lint (wave 18, strand F): the health score, the findings by rule or by folder, a
     level per rule and "Ignore this". It lints the model in memory (unsaved edits included);
     nothing is sent. Clicking a request opens it. Ignoring and levels are written to
     `opencollection.yml` (unsaved, like any edit). -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { LintFinding, LintReport, LintSettings } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Badge, Banner, Button, EmptyState, Icon, Progress, SegmentedControl, Select } from "../ui";
  import {
    FORMULA,
    LEVELS,
    RULES,
    groupByFolder,
    groupByRule,
    levelOf,
    opensRequest,
    ruleInfo,
    scoreTone,
    withLevel,
  } from "./model";

  let { app, close }: ToolProps = $props();

  let report = $state<LintReport | null>(null);
  let settings = $state<LintSettings | null>(null);
  let error = $state<string | null>(null);
  let running = $state(false);
  let by = $state("rule");

  async function run() {
    running = true;
    error = null;
    try {
      [report, settings] = await Promise.all([app.backend.lint.run(), app.backend.lint.settings()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      running = false;
    }
  }

  // Runs when the tool opens and whenever the active collection or the set of unsaved files changes.
  $effect(() => {
    void app.collection?.id;
    void app.dirty.length;
    untrack(() => void run());
  });

  async function ignore(f: LintFinding) {
    try {
      await app.backend.lint.ignore(f.path, f.rule);
      await app.refreshDirty();
      await run();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function setLevel(rule: (typeof RULES)[number]["rule"], level: string) {
    if (!settings) return;
    try {
      await app.backend.lint.setSettings(withLevel($state.snapshot(settings), rule, level as (typeof LEVELS)[number]));
      await app.refreshDirty();
      await run();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function open(f: LintFinding) {
    if (opensRequest(f.path)) {
      close();
      await app.select(f.path);
    }
  }

  const tone = (level: string) => (level === "error" ? "danger" : level === "warn" ? "warn" : "info");
  const shown = (path: string) => (path === "" ? "(collection)" : path);
  const folderLabel = (folder: string) => (folder === "" ? "Collection" : folder);
  const ruleScore = (rule: string) => report?.score.rules.find((r) => r.rule === rule);
</script>

<div class="lint" data-testid="lint">
  {#if error}
    <Banner tone="danger" data-testid="lint-error">{error}</Banner>
  {/if}

  {#if !report}
    <EmptyState icon="check" title="Linting…" text="Reading the collection in memory." />
  {:else}
    <section class="top" aria-label="Health score">
      <div class="score {scoreTone(report.score.score)}" data-testid="lint-score">
        <span class="number">{report.score.score}</span>
        <span class="of">/ 100</span>
      </div>
      <div class="meta">
        <h3>{report.collection}</h3>
        <p class="muted">
          {report.findings.length}
          {report.findings.length === 1 ? "finding" : "findings"}{report.ignored ? `, ${report.ignored} ignored` : ""}.
          {#if settings?.min_score != null}Minimum for pipelines: {settings.min_score}.{/if}
        </p>
        <details class="formula">
          <summary>How is this calculated?</summary>
          <p>{FORMULA}</p>
        </details>
      </div>
      <Button variant="ghost" icon="redo" loading={running} onclick={() => void run()} data-testid="lint-run">Run again</Button>
    </section>

    <section class="rules" aria-label="Rules">
      {#each RULES as info (info.rule)}
        {@const s = ruleScore(info.rule)}
        <div class="rule" data-testid="lint-rule-{info.rule}">
          <div class="rule-head">
            <strong>{info.title}</strong>
            <span class="muted count">{s && s.total > 0 ? `${s.ok} / ${s.total}` : "nothing to check"}</span>
          </div>
          <Progress
            value={s && s.total > 0 ? s.ok : 0}
            max={s && s.total > 0 ? s.total : 1}
            label="{info.title}: satisfied"
            size="sm"
            tone={s && s.total > 0 ? scoreTone(Math.round((100 * s.ok) / s.total)) : "accent"}
          />
          <p class="muted about">{info.about}</p>
          {#if settings}
            <Select
              size="sm"
              label="Level of {info.title}"
              value={levelOf(settings, info.rule)}
              options={LEVELS.map((l) => ({ value: l, label: l }))}
              onchange={(e) => void setLevel(info.rule, e.currentTarget.value)}
              data-testid="lint-level-{info.rule}"
            />
          {/if}
        </div>
      {/each}
    </section>

    <section class="list" aria-label="Findings">
      <div class="bar">
        <h3>Findings</h3>
        <SegmentedControl
          label="Group findings by"
          size="sm"
          bind:value={by}
          options={[
            { value: "rule", label: "By rule" },
            { value: "folder", label: "By folder" },
          ]}
        />
      </div>

      {#if report.findings.length === 0}
        <EmptyState icon="success" title="Nothing to fix" text="No rule found anything in this collection." data-testid="lint-empty" />
      {:else if by === "rule"}
        {#each groupByRule(report.findings) as group (group.rule)}
          <h4>{ruleInfo(group.rule).title} <Badge kind="count" value={group.findings.length} /></h4>
          <ul>
            {#each group.findings as f (f.path + f.message + (f.location ?? ""))}
              {@render row(f)}
            {/each}
          </ul>
        {/each}
      {:else}
        {#each report.folders.filter((x) => report!.findings.some((f) => groupByFolder([f])[0].folder === x.folder)) as fs (fs.folder)}
          <h4>
            {folderLabel(fs.folder)}
            <Badge kind="label" tone={scoreTone(fs.score.score)} value="{fs.score.score}/100" />
          </h4>
          <ul>
            {#each groupByFolder(report.findings).find((g) => g.folder === fs.folder)?.findings ?? [] as f (f.path + f.rule + f.message + (f.location ?? ""))}
              {@render row(f)}
            {/each}
          </ul>
        {/each}
        {#each groupByFolder(report.findings).filter((g) => !report!.folders.some((x) => x.folder === g.folder)) as g (g.folder)}
          <h4>{folderLabel(g.folder)}</h4>
          <ul>
            {#each g.findings as f (f.path + f.rule + f.message + (f.location ?? ""))}
              {@render row(f)}
            {/each}
          </ul>
        {/each}
      {/if}
    </section>
  {/if}
</div>

{#snippet row(f: LintFinding)}
  <li class="finding" data-testid="lint-finding" data-rule={f.rule} data-level={f.level}>
    <Badge kind="label" tone={tone(f.level)} value={f.level} />
    <button class="where" disabled={!opensRequest(f.path)} onclick={() => void open(f)} title={opensRequest(f.path) ? "Open the request" : undefined}>
      {shown(f.path)}
    </button>
    <span class="what">{f.message}</span>
    <Button variant="ghost" size="sm" onclick={() => void ignore(f)} data-testid="lint-ignore">
      <Icon name="eye" size="sm" /> Ignore this
    </Button>
  </li>
{/snippet}

<style>
  .lint {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    padding: var(--rt-space-4) var(--rt-space-5);
    min-width: 0;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--rt-space-5);
    flex-wrap: wrap;
  }
  .score {
    display: flex;
    align-items: baseline;
    gap: var(--rt-space-2);
    padding: var(--rt-space-3) var(--rt-space-4);
    border-radius: var(--rt-radius-lg);
    border: 1px solid var(--rt-border);
  }
  .score.ok {
    background: var(--rt-ok-bg);
    color: var(--rt-ok-fg);
    border-color: var(--rt-ok-border);
  }
  .score.warn {
    background: var(--rt-warn-bg);
    color: var(--rt-warn-fg);
    border-color: var(--rt-warn-border);
  }
  .score.danger {
    background: var(--rt-danger-bg);
    color: var(--rt-danger-fg);
    border-color: var(--rt-danger-border);
  }
  .number {
    font-size: 2.5rem;
    font-weight: 600;
    line-height: 1;
  }
  .of {
    font: var(--rt-font-small);
  }
  .meta {
    flex: 1;
    min-width: 12rem;
  }
  h3,
  h4 {
    margin: 0;
    font: var(--rt-font-title);
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  h4 {
    margin-top: var(--rt-space-4);
    font: var(--rt-font-body);
    font-weight: 600;
  }
  .muted {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    margin: var(--rt-space-1) 0 0;
  }
  .formula summary {
    cursor: pointer;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .formula p {
    font: var(--rt-font-small);
    max-width: 60ch;
  }
  .rules {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: var(--rt-space-3);
  }
  .rule {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    padding: var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-1);
  }
  .rule-head {
    display: flex;
    justify-content: space-between;
    gap: var(--rt-space-2);
  }
  .about {
    margin: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--rt-space-3);
  }
  ul {
    list-style: none;
    margin: var(--rt-space-2) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .finding {
    display: grid;
    grid-template-columns: auto minmax(8rem, 16rem) 1fr auto;
    align-items: center;
    gap: var(--rt-space-3);
    min-height: var(--rt-control-lg);
    padding: var(--rt-space-1) 0;
    border-bottom: 1px solid var(--rt-border);
  }
  .where {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: var(--rt-accent);
    font: var(--rt-font-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .where:disabled {
    color: var(--rt-fg);
    cursor: default;
  }
  .where:focus-visible {
    outline: none;
    box-shadow: var(--rt-focus-ring);
  }
  .what {
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
    min-width: 0;
  }
  @media (max-width: 700px) {
    .finding {
      grid-template-columns: auto 1fr;
    }
    .what,
    .finding :global(button:last-child) {
      grid-column: 1 / -1;
    }
  }
</style>
