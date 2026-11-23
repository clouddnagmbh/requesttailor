<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Picks one side of a comparison (wave 14, strand H): a field that takes any revision as
     typed (`HEAD~2`, a commit id) and a list of branches, remote branches, tags and recent
     commits, each on its lane like in the commit graph. Keyboard: typing filters, ↑/↓ move,
     Enter picks (the typed text when nothing is highlighted), Esc closes. -->
<script lang="ts">
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
  let input = $state<HTMLInputElement | null>(null);

  const shown = $derived(filterOptions(options, query));
  const lanes = $derived(Math.max(1, ...options.map((o) => o.lane)));
  const groups = $derived(
    (["branch", "remote", "tag", "commit"] as RevisionKind[])
      .map((kind) => ({ kind, items: shown.filter((o) => o.kind === kind) }))
      .filter((g) => g.items.length > 0),
  );
  const flat = $derived(groups.flatMap((g) => g.items));
  const selected = $derived(options.find((o) => o.rev === value));

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
  <label class="field">
    <span class="label">{label}</span>
    <span class="control" class:open>
      {#if selected}
        <LaneMark lane={selected.lane} lanes={1} kind={selected.kind} single />
      {/if}
      <input
        bind:this={input}
        bind:value
        data-testid={testid}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && active >= 0 ? `${listId}-${active}` : undefined}
        autocomplete="off"
        spellcheck="false"
        placeholder="branch, tag or commit"
        {disabled}
        onfocus={show}
        onclick={() => !open && show()}
        {oninput}
        {onkeydown}
      />
      <button
        type="button"
        class="chevron"
        tabindex="-1"
        aria-label="Show revisions"
        {disabled}
        onclick={() => {
          if (open) open = false;
          else {
            input?.focus();
            show();
          }
        }}>▾</button
      >
    </span>
  </label>
  {#if open}
    <div class="popup" id={listId} role="listbox" aria-label="{label} revision" data-testid="{testid}-list">
      {#if flat.length === 0}
        <p class="empty">
          {query.trim() ? `Nothing matches. Press Enter to compare "${query.trim()}" as typed.` : "No branches, tags or commits yet."}
        </p>
      {/if}
      {#each groups as g (g.kind)}
        <div class="group" role="group" aria-label={KIND_LABEL[g.kind]}>
          <div class="group-title">{KIND_LABEL[g.kind]}</div>
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
              <span class="text">
                <span class="name" class:mono={o.kind === "commit"}>{o.label}</span>
                {#if o.current}<span class="current">current</span>{/if}
                {#if o.detail}<span class="detail">{o.detail}</span>{/if}
              </span>
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
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .label {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .control {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 0 0 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
  }
  .control:focus-within {
    border-color: var(--rt-focus);
  }
  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 0;
    outline: 0;
    background: transparent;
    font-family: var(--rt-font-mono);
  }
  .chevron {
    height: 28px;
    width: 24px;
    border: 0;
    background: transparent;
    color: var(--rt-fg-muted);
    cursor: pointer;
  }
  .popup {
    position: absolute;
    z-index: 20;
    top: calc(100% + 4px);
    left: 0;
    width: max(100%, 320px);
    max-width: min(480px, 90vw);
    max-height: 360px;
    overflow: auto;
    padding: 4px 0;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    box-shadow: var(--rt-shadow);
  }
  .group-title {
    padding: 6px 10px 2px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    min-height: 28px;
    cursor: pointer;
  }
  .option.active {
    background: var(--rt-bg-hover);
  }
  .option[aria-selected="true"] {
    background: var(--rt-bg-selected);
  }
  .text {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .name {
    white-space: nowrap;
  }
  .mono {
    font-family: var(--rt-font-mono);
  }
  .current {
    font-size: var(--rt-size-small);
    color: var(--rt-accent);
  }
  .detail {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    margin: 0;
    padding: 8px 10px;
    color: var(--rt-fg-muted);
  }
</style>
