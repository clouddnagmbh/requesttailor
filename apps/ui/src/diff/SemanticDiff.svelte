<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One semantic diff, embeddable: the PR review (strand E) shows it with comments at fields.
     `oncomment`: the user wants to comment on a field (file and 1-based line from
     `FieldChange.line`) (wave 12, strand A).
     Each changed request, folder, collection and environment is a card with its changed
     fields: old → new for a single value, a code diff for a multi-line one (body, script).
     A secret's value is never shown. Files the model does not fully cover are listed at
     the end; `fileDetail` (optional) renders something under each, e.g. its line diff. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  import type { ChangeTarget, ChangedFile, CollectionDiff, DiffValue, FieldChange, FieldPath, ModelChange } from "../commands/types";
  import { fieldLabel, fieldValueText, isMultiline, KIND_LABEL, secretNote, targetLabel, valueText } from "./format";
  import { Badge, Button, EmptyState, Icon } from "../ui";
  import { lineOps } from "./textdiff";

  let {
    diff,
    oncomment,
    fileDetail,
  }: {
    diff: CollectionDiff;
    oncomment?: (target: ChangeTarget, field: FieldPath, file: string, line: number) => void;
    fileDetail?: Snippet<[ChangedFile]>;
  } = $props();

  const TONE = { added: "ok", removed: "danger", modified: "warn", moved: "info" } as const;
  /** Git status letters of a changed file, shown as the word by the status Badge. */
  const FILE_STATUS = { added: "A", removed: "D", modified: "M" } as const;
  const uncovered = $derived(diff.files.filter((f) => !f.covered));

  function key(c: ModelChange): string {
    const t = c.target;
    return `${t.kind}:${t.kind === "environment" ? t.name : t.kind === "collection" ? "" : t.path}:${c.change.kind}`;
  }

  const lines = (f: FieldChange) => lineOps(valueText(f.old), valueText(f.new));
  const text = (v: DiffValue | null, path: FieldPath) => fieldValueText(path, v) ?? "";
</script>

<div class="sd" data-testid="semantic-diff">
  {#if diff.changes.length === 0 && uncovered.length === 0}
    <div data-testid="sd-empty"><EmptyState icon="compare" title="No changes" text="Nothing differs in this comparison." size="sm" /></div>
  {/if}

  {#each diff.changes as c (key(c))}
    <section class="change" data-testid="sd-change" data-kind={c.change.kind} data-target={c.target.kind} aria-label="{KIND_LABEL[c.change.kind]} {targetLabel(c.target).toLowerCase()} {c.name}">
      <header>
        <span data-testid="sd-badge"><Badge value={KIND_LABEL[c.change.kind]} tone={TONE[c.change.kind] ?? "neutral"} /></span>
        <span class="target">{targetLabel(c.target)}</span>
        <strong class="name" data-testid="sd-name">{c.name}</strong>
        <span class="file" data-testid="sd-file">{c.file}</span>
        {#if c.change.kind === "moved"}<span class="from" data-testid="sd-from">from {c.change.from}</span>{/if}
      </header>
      {#if c.fields.length > 0}
        <ul class="fields">
          {#each c.fields as f (f.path.join("\u0000"))}
            <li data-testid="sd-field" data-path={f.path.join("/")}>
              <div class="field-head">
                <span class="path" data-testid="sd-path">{fieldLabel(f.path)}</span>
                {#if f.line}<span class="line">line {f.line}</span>{/if}
                {#if oncomment && f.line}
                  <span class="comment">
                    <Button variant="ghost" size="sm" icon="edit" data-testid="sd-comment" aria-label="Comment on {fieldLabel(f.path)}" onclick={() => oncomment(c.target, f.path, c.file, f.line!)}>Comment</Button>
                  </span>
                {/if}
              </div>
              {#if f.old?.kind === "secret" || f.new?.kind === "secret"}
                <div class="values">
                  <span class="secret" data-testid="sd-secret"><Icon name="lock" size="sm" /> secret value hidden</span>
                  <span class="note" data-testid="sd-secret-note">{secretNote(f.path, f.old, f.new)}</span>
                </div>
              {:else if isMultiline(f.old, f.new)}
                <div class="code" data-testid="sd-code">
                  {#each lines(f) as op, i (i)}
                    <div class="row {op.kind}" data-kind={op.kind}><span class="sign">{op.kind === "added" ? "+" : op.kind === "removed" ? "-" : " "}</span><span class="text">{op.text}</span></div>
                  {/each}
                </div>
              {:else}
                <div class="values">
                  {#if f.old}<del class="old" data-testid="sd-old">{text(f.old, f.path) || "(empty)"}</del>{/if}
                  {#if f.old && f.new}<span class="arrow" aria-hidden="true"><Icon name="chevron-right" size="sm" /></span>{/if}
                  {#if f.new}<ins class="new" data-testid="sd-new">{text(f.new, f.path) || "(empty)"}</ins>{/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/each}

  {#if uncovered.length > 0}
    <section class="uncovered" data-testid="sd-uncovered" aria-label="Files not fully shown">
      <h3>Not fully shown by field</h3>
      <p class="hint">These files also differ in ways the field view cannot show (comments, notes, other settings).</p>
      <ul>
        {#each uncovered as f (f.path)}
          <li data-testid="sd-uncovered-file">
            <div class="uncovered-head"><span class="file">{f.path}</span> <Badge kind="status" long value={FILE_STATUS[f.status]} /></div>
            {#if fileDetail}{@render fileDetail(f)}{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .sd {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-6);
    padding: var(--rt-space-6);
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .change {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
    overflow: hidden;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4) var(--rt-space-5);
    padding: var(--rt-space-4) var(--rt-space-6);
    background: var(--rt-surface-sunken);
  }
  .name {
    font: var(--rt-font-title);
    overflow-wrap: anywhere;
  }
  .target,
  .file,
  .from,
  .note,
  .line {
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .file,
  .from {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .fields {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .fields li {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    padding: var(--rt-space-5) var(--rt-space-6);
    border-top: 1px solid var(--rt-border);
  }
  header + .fields li:first-child {
    border-top: 1px solid var(--rt-border);
  }
  .field-head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-5);
    min-height: var(--rt-control-sm);
  }
  .path {
    font: var(--rt-font-small);
    font-weight: var(--rt-weight-semibold);
    font-family: var(--rt-font-mono);
  }
  .comment {
    margin-left: auto;
  }
  .values {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-4);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-leading-small);
  }
  del,
  ins {
    text-decoration: none;
    padding: var(--rt-space-1) var(--rt-space-4);
    border-radius: var(--rt-radius-sm);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
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
    padding: var(--rt-space-1) var(--rt-space-4);
    border: 1px dashed var(--rt-border-strong);
    border-radius: var(--rt-radius-sm);
    color: var(--rt-fg-muted);
  }
  .code {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-leading-small);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    overflow: hidden;
  }
  .row {
    padding: 0 var(--rt-space-4);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .row.added {
    background: var(--rt-diff-add-bg);
  }
  .row.removed {
    background: var(--rt-diff-del-bg);
  }
  .sign {
    display: inline-block;
    width: 2ch;
    color: var(--rt-fg-muted);
    user-select: none;
  }
  .uncovered {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
  }
  .uncovered h3 {
    margin: 0;
    font: var(--rt-font-title);
  }
  .uncovered ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .uncovered-head {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
  }
</style>
