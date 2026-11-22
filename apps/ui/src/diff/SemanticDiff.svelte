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
  import { fieldLabel, isMultiline, KIND_LABEL, targetLabel, valueText } from "./format";
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

  const uncovered = $derived(diff.files.filter((f) => !f.covered));

  function key(c: ModelChange): string {
    const t = c.target;
    return `${t.kind}:${t.kind === "environment" ? t.name : t.kind === "collection" ? "" : t.path}:${c.change.kind}`;
  }

  const lines = (f: FieldChange) => lineOps(valueText(f.old), valueText(f.new));
  const text = (v: DiffValue | null) => valueText(v) ?? "";
</script>

<div class="sd" data-testid="semantic-diff">
  {#if diff.changes.length === 0 && uncovered.length === 0}
    <p class="hint" data-testid="sd-empty">No changes.</p>
  {/if}

  {#each diff.changes as c (key(c))}
    <section class="change {c.change.kind}" data-testid="sd-change" data-kind={c.change.kind} data-target={c.target.kind} aria-label="{KIND_LABEL[c.change.kind]} {targetLabel(c.target).toLowerCase()} {c.name}">
      <header>
        <span class="badge {c.change.kind}" data-testid="sd-badge">{KIND_LABEL[c.change.kind]}</span>
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
                  <button class="link" data-testid="sd-comment" aria-label="Comment on {fieldLabel(f.path)}" onclick={() => oncomment(c.target, f.path, c.file, f.line!)}>Comment</button>
                {/if}
              </div>
              {#if f.old?.kind === "secret" || f.new?.kind === "secret"}
                <div class="values">
                  <span class="secret" data-testid="sd-secret">secret value hidden</span>
                  <span class="note">{f.old && f.new ? "changed" : f.new ? "added" : "removed"}</span>
                </div>
              {:else if isMultiline(f.old, f.new)}
                <div class="code" data-testid="sd-code">
                  {#each lines(f) as op, i (i)}
                    <div class="row {op.kind}" data-kind={op.kind}><span class="sign">{op.kind === "added" ? "+" : op.kind === "removed" ? "-" : " "}</span><span class="text">{op.text}</span></div>
                  {/each}
                </div>
              {:else}
                <div class="values">
                  {#if f.old}<del class="old" data-testid="sd-old">{text(f.old) || "(empty)"}</del>{/if}
                  {#if f.old && f.new}<span class="arrow" aria-hidden="true">→</span>{/if}
                  {#if f.new}<ins class="new" data-testid="sd-new">{text(f.new) || "(empty)"}</ins>{/if}
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
            <span class="file">{f.path}</span> <span class="note">{f.status}</span>
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
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
  }
  .change {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    overflow: hidden;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) / 2) var(--rt-gap);
    background: var(--rt-bg-panel);
  }
  .target,
  .file,
  .from,
  .note,
  .line {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .file,
  .from {
    font-family: var(--rt-font-mono);
    overflow-wrap: anywhere;
  }
  .badge {
    padding: 0 6px;
    border-radius: var(--rt-radius);
    font-size: var(--rt-size-small);
    font-weight: 600;
    border: 1px solid currentcolor;
  }
  .badge.added {
    color: var(--rt-ok);
  }
  .badge.removed {
    color: var(--rt-danger);
  }
  .badge.modified,
  .badge.moved {
    color: var(--rt-warn);
  }
  .fields {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .fields li {
    padding: calc(var(--rt-gap) / 2) var(--rt-gap);
    border-top: 1px solid var(--rt-border);
  }
  .field-head {
    display: flex;
    align-items: baseline;
    gap: var(--rt-gap);
  }
  .path {
    font-weight: 600;
  }
  .link {
    margin-left: auto;
    background: none;
    border: none;
    padding: 0;
    color: var(--rt-accent);
    cursor: pointer;
    font: inherit;
    font-size: var(--rt-size-small);
  }
  .values {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--rt-gap);
    margin-top: 2px;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  del,
  ins {
    text-decoration: none;
    padding: 0 4px;
    border-radius: var(--rt-radius);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  del {
    background: color-mix(in srgb, var(--rt-danger) 16%, transparent);
  }
  ins {
    background: color-mix(in srgb, var(--rt-ok) 16%, transparent);
  }
  .arrow {
    color: var(--rt-fg-muted);
  }
  .secret {
    padding: 0 6px;
    border: 1px dashed var(--rt-border);
    border-radius: var(--rt-radius);
    color: var(--rt-fg-muted);
  }
  .code {
    margin-top: 2px;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
  }
  .row {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .row.added {
    background: color-mix(in srgb, var(--rt-ok) 16%, transparent);
  }
  .row.removed {
    background: color-mix(in srgb, var(--rt-danger) 16%, transparent);
  }
  .sign {
    display: inline-block;
    width: 1.5ch;
    user-select: none;
  }
  .uncovered h3 {
    margin: 0;
    font-size: var(--rt-size);
  }
  .uncovered ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .uncovered li {
    padding: 2px 0;
  }
</style>
