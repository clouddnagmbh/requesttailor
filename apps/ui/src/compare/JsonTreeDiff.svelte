<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Two JSON bodies as one tree, side by side (wave 14, strand H, `jsontree.ts`): changed values
     marked word by word, added and removed ones on their side only, unchanged objects and
     arrays folded with their size. A fold toggle sits on each object and array. -->
<script lang="ts">
  import { diffTree, initiallyOpen, treeLines, type TreeLine } from "./jsontree";
  import Words from "./Words.svelte";
  import { wordDiff } from "./worddiff";

  let {
    old,
    now,
    hasOld = true,
    hasNew = true,
  }: { old: unknown; now: unknown; hasOld?: boolean; hasNew?: boolean } = $props();

  const tree = $derived(diffTree(old, now, hasOld, hasNew));
  let open = $state(new Set<string>());
  $effect.pre(() => {
    open = initiallyOpen(tree);
  });
  const lines = $derived(treeLines(tree, (p) => open.has(p)));

  function toggle(l: TreeLine) {
    const next = new Set(open);
    if (next.has(l.pointer)) next.delete(l.pointer);
    else next.add(l.pointer);
    open = next;
  }

  function all(on: boolean) {
    if (!on) {
      open = new Set([""]);
      return;
    }
    const next = new Set<string>();
    const walk = (n: typeof tree) => {
      if (n.container && next.size < 2000) {
        next.add(n.pointer);
        n.children.forEach(walk);
      }
    };
    walk(tree);
    open = next;
  }

  const keyText = (l: TreeLine) => (l.key === null || l.closing ? "" : /^\d+$/.test(l.key) && l.depth > 0 ? `${l.key}: ` : `"${l.key}": `);
</script>

<div class="tree" data-testid="cmp-json">
  <div class="tools">
    <span class="count">{tree.changes === 0 ? "Same JSON" : `${tree.changes} ${tree.changes === 1 ? "value differs" : "values differ"}`}</span>
    <button type="button" class="link" onclick={() => all(true)}>Unfold all</button>
    <button type="button" class="link" onclick={() => all(false)}>Fold all</button>
  </div>
  <div class="grid" role="tree" aria-label="Response body, before and after">
    {#each lines as l (l.pointer)}
      <div class="line {l.kind}" role="treeitem" aria-selected="false" aria-expanded={l.container && !l.closing ? l.open : undefined} data-pointer={l.pointer} data-kind={l.kind}>
        {#each ["old", "new"] as const as side (side)}
          {@const t = side === "old" ? l.old : l.new}
          <div class="cell {side}" class:blank={t === null} class:hit={t !== null && l.kind !== "same" && (l.container === null || !l.open) && !l.closing} style="padding-left: {l.depth * 14 + 20}px">
            {#if t !== null}
              {#if l.container && !l.closing}
                <button type="button" class="fold" aria-label={l.open ? "Fold" : "Unfold"} onclick={() => toggle(l)}>{l.open ? "▾" : "▸"}</button>
              {/if}
              <span class="key">{keyText(l)}</span>{#if l.kind === "changed" && l.container === null && l.old !== null && l.new !== null}<Words segments={wordDiff(l.old, l.new)[side]} {side} />{:else}<span class="val" class:summary={l.container && !l.open}>{t}</span>{/if}{#if l.container && !l.open && l.changes > 0 && l.kind === "changed"}<span class="badge">{l.changes}</span>{/if}
            {/if}
          </div>
        {/each}
      </div>
    {/each}
  </div>
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .tools {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-size: var(--rt-size-small);
  }
  .count {
    color: var(--rt-fg-muted);
    margin-right: auto;
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: var(--rt-accent);
    cursor: pointer;
    font-size: var(--rt-size-small);
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    line-height: 1.6;
    overflow: hidden;
  }
  .line {
    display: contents;
  }
  .cell {
    position: relative;
    min-width: 0;
    padding-right: 8px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cell.old {
    border-right: 1px solid var(--rt-border);
  }
  .cell.blank {
    background: var(--rt-bg-panel);
  }
  .cell.old.hit {
    background: var(--rt-diff-del-bg);
  }
  .cell.new.hit {
    background: var(--rt-diff-add-bg);
  }
  .fold {
    position: absolute;
    margin-left: -16px;
    width: 14px;
    border: 0;
    padding: 0;
    background: none;
    color: var(--rt-fg-muted);
    cursor: pointer;
    font: inherit;
  }
  .key {
    color: var(--rt-accent);
  }
  .summary {
    color: var(--rt-fg-muted);
  }
  .badge {
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--rt-bg-selected);
    font-family: var(--rt-font-ui);
    font-size: 11px;
  }
</style>
