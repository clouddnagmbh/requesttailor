<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The semantic diff in the main area (working tree, commit, branch compare) with the line diff
     as a toggle (wave 12, strand A). "By field" is the semantic diff (`SemanticDiff`),
     "Lines" the line diff of every changed file. A branch compare also shows the contract
     notes (strand F) of the OpenAPI spec between the two revisions. -->
<script lang="ts">
  import ContractNotes from "../contract/ContractNotes.svelte";
  import ExtensionActions from "../shell/ExtensionActions.svelte";
  import ExtensionDiffNotes from "../shell/ExtensionDiffNotes.svelte";
  import { extensionRangeActions } from "../shell/extension-actions";
  import { getExtensions } from "../shell/extension-tabs";
  import type { CollectionDiff, DiffScope } from "../commands/types";
  import { getGitViews } from "../git/views.svelte";
  import { getApp } from "../state/app.svelte";
  import { Button, EmptyState, SegmentedControl, Spinner } from "../ui";
  import { summary } from "./format";
  import LineDiff from "./LineDiff.svelte";
  import SemanticDiff from "./SemanticDiff.svelte";

  let { scope }: { scope: DiffScope } = $props();
  const app = getApp();
  const views = getGitViews();
  // Wave 16: actions of extensions on the two revisions of a comparison.
  const extensions = getExtensions();
  const rangeActions = $derived(
    scope.kind === "between"
      ? extensionRangeActions(extensions, { collection: app.activeCollection, from: scope.from, to: scope.to, source: "diff" })
      : [],
  );

  let diff = $state<CollectionDiff | null>(null);
  let error = $state<string | null>(null);
  let mode = $state("fields");

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
    <div class="heading">
      <span class="title" data-testid="sd-title">{title}</span>
      {#if diff}<span class="summary" data-testid="sd-summary">{summary(diff.changes)}</span>{/if}
    </div>
    <div class="actions">
      {#if scope.kind === "between"}
        {@const s = scope}
        <Button size="sm" icon="compare" data-testid="sd-compare" onclick={() => views.open({ kind: "compare", from: s.from, to: s.to })}>Compare side by side</Button>
      {/if}
      <ExtensionActions actions={rangeActions} label="Range actions" />
      <SegmentedControl
        size="sm"
        label="Diff display"
        bind:value={mode}
        options={[
          { value: "fields", label: "By field", testid: "sd-mode-fields" },
          { value: "lines", label: "Lines", testid: "sd-mode-lines" },
        ]}
      />
    </div>
  </div>

  {#if scope.kind === "between"}
    <div class="notes">
      <ContractNotes from={scope.from} to={scope.to} />
      <ExtensionDiffNotes from={scope.from} to={scope.to} />
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert" data-testid="sd-error">{error}</p>
  {:else if !diff}
    <p class="hint"><Spinner size="sm" label="Reading the changes" /> Reading the changes…</p>
  {:else if mode === "fields"}
    <SemanticDiff {diff}>
      {#snippet fileDetail(file)}
        <LineDiff {scope} path={file.path} />
      {/snippet}
    </SemanticDiff>
  {:else if diff.files.length === 0}
    <div class="body" data-testid="sd-empty"><EmptyState icon="compare" title="No changes" text="Nothing differs in this comparison." size="sm" /></div>
  {:else}
    <div class="body" data-testid="sd-lines">
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
    overflow-wrap: anywhere;
  }
  .summary {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    padding: var(--rt-space-6) var(--rt-space-6) 0;
  }
  .notes:empty {
    display: none;
  }
  .body {
    padding: var(--rt-space-6);
  }
  .hint,
  .error {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    margin: 0;
    padding: var(--rt-space-6);
    font: var(--rt-font-body);
  }
  .hint {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger-fg);
  }
</style>
