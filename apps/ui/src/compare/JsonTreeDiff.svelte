<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Two JSON bodies as one tree, side by side (wave 14, strand H, `jsontree.ts`): changed values
     marked word by word, added and removed ones on their side only, unchanged objects and
     arrays folded with their size. A fold toggle sits on each object and array. -->
<script lang="ts">
  import { Badge, Button, IconButton } from "../ui";
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
    <Button size="sm" variant="ghost" icon="chevron-down" onclick={() => all(true)}>Unfold all</Button>
    <Button size="sm" variant="ghost" icon="chevron-right" onclick={() => all(false)}>Fold all</Button>
  </div>
  <div class="grid" role="tree" aria-label="Response body, before and after">
    <div class="side-head">Before</div>
    <div class="side-head">After</div>
    {#each lines as l (l.pointer)}
      <div class="line {l.kind}" role="treeitem" aria-selected="false" aria-expanded={l.container && !l.closing ? l.open : undefined} data-pointer={l.pointer} data-kind={l.kind}>
        {#each ["old", "new"] as const as side (side)}
          {@const t = side === "old" ? l.old : l.new}
          <div class="cell {side}" class:blank={t === null} class:hit={t !== null && l.kind !== "same" && (l.container === null || !l.open) && !l.closing} style="--depth: {l.depth}">
            {#if t !== null}
              {#if l.container && !l.closing}
                <span class="fold"><IconButton size="sm" icon={l.open ? "chevron-down" : "chevron-right"} label={l.open ? "Fold" : "Unfold"} tooltipPlacement="left" onclick={() => toggle(l)} /></span>
              {/if}
              <span class="key">{keyText(l)}</span>{#if l.kind === "changed" && l.container === null && l.old !== null && l.new !== null}<Words segments={wordDiff(l.old, l.new)[side]} {side} />{:else}<span class="val" class:summary={l.container && !l.open}>{t}</span>{/if}{#if l.container && !l.open && l.changes > 0 && l.kind === "changed"}<span class="badge"><Badge kind="count" tone="warn" value={l.changes} title="{l.changes} values differ inside" /></span>{/if}
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
    gap: var(--rt-space-3);
  }
  .tools {
    display: flex;
    gap: var(--rt-space-2);
    align-items: center;
  }
  .count {
    margin-right: auto;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-2);
    box-shadow: var(--rt-elev-1);
    font-family: var(--rt-font-mono);
    font-size: var(--rt-text-small);
    line-height: var(--rt-control-sm);
    overflow: hidden;
  }
  .side-head {
    padding: var(--rt-space-2) var(--rt-space-5);
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-surface-sunken);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .side-head:first-child {
    border-right: 1px solid var(--rt-border);
  }
  .line {
    display: contents;
  }
  .cell {
    position: relative;
    min-width: 0;
    padding: 0 var(--rt-space-4) 0 calc(var(--depth) * var(--rt-space-6) + var(--rt-control-sm) + var(--rt-space-2));
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cell.old {
    border-right: 1px solid var(--rt-border);
  }
  .cell.blank {
    background: repeating-linear-gradient(
      -45deg,
      var(--rt-surface-sunken) 0 var(--rt-space-2),
      transparent var(--rt-space-2) var(--rt-space-4)
    );
  }
  .cell.old.hit {
    background: var(--rt-diff-del-bg);
    box-shadow: inset 2px 0 0 var(--rt-danger-border);
  }
  .cell.new.hit {
    background: var(--rt-diff-add-bg);
    box-shadow: inset 2px 0 0 var(--rt-ok-border);
  }
  .fold {
    position: absolute;
    top: 0;
    margin-left: calc(-1 * (var(--rt-control-sm) + var(--rt-space-1)));
    display: inline-flex;
  }
  .key {
    color: var(--rt-accent);
  }
  .summary {
    color: var(--rt-fg-muted);
  }
  .badge {
    display: inline-flex;
    margin-left: var(--rt-space-3);
    vertical-align: middle;
  }
</style>
