<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A request's definition at both revisions, field by field side by side (wave 14, strand H):
     the semantic diff's fields with the words that differ marked; a multi-line value (body,
     script) as a side-by-side text diff. A secret's value is never shown. -->
<script lang="ts">
  import type { DiffValue, FieldChange, ModelChange } from "../commands/types";
  import { fieldLabel, fieldValueText, isMultiline, secretNote, valueText } from "../diff/format";
  import TextSides from "./TextSides.svelte";
  import Words from "./Words.svelte";
  import { wordDiff } from "./worddiff";

  let { change }: { change: ModelChange } = $props();

  const text = (f: FieldChange, v: DiffValue | null) => (v ? (fieldValueText(f.path, v) ?? "") : null);
  const isSecret = (f: FieldChange) => f.old?.kind === "secret" || f.new?.kind === "secret";
</script>

<div class="fields" data-testid="cmp-definition" role="table" aria-label="Definition, before and after">
  <div class="head" role="row">
    <span role="columnheader">Field</span>
    <span role="columnheader">Before</span>
    <span role="columnheader">After</span>
  </div>
  {#each change.fields as f (f.path.join("\u0000"))}
    {@const old = text(f, f.old)}
    {@const now = text(f, f.new)}
    <div class="row" role="row" data-testid="cmp-field" data-path={f.path.join("/")}>
      <span class="name" role="cell">{fieldLabel(f.path)}</span>
      {#if isSecret(f)}
        <span class="secret" role="cell" data-testid="cmp-secret">secret value hidden · {secretNote(f.path, f.old, f.new)}</span>
      {:else if isMultiline(f.old, f.new)}
        <div class="multi" role="cell">
          <TextSides old={valueText(f.old) ?? ""} now={valueText(f.new) ?? ""} hasOld={f.old !== null} hasNew={f.new !== null} />
        </div>
      {:else if old !== null && now !== null}
        {@const w = wordDiff(old, now)}
        <span class="value old" role="cell" data-testid="cmp-old"><Words segments={w.old} side="old" /></span>
        <span class="value new" role="cell" data-testid="cmp-new"><Words segments={w.new} side="new" /></span>
      {:else}
        <span class="value old" class:none={old === null} role="cell" data-testid="cmp-old">{old === null ? "not set" : old || "(empty)"}</span>
        <span class="value new" class:none={now === null} role="cell" data-testid="cmp-new">{now === null ? "not set" : now || "(empty)"}</span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .fields {
    display: grid;
    grid-template-columns: minmax(90px, max-content) minmax(0, 1fr) minmax(0, 1fr);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    overflow: hidden;
  }
  .head,
  .row {
    display: contents;
  }
  .head span {
    padding: 4px 8px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    background: var(--rt-bg-panel);
    border-bottom: 1px solid var(--rt-border);
  }
  .row > * {
    padding: 4px 8px;
    border-bottom: 1px solid var(--rt-border);
    min-width: 0;
  }
  .row:last-child > * {
    border-bottom: 0;
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .value {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .value.old {
    background: var(--rt-diff-del-bg);
  }
  .value.new {
    background: var(--rt-diff-add-bg);
  }
  .value.none {
    background: transparent;
    color: var(--rt-fg-muted);
    font-family: var(--rt-font-ui);
  }
  .secret,
  .multi {
    grid-column: span 2;
  }
  .secret {
    color: var(--rt-fg-muted);
  }
  .multi {
    padding: 0;
  }
</style>
