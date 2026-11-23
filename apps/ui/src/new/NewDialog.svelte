<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The "New…" dialog (wave 14, strand B): tiles for every built-in kind (./kinds.ts) and the
     tiles of extensions (./items.ts), grouped; then a short form per kind. Opened with
     `AppState.openNew` (Mod+N, palette, sidebar, a node's context menu).
     Keyboard first: typing filters the tiles, the arrow keys move on the grid, Enter chooses;
     in a form Enter or Mod+Enter creates; Escape goes back to the tiles, then closes.
     Everything it creates is unsaved, like any edit. -->
<script module lang="ts">
  import { isMacPlatform, matchShortcut } from "../shell/shortcuts";

  /** The open dialog's handlers of Mod+Enter and Escape. */
  let owner: { create: () => void; escape: () => void } | null = null;

  // The shell takes its shortcuts on `window` in the capture phase. This listener is added when
  // the module loads, before the shell mounts, so it runs first: while the dialog is open it owns
  // the shortcuts (Mod+Enter creates; the others would act on the page behind the modal), except
  // Save. It also takes Escape wherever focus is, so the browser's own cancel (which closes the
  // dialog outright after a second Escape) never runs.
  const KEY = Symbol.for("requesttailor.new-dialog.keys");
  const w = typeof window === "undefined" ? null : (window as unknown as Record<symbol, boolean>);
  if (w && !w[KEY]) {
    w[KEY] = true;
    const mac = isMacPlatform();
    window.addEventListener(
      "keydown",
      (e) => {
        if (!owner) return;
        if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
          e.preventDefault();
          e.stopImmediatePropagation();
          if (!e.repeat) owner.escape();
          return;
        }
        const s = matchShortcut(e, mac);
        if (!s || s.command === "save") return;
        e.preventDefault();
        e.stopImmediatePropagation();
        if (s.command === "send" && !e.repeat) owner.create();
      },
      true,
    );
  }
</script>

