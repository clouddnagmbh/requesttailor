<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- A suggested change as a diff the user applies or discards (wave 16, strand C). One look for
     every suggestion: a suggested edit of a request, a script, a file, and the inline proposal
     in a code editor (`mode="inline"`; in a dialog the dialog's title bar says it). Changed
     words are marked in one-line fields, changed lines in texts. Mod+Enter applies. Nothing
     here writes anything: the parent does that in `onapply`. -->
<script lang="ts">
  import Markdown from "../ai/Markdown.svelte";
  import Words from "../compare/Words.svelte";
  import { wordDiff } from "../compare/worddiff";
  import type { SuggestionNote } from "../extensions";
  import { onMount } from "svelte";

  import { Button, Icon, Kbd } from "../ui";
  import { suggestionOpened } from "./keys.svelte";
  import { blockStats, diffRows, isMulti, totalStats, type PreviewBlock } from "./model";

  let {
    blocks,
    title = "Suggested change",
    subtitle = "",
    note,
    mode = "dialog",
    applyLabel = "Apply",
    busy = false,
    error = null,
    undoable = true,
    onapply,
    ondiscard,
    ondismiss,
  }: {
    blocks: PreviewBlock[];
    title?: string;
    subtitle?: string;
    note?: SuggestionNote;
    mode?: "dialog" | "inline";
    applyLabel?: string;
    busy?: boolean;
    error?: string | null;
    /** The change can be undone after applying (says so in the footer). */
    undoable?: boolean;
    onapply: () => void;
    ondiscard: () => void;
    /** "Don't suggest this again" (only when the suggestion has a `dismissKey`). */
    ondismiss?: () => void;
  } = $props();

  const stats = $derived(totalStats(blocks));
  const empty = $derived(blocks.length === 0 || (stats.added === 0 && stats.removed === 0));
  let applyButton = $state<HTMLButtonElement>();

  $effect(() => {
    // Keyboard first: the primary action has the focus, so Enter applies and Esc discards.
    // After the frame, so a dialog around the view has placed its own focus first.
    const button = applyButton;
    if (!button) return;
    const frame = requestAnimationFrame(() => button.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  });

  onMount(() => suggestionOpened(mode === "inline"));

  // On the window, so the keys work wherever the focus is (a menu that just closed leaves it on
  // the page). An inline proposal steps back while a dialog is open.
  function onkeydown(e: KeyboardEvent) {
    if (mode === "inline" && document.querySelector('[data-testid="extension-dialog"]')) return;
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      if (!empty && !busy && !e.repeat) onapply();
    } else if (e.key === "Escape" && mode === "inline" && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      ondiscard();
    }
  }

  const SIGN = { same: " ", added: "+", removed: "−" } as const;
</script>

{#snippet buttons()}
  <span class="buttons">
    {#if ondismiss}
      <Button variant="ghost" size={mode === "inline" ? "sm" : "md"} data-testid="suggest-dismiss" onclick={ondismiss}>Don't suggest this again</Button>
    {/if}
    <Button variant="ghost" size={mode === "inline" ? "sm" : "md"} data-testid="suggest-discard" onclick={ondiscard}>
      {empty ? "Close" : "Discard"}{#if mode === "inline"}<span class="key"><Kbd keys="Esc" size="sm" /></span>{/if}
    </Button>
    {#if !empty}
      <Button variant="primary" size={mode === "inline" ? "sm" : "md"} data-testid="suggest-apply" loading={busy} bind:element={applyButton} onclick={onapply}>
        {applyLabel}<span class="key"><Kbd keys="Mod+Enter" size="sm" /></span>
      </Button>
    {/if}
  </span>
{/snippet}

<svelte:window onkeydowncapture={onkeydown} />

<section class="view {mode}" data-testid="suggested-edit" aria-label="Suggested change">
  {#if mode === "inline" || subtitle}
    <header>
      {#if mode === "inline"}
        <span class="mark"><Icon name="spark" size="sm" /></span>
        <strong>{title}</strong>
      {/if}
      {#if subtitle}<span class="where">{subtitle}</span>{/if}
      <span class="stats" aria-label="{stats.added} added, {stats.removed} removed">
        {#if stats.added > 0}<span class="add" data-testid="suggest-added">+{stats.added}</span>{/if}
        {#if stats.removed > 0}<span class="del" data-testid="suggest-removed">−{stats.removed}</span>{/if}
      </span>
      {#if mode === "inline"}{@render buttons()}{/if}
    </header>
  {/if}

  {#if note?.title || note?.explanation}
    <div class="note" data-testid="suggest-note">
      {#if note.title}<p class="note-title">{note.title}</p>{/if}
      {#if note.explanation}<Markdown text={note.explanation} />{/if}
    </div>
  {/if}

  {#if empty}
    <div class="nothing" data-testid="suggest-empty">
      <Icon name="success" />
      <p><strong>Nothing would change.</strong> The suggestion is the same as what is there now.</p>
    </div>
  {:else}
    <div class="blocks">
      {#each blocks as block, i (i)}
        {@const s = blockStats(block)}
        <div class="block" data-testid="suggest-block">
          <div class="label">
            <span>{block.label}</span>
            {#if block.old === null}<span class="tag add">new</span>{:else if block.next === null}<span class="tag del">removed</span>{/if}
            {#if isMulti(block)}
              <span class="stats">
                {#if s.added > 0}<span class="add">+{s.added}</span>{/if}
                {#if s.removed > 0}<span class="del">−{s.removed}</span>{/if}
              </span>
            {/if}
          </div>
          {#if isMulti(block)}
            <div class="code" role="table" aria-label="{block.label}, changed lines">
              {#each diffRows(block.old, block.next) as row, k (k)}
                {#if row.kind === "gap"}
                  <div class="row gap" role="row"><span class="gutter"></span><span class="gutter"></span><span class="sign"></span><span class="text">⋯ {row.hidden} unchanged {row.hidden === 1 ? "line" : "lines"}</span></div>
                {:else}
                  <div class="row {row.kind}" role="row" data-kind={row.kind}>
                    <span class="gutter" aria-hidden="true">{row.oldLine ?? ""}</span>
                    <span class="gutter" aria-hidden="true">{row.newLine ?? ""}</span>
                    <span class="sign" aria-hidden="true">{SIGN[row.kind]}</span>
                    <span class="text">{row.text === "" ? " " : row.text}</span>
                  </div>
                {/if}
              {/each}
            </div>
          {:else}
            {@const words = block.old !== null && block.next !== null ? wordDiff(block.old, block.next) : null}
            <div class="code" role="table" aria-label="{block.label}, before and after">
              {#if block.old !== null}
                <div class="row removed" role="row" data-kind="removed">
                  <span class="sign" aria-hidden="true">−</span>
                  <span class="text">{#if words}<Words segments={words.old} side="old" />{:else}{block.old === "" ? "(empty)" : block.old}{/if}</span>
                </div>
              {/if}
              {#if block.next !== null}
                <div class="row added" role="row" data-kind="added">
                  <span class="sign" aria-hidden="true">+</span>
                  <span class="text">{#if words}<Words segments={words.new} side="new" />{:else}{block.next === "" ? "(empty)" : block.next}{/if}</span>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert" data-testid="suggest-error"><Icon name="error" size="sm" /> {error}</p>
  {/if}

  <footer>
    <span class="foot-note">
      {#if empty}Nothing to apply.{:else if undoable}Applied as an unsaved change. You can undo it.{:else}Applied as an unsaved change. Discard it with the other unsaved changes.{/if}
    </span>
    {#if mode === "dialog"}{@render buttons()}{/if}
  </footer>
</section>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    min-width: 0;
    color: var(--rt-fg);
  }
  .view.inline {
    padding: var(--rt-space-5);
    border: 1px solid var(--rt-info-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    min-width: 0;
  }
  header strong {
    font: var(--rt-font-title);
  }
  .mark {
    display: inline-grid;
    place-items: center;
    width: var(--rt-control-sm);
    height: var(--rt-control-sm);
    border-radius: var(--rt-radius-md);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .where {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
  }
  .stats {
    display: inline-flex;
    gap: var(--rt-space-3);
    margin-left: auto;
    font: var(--rt-font-small);
    font-family: var(--rt-font-mono);
    font-variant-numeric: tabular-nums;
  }
  .key {
    display: inline-flex;
    margin-left: var(--rt-space-3);
  }
  .key :global(kbd) {
    border-color: color-mix(in srgb, currentColor 45%, transparent);
    background: transparent;
    color: inherit;
  }
  header .buttons {
    margin-left: 0;
  }
  .label .stats {
    margin-left: 0;
  }
  .add {
    color: var(--rt-ok-fg);
  }
  .del {
    color: var(--rt-danger-fg);
  }
  .note {
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-info-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-info-bg);
    color: var(--rt-info-fg);
    font: var(--rt-font-body);
  }
  .note :global(p) {
    margin: 0;
  }
  .note :global(p + p) {
    margin-top: var(--rt-space-3);
  }
  .note-title {
    margin: 0 0 var(--rt-space-2);
    font-weight: var(--rt-weight-semibold);
  }
  .blocks {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-5);
    max-height: min(52vh, 28rem);
    overflow: auto;
    padding-right: var(--rt-space-1);
  }
  .block {
    /* The list scrolls; a block keeps its height instead of shrinking to its label. */
    flex-shrink: 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-1);
    overflow: hidden;
  }
  .label {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    padding: var(--rt-space-3) var(--rt-space-5);
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-surface-sunken);
    font: var(--rt-font-small);
    font-weight: var(--rt-weight-semibold);
    color: var(--rt-fg-muted);
  }
  .tag {
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-pill);
    font: var(--rt-font-caption);
  }
  .tag.add {
    background: var(--rt-ok-bg);
    border: 1px solid var(--rt-ok-border);
  }
  .tag.del {
    background: var(--rt-danger-bg);
    border: 1px solid var(--rt-danger-border);
  }
  .code {
    overflow-x: auto;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-leading-small);
  }
  .row {
    display: grid;
    grid-template-columns: auto auto 2ch minmax(0, 1fr);
    min-width: max-content;
    width: 100%;
  }
  .row:not(:has(.gutter)) {
    grid-template-columns: 2ch minmax(0, 1fr);
    padding-left: var(--rt-space-5);
  }
  .row.added {
    background: var(--rt-diff-add-bg);
  }
  .row.removed {
    background: var(--rt-diff-del-bg);
  }
  .row.gap {
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-muted);
  }
  .gutter {
    min-width: 3ch;
    padding: 0 var(--rt-space-3);
    text-align: right;
    color: var(--rt-fg-subtle);
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .sign {
    color: var(--rt-fg-muted);
    user-select: none;
  }
  .added .sign {
    color: var(--rt-ok-fg);
  }
  .removed .sign {
    color: var(--rt-danger-fg);
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding-right: var(--rt-space-5);
  }
  .nothing {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    padding: var(--rt-space-5);
    border: 1px dashed var(--rt-border-strong);
    border-radius: var(--rt-radius-md);
    color: var(--rt-fg-muted);
  }
  .nothing p {
    margin: 0;
    font: var(--rt-font-body);
  }
  .error {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    margin: 0;
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-danger-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-danger-bg);
    color: var(--rt-danger-fg);
    font: var(--rt-font-small);
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--rt-space-5);
    flex-wrap: wrap;
  }
  .foot-note {
    flex: 1 1 14rem;
    min-width: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .buttons {
    display: inline-flex;
    gap: var(--rt-space-4);
    margin-left: auto;
  }
</style>
