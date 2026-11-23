<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Picks one side of a comparison (wave 14, strand H): a field that takes any revision as
     typed (`HEAD~2`, a commit id) and a list of branches, remote branches, tags and recent
     commits, each on its lane like in the commit graph. Keyboard: typing filters, ↑/↓ move,
     Enter picks (the typed text when nothing is highlighted), Esc closes. -->
<script lang="ts">
  import { IconButton, TextField } from "../ui";
  import LaneMark from "./LaneMark.svelte";
  import { filterOptions, KIND_LABEL, type RevisionKind, type RevisionOption } from "./revisions";

  let {
    label,
    value = $bindable(),
    options,
    disabled = false,
    testid,
    onpick,
  }: {
    label: string;
    value: string;
    options: RevisionOption[];
    disabled?: boolean;
    testid: string;
    onpick?: (rev: string) => void;
  } = $props();

  const listId = $derived(`${testid}-options`);
  let open = $state(false);
  let query = $state("");
  let active = $state(-1);
  let root = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | undefined>(undefined);

  const shown = $derived(filterOptions(options, query));
  const lanes = $derived(Math.max(1, ...options.map((o) => o.lane)));
  const groups = $derived(
    (["branch", "remote", "tag", "commit"] as RevisionKind[])
      .map((kind) => ({ kind, items: shown.filter((o) => o.kind === kind) }))
      .filter((g) => g.items.length > 0),
  );
  const flat = $derived(groups.flatMap((g) => g.items));
  const selected = $derived(options.find((o) => o.rev === value));
  const ICON: Record<RevisionKind, string> = { branch: "branch", remote: "branch", tag: "tag", commit: "commit" };

  function show() {
    if (disabled) return;
    open = true;
    query = "";
    active = flat.findIndex((o) => o.rev === value);
  }

  function pick(rev: string) {
    value = rev;
    open = false;
    query = "";
    onpick?.(rev);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) show();
      const n = flat.length;
      if (n > 0) active = e.key === "ArrowDown" ? (active + 1) % n : (active - 1 + n) % n;
      root?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && open) {
      e.preventDefault();
      const o = flat[active];
      pick(o ? o.rev : value.trim());
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    }
  }

  function oninput() {
    query = value;
    open = true;
    active = -1;
  }

  function onfocusout(e: FocusEvent) {
    if (root && e.relatedTarget instanceof Node && root.contains(e.relatedTarget)) return;
    open = false;
  }
</script>

<div class="picker" bind:this={root} onfocusout={onfocusout} data-testid="{testid}-picker">
  <TextField
    {label}
    bind:value
    bind:element={input}
    icon={selected ? ICON[selected.kind] : "commit"}
    mono
    data-testid={testid}
    role="combobox"
    aria-expanded={open}
    aria-controls={listId}
    aria-autocomplete="list"
    aria-activedescendant={open && active >= 0 ? `${listId}-${active}` : undefined}
    autocomplete="off"
    placeholder="Branch, tag or commit"
    {disabled}
    onfocus={show}
    onclick={() => !open && show()}
    {oninput}
    {onkeydown}
  >
    {#snippet trailing()}
      <span class="trail">
        {#if selected && selected.lane > 0}<span class="lane" style="--lane: var(--rt-lane-{selected.lane})" aria-hidden="true"></span>{/if}
        <IconButton
          icon="chevron-down"
          label="Show revisions"
          size="sm"
          tabindex={-1}
          {disabled}
          onclick={() => {
            if (open) open = false;
            else {
              input?.focus();
              show();
            }
          }}
        />
      </span>
    {/snippet}
  </TextField>
  {#if open}
    <div class="popup" id={listId} role="listbox" aria-label="{label} revision" data-testid="{testid}-list">
      {#if flat.length === 0}
        <p class="empty">
          {query.trim() ? `Nothing matches. Press Enter to compare "${query.trim()}" as typed.` : "No branches, tags or commits yet."}
        </p>
      {/if}
      {#each groups as g (g.kind)}
        <div class="group" role="group" aria-label={KIND_LABEL[g.kind]}>
          <div class="heading">{KIND_LABEL[g.kind]}</div>
          {#each g.items as o (o.kind + o.rev)}
            {@const index = flat.indexOf(o)}
            <div
              class="option"
              id="{listId}-{index}"
              role="option"
              tabindex="-1"
              aria-selected={o.rev === value}
              data-index={index}
              data-rev={o.rev}
              data-kind={o.kind}
              class:active={index === active}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => pick(o.rev)}
              onkeydown={() => {}}
              onmousemove={() => (active = index)}
            >
              <LaneMark lane={o.lane} {lanes} kind={o.kind} />
              <span class="name" class:mono={o.kind !== "branch"}>{o.label}</span>
              {#if o.current}<span class="current">current</span>{/if}
              {#if o.detail}<span class="detail">{o.detail}</span>{/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    min-width: 0;
  }
  .trail {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
    margin-right: calc(-1 * var(--rt-space-3));
  }
  .lane {
    width: var(--rt-space-4);
    height: var(--rt-space-4);
    border-radius: var(--rt-radius-pill);
    background: var(--lane);
  }
  .popup {
    position: absolute;
    z-index: var(--rt-z-popover);
    top: calc(100% + var(--rt-space-2));
    left: 0;
    box-sizing: border-box;
    width: max(100%, 320px);
    max-width: min(480px, 90vw);
    max-height: 360px;
    overflow: auto;
    padding: var(--rt-space-2);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-lg);
    background: var(--rt-surface-3);
    box-shadow: var(--rt-elev-2);
  }
  .heading {
    padding: var(--rt-space-3) var(--rt-space-4) var(--rt-space-2);
    color: var(--rt-fg-subtle);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .option {
    display: flex;
    align-items: center;
    gap: var(--rt-space-4);
    min-height: var(--rt-control-md);
    padding: 0 var(--rt-space-4);
    border-radius: var(--rt-radius-md);
    cursor: pointer;
    font: var(--rt-font-body);
  }
  .option.active {
    background: var(--rt-bg-hover);
  }
  .option[aria-selected="true"] {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .name {
    flex: none;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .current {
    flex: none;
    padding: 0 var(--rt-space-3);
    border-radius: var(--rt-radius-pill);
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
    font: var(--rt-font-caption);
  }
  .detail {
    min-width: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    margin: 0;
    padding: var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
</style>