<script lang="ts">
  import { tick } from "svelte";

  import type { ExtensionContext, NewTarget } from "../extensions";
  import { getExtensions } from "../shell/extension-tabs";
  import { getApp } from "../state/app.svelte";
  import { createNew } from "./create";
  import { folderChildren, initialForm, validate, type FormErrors, type NewForm } from "./forms";
  import { extensionNewItems } from "./items";
  import { isNewItemKind, NEW_ITEMS, type NewItemKind } from "./kinds";
  import NewFields from "./NewFields.svelte";
  import TileIcon from "./TileIcon.svelte";
  import { allTiles, filterTiles, moveTile, type MoveKey, type Tile } from "./tiles";

  /** Unused since wave 14 (the dialog replaced the sidebar's inline prompt); kept so the shell's
   *  mount point stays as it is. */
  let { onfallback: _onfallback }: { onfallback?: (folder: string) => void } = $props();

  const app = getApp();
  const provided = getExtensions();
  const ctx: ExtensionContext = provided?.ctx ?? { backend: app.backend, app };
  const mac = isMacPlatform();
  const desktop = app.backend.kind === "tauri";

  let dialogEl = $state<HTMLDialogElement>();
  let gridEl = $state<HTMLElement>();
  let query = $state("");
  let current = $state(0);
  let form = $state<NewForm | null>(null);
  /** Shown after a failed Create; field errors show once Create was tried. */
  let error = $state<string | null>(null);
  let tried = $state(false);
  let busy = $state(false);

  const request = $derived(app.newDialog);
  const target = $derived<NewTarget>({ collection: request?.collection ?? null, folder: request?.folder ?? "" });
  const hasCollection = $derived(app.collections.length > 0);
  // Asked only while open (extensions may read state, never write it).
  const tiles = $derived(request ? allTiles(extensionNewItems(provided), hasCollection) : []);
  const shown = $derived(filterTiles(tiles, query));
  const currentTile = $derived<Tile | undefined>(shown.flat[current]);
  const info = $derived(form ? NEW_ITEMS.find((i) => i.kind === form!.kind) : undefined);

  const facts = $derived.by(() => {
    if (!form) return {};
    const active = "collection" in form && form.collection === app.activeCollection;
    return {
      collections: app.collections,
      environments: active ? app.environments.map((e) => e.name) : undefined,
      siblings: form.kind === "folder" && form.collection ? folderChildren(app.trees[form.collection], form.folder) : undefined,
    };
  });
  const errors = $derived<FormErrors>(form && tried ? validate(form, facts) : {});
  const roots = $derived([...new Map(app.collections.map((c) => [c.root, rootName(c.root)])).entries()]);

  function rootName(root: string): string {
    // The root's own collection, if it is one, else the folder the collections were found in.
    const own = app.collections.find((c) => c.root === root && c.dir === "");
    if (own) return own.name;
    const any = app.collections.find((c) => c.root === root);
    const loc = any?.location ?? root;
    const dir = any?.dir ?? "";
    const parent = dir && loc.endsWith(dir) ? loc.slice(0, -dir.length).replace(/[\\/]+$/, "") : loc;
    return parent.split(/[\\/]/).pop() || loc;
  }

  // Each `openNew` starts over: on the tiles, or on the form of the kind asked for.
  let seen = -1;
  $effect(() => {
    const r = request;
    if (!r || r.seq === seen) return;
    seen = r.seq;
    query = "";
    current = 0;
    error = null;
    tried = false;
    busy = false;
    form = null;
    if (r.kind && isNewItemKind(r.kind)) choose(r.kind);
    else if (r.kind) {
      const tile = tiles.find((t) => t.key === r.kind);
      if (tile) void pick(tile);
    }
    void tick().then(() => {
      if (!dialogEl || dialogEl.open || !app.newDialog) return;
      dialogEl.showModal();
      // showModal focuses the first control (the close button); start where typing belongs.
      const first = dialogEl.querySelector<HTMLInputElement>("[data-autofocus]");
      first?.focus();
      first?.select();
    });
  });

  // While open, Mod+Enter and Escape come here (the module's listener hands them over).
  $effect(() => {
    if (!request) return;
    owner = {
      create: () => {
        if (form) void submit();
        else if (currentTile) void pick(currentTile);
      },
      escape,
    };
    return () => {
      owner = null;
    };
  });

  function choose(kind: NewItemKind) {
    form = initialForm(kind, { collection: target.collection, folder: target.folder, collections: app.collections, desktop });
    error = null;
    tried = false;
  }

  async function pick(tile: Tile) {
    if (tile.disabled) {
      error = tile.disabled;
      return;
    }
    if (tile.builtin) return choose(tile.builtin);
    const ref = tile.extension;
    const at = target;
    close();
    await tick();
    try {
      await ref?.item.run(ctx, at);
    } catch (e) {
      console.warn(`RequestTailor: new item ${tile.key} failed:`, e);
      app.error = e instanceof Error ? e.message : String(e);
    }
  }

  async function back() {
    form = null;
    error = null;
    await tick();
    dialogEl?.querySelector<HTMLInputElement>("[data-testid=new-filter]")?.focus();
  }

  function close() {
    dialogEl?.close();
    app.closeNew();
  }

  async function submit() {
    if (!form || busy) return;
    tried = true;
    if (Object.keys(validate(form, facts)).length > 0) {
      await tick();
      dialogEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
      return;
    }
    busy = true;
    error = null;
    const outcome = await createNew(app, $state.snapshot(form) as NewForm);
    busy = false;
    if (outcome.ok) close();
    else if (!("cancelled" in outcome)) error = outcome.error;
  }

  const MOVES = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"]);

  function onFilterKeydown(e: KeyboardEvent) {
    if (MOVES.has(e.key) && !e.altKey && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      const boxes = [...(gridEl?.querySelectorAll<HTMLElement>("[data-tile]") ?? [])].map((el) => el.getBoundingClientRect());
      current = moveTile(boxes, current, e.key as MoveKey);
      e.preventDefault();
      void tick().then(() => gridEl?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (currentTile) void pick(currentTile);
    }
  }

  /** Escape: from a form back to the tiles, from the tiles closed. */
  function escape() {
    if (form && !busy) void back();
    else if (!form) close();
  }

  function onFilterInput(e: Event) {
    query = (e.currentTarget as HTMLInputElement).value;
    current = filterTiles(tiles, query).best;
  }

  const autofocus = (el: HTMLInputElement) => {
    el.focus();
  };

  const modEnter = mac ? "⌘↩" : "Ctrl+Enter";
  const tileId = (key: string) => `rt-new-tile-${key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const collectionName = (id: string | null) => app.collections.find((c) => c.id === id)?.name ?? null;
</script>

{#if request}
  <dialog
    bind:this={dialogEl}
    class="new-dialog"
    data-testid="new-dialog"
    aria-labelledby="rt-new-title"
    oncancel={(e) => {
      e.preventDefault();
      escape();
    }}
    onclose={(e) => {
      // The browser closed it by itself. A `close` of a dialog closed before (and gone since)
      // arrives later, possibly after the next `openNew`: only this element's own counts.
      if (e.currentTarget === dialogEl && app.newDialog) app.closeNew();
    }}
  >
    {#if !form}
      <header class="head">
        <div class="titles">
          <h2 id="rt-new-title">Create new</h2>
          <p class="sub">
            {collectionName(target.collection) ? `Pick what to create in ${collectionName(target.collection)}.` : "Start with a collection, the folder your requests live in."}
          </p>
        </div>
        <button type="button" class="close" aria-label="Close" data-testid="new-close" onclick={close}>×</button>
      </header>
      <div class="filter">
        <input
          data-testid="new-filter"
          data-autofocus
          type="search"
          role="combobox"
          aria-label="Filter"
          aria-expanded="true"
          aria-controls="rt-new-grid"
          aria-activedescendant={currentTile ? tileId(currentTile.key) : undefined}
          placeholder="Type to filter: request, folder, environment…"
          value={query}
          oninput={onFilterInput}
          onkeydown={onFilterKeydown}
          autocomplete="off"
          spellcheck="false"
          {@attach autofocus}
        />
      </div>
      <div class="body" id="rt-new-grid" role="listbox" aria-label="What to create" bind:this={gridEl}>
        {#each shown.groups as g (g.group)}
          <div class="group" role="group" aria-label={g.group}>
            <h3>{g.group}</h3>
            <div class="grid">
              {#each g.tiles as tile (tile.key)}
                {@const i = shown.flat.indexOf(tile)}
                <div
                  id={tileId(tile.key)}
                  class="tile"
                  class:current={i === current}
                  role="option"
                  tabindex="-1"
                  aria-selected={i === current}
                  aria-disabled={tile.disabled ? "true" : undefined}
                  title={tile.disabled}
                  data-tile
                  data-testid="new-tile"
                  data-key={tile.key}
                  onmouseenter={() => (current = i)}
                  onclick={() => pick(tile)}
                  onkeydown={(e) => e.key === "Enter" && pick(tile)}
                >
                  <TileIcon name={tile.icon} kind={tile.builtin ?? null} />
                  <span class="text">
                    <span class="name">{tile.title}</span>
                    <span class="desc">{tile.description}</span>
                  </span>
                  {#if tile.badge}<span class="badge" data-testid="new-tile-badge">{tile.badge}</span>{/if}
                </div>
              {/each}
            </div>
          </div>
        {:else}
          <p class="empty" data-testid="new-empty">Nothing matches “{query}”. Try “request”, “folder” or “environment”.</p>
        {/each}
      </div>
      {#if error}<p class="error" role="alert" data-testid="new-error">{error}</p>{/if}
      <footer class="foot">
        <span class="keys"><kbd>↑</kbd><kbd>↓</kbd><kbd>←</kbd><kbd>→</kbd> move · <kbd>↩</kbd> choose · <kbd>Esc</kbd> close</span>
      </footer>
    {:else}
      <form
        class="form"
        data-testid="new-form"
        data-kind={form.kind}
        novalidate
        onsubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <header class="head">
          <button type="button" class="back" aria-label="Back to all kinds" data-testid="new-back" onclick={back}>←</button>
          <TileIcon name={info?.icon ?? ""} kind={form.kind} />
          <div class="titles">
            <h2 id="rt-new-title">New {info?.title ?? ""}</h2>
            <p class="sub">{info?.description}</p>
          </div>
          <button type="button" class="close" aria-label="Close" onclick={close}>×</button>
        </header>
        <div class="body">
          <NewFields bind:form {errors} {desktop} pickRoot={roots} />
        </div>
        {#if error}<p class="error" role="alert" data-testid="new-error">{error}</p>{/if}
        <footer class="foot">
          <span class="keys"><kbd>Esc</kbd> back</span>
          <span class="spacer"></span>
          <button type="button" data-testid="new-cancel" onclick={close}>Cancel</button>
          <button type="submit" class="primary" data-testid="new-create" disabled={busy}>
            {busy ? "Creating…" : form.kind === "collection" && form.place === "elsewhere" ? "Choose folder and create" : "Create"}
            <kbd>{modEnter}</kbd>
          </button>
        </footer>
      </form>
    {/if}
  </dialog>
{/if}

<style>
  /* Structure only; the visual pass follows with the shared design primitives. */
  .new-dialog {
    width: min(760px, calc(100vw - 32px));
    max-height: min(640px, calc(100vh - 48px));
    box-sizing: border-box;
    padding: 0;
    flex-direction: column;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    box-shadow: var(--rt-shadow);
    overflow: hidden;
  }
  .new-dialog[open] {
    display: flex;
  }
  .new-dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  .form {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-gap);
    padding: 16px 16px 8px;
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  .sub {
    margin: 2px 0 0;
    color: var(--rt-fg-muted);
  }
  .close,
  .back {
    flex: none;
    background: none;
    border: 0;
    color: var(--rt-fg-muted);
    font-size: 18px;
    cursor: pointer;
  }
  .filter {
    padding: 0 16px 8px;
  }
  .filter input {
    width: 100%;
    box-sizing: border-box;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 8px 16px 16px;
  }
  .group + .group {
    margin-top: 12px;
  }
  h3 {
    margin: 0 0 6px;
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--rt-gap);
  }
  .tile {
    position: relative;
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
    min-width: 0;
  }
  .tile.current {
    border-color: var(--rt-accent);
    background: var(--rt-bg-selected);
  }
  .tile[aria-disabled="true"] {
    opacity: 0.5;
    cursor: default;
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 600;
  }
  .desc {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .badge {
    position: absolute;
    top: 6px;
    right: 6px;
    font-size: 11px;
    padding: 0 6px;
    border-radius: 999px;
    border: 1px solid var(--rt-accent);
    color: var(--rt-accent);
  }
  .empty {
    color: var(--rt-fg-muted);
    text-align: center;
    padding: 24px 0;
    margin: 0;
  }
  .error {
    margin: 0;
    padding: 0 16px 8px;
    color: var(--rt-danger);
    overflow-wrap: anywhere;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: 10px 16px;
    border-top: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }
  .keys {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .spacer {
    flex: 1;
  }
</style>
