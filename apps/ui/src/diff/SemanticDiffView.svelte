<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The semantic diff in the main area (working tree, commit, branch compare) with the line diff
     as a toggle (wave 12, strand A). "By field" is the semantic diff (`SemanticDiff`),
     "Lines" the line diff of every changed file. A branch compare also shows the contract
     notes (strand F) of the OpenAPI spec between the two revisions. -->
<script lang="ts">
  import ContractNotes from "../contract/ContractNotes.svelte";
  import ExtensionDiffNotes from "../shell/ExtensionDiffNotes.svelte";
  import type { CollectionDiff, DiffScope } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";
  import { summary } from "./format";
  import LineDiff from "./LineDiff.svelte";
  import SemanticDiff from "./SemanticDiff.svelte";

  let { scope }: { scope: DiffScope } = $props();
  const app = getApp();
  const views = getGitViews();

  let diff = $state<CollectionDiff | null>(null);
  let error = $state<string | null>(null);
  let mode = $state<"fields" | "lines">("fields");

  const title = $derived(
    scope.kind === "working"
      ? "Saved changes against HEAD"
      : scope.kind === "commit"
        ? `Commit ${scope.rev.slice(0, 10)} against its parent`
        : `${scope.from} → ${scope.to}`,
  );

  function request(s: DiffScope): Promise<CollectionDiff> {
    const api = app.backend.diffs;
    return s.kind === "working" ? api.working() : s.kind === "commit" ? api.commit(s.rev) : api.between(s.from, s.to);
  }

  $effect(() => {
    const s = scope;
    // Saved edits change the working diff.
    void app.loadCount;
    void app.dirty.length;
    diff = null;
    error = null;
    request(s).then(
      (d) => {
        if (s === scope) diff = d;
      },
      (e: unknown) => {
        if (s === scope) error = e instanceof Error ? e.message : String(e);
      },
    );
  });
</script>

<div class="view" data-testid="semantic-diff-view" data-scope={scope.kind}>
  <div class="bar">
    <span class="title" data-testid="sd-title">{title}</span>
    {#if diff}<span class="summary" data-testid="sd-summary">{summary(diff.changes)}</span>{/if}
    {#if scope.kind === "between"}
      {@const s = scope}
      <button class="button" data-testid="sd-compare" onclick={() => views.open({ kind: "compare", from: s.from, to: s.to })}>Compare side by side</button>
    {/if}
    <div class="toggle" role="group" aria-label="Diff display">
      <button class="button" data-testid="sd-mode-fields" aria-pressed={mode === "fields"} onclick={() => (mode = "fields")}>By field</button>
      <button class="button" data-testid="sd-mode-lines" aria-pressed={mode === "lines"} onclick={() => (mode = "lines")}>Lines</button>
    </div>
  </div>

  {#if scope.kind === "between"}
    <ContractNotes from={scope.from} to={scope.to} />
    <ExtensionDiffNotes from={scope.from} to={scope.to} />
  {/if}

  {#if error}
    <p class="error" role="alert" data-testid="sd-error">{error}</p>
  {:else if !diff}
    <p class="hint">Reading the changes…</p>
  {:else if mode === "fields"}
    <SemanticDiff {diff}>
      {#snippet fileDetail(file)}
        <LineDiff {scope} path={file.path} />
      {/snippet}
    </SemanticDiff>
  {:else if diff.files.length === 0}
    <p class="hint" data-testid="sd-empty">No changes.</p>
  {:else}
    <div data-testid="sd-lines">
      {#each diff.files as f (f.path)}
        <LineDiff {scope} path={f.path} />
      {/each}
    </div>
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
    overflow-wrap: anywhere;
  }
  .summary {
    color: var(--rt-fg-muted);
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
  .error {
    margin: 0;
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
  }
  .hint {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger);
  }
</style>
