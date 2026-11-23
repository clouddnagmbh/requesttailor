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
  import { createExtensionContext } from "../shell/extension-context";
  import { getExtensions } from "../shell/extension-tabs";
  import { aiFor } from "../ai/ai.svelte";
  import { getApp } from "../state/app.svelte";
  import { Badge, Button, Card, Dialog, DialogFooter, EmptyState, Icon, Kbd, TextField } from "../ui";
  import { createNew } from "./create";
  import { folderChildren, FORM_TITLES, initialForm, validate, type FormErrors, type NewForm } from "./forms";
  import { isNewIcon } from "./icons";
  import { extensionNewItems } from "./items";
  import { isNewItemKind, NEW_ITEMS, type NewItemKind } from "./kinds";
  import NewFields from "./NewFields.svelte";
  import { allTiles, filterTiles, moveTile, type MoveKey, type Tile } from "./tiles";

  /** Unused since wave 14 (the dialog replaced the sidebar's inline prompt); kept so the shell's
   *  mount point stays as it is. */
  let { onfallback: _onfallback }: { onfallback?: (folder: string) => void } = $props();

  const app = getApp();
  const provided = getExtensions();
  const ctx: ExtensionContext = provided?.ctx ?? createExtensionContext(app.backend, app);
  const desktop = app.backend.kind === "tauri";
  const ai = aiFor(app.backend);
  void ai.ensure();
  const FORM_ID = "rt-new-form";

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
  const tiles = $derived(request ? allTiles(extensionNewItems(provided), hasCollection, ai.ready) : []);
  const shown = $derived(filterTiles(tiles, query));
  const currentTile = $derived<Tile | undefined>(shown.flat[current]);
  const info = $derived(form ? NEW_ITEMS.find((i) => i.kind === form!.kind) : undefined);
  const targetName = $derived(app.collections.find((c) => c.id === target.collection)?.name ?? null);

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
  const roots = $derived<[string, string][]>([...new Map(app.collections.map((c) => [c.root, rootName(c.root)])).entries()]);

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
    error = null;
    tried = false;
    busy = false;
    form = null;
    current = filterTiles(tiles, "").best;
    if (r.kind && isNewItemKind(r.kind)) choose(r.kind);
    else if (r.kind) {
      const tile = tiles.find((t) => t.key === r.kind);
      if (tile) void pick(tile);
    }
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
    const filter = document.querySelector<HTMLInputElement>("[data-testid=new-filter]");
    filter?.focus();
    filter?.select();
  }

  function close() {
    app.closeNew();
    // The next opening starts on the tiles (the frame focuses its first field when it mounts).
    form = null;
    query = "";
    error = null;
  }

  /** Escape: from a form back to the tiles, from the tiles closed. */
  function escape() {
    if (form && !busy) void back();
    else if (!form) close();
  }

  async function submit() {
    if (!form || busy) return;
    tried = true;
    if (Object.keys(validate(form, facts)).length > 0) {
      await tick();
      document.querySelector<HTMLElement>(`#${FORM_ID} [aria-invalid="true"]`)?.focus();
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
      void tick().then(() => gridEl?.querySelector('[data-current="true"]')?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (currentTile) void pick(currentTile);
    }
  }

  function onFilterInput(e: Event) {
    query = (e.currentTarget as HTMLInputElement).value;
    current = filterTiles(tiles, query).best;
    error = null;
  }
</script>

{#if request}
  <Dialog
    title={form ? FORM_TITLES[form.kind] : "Create new"}
    description={form
      ? info?.description
      : targetName
        ? `Pick what to create in ${targetName}. Type to filter, arrows to move, Enter to choose.`
        : "Start with a collection: the folder your requests live in."}
    icon={form ? (info?.icon ?? "add") : "add"}
    size="lg"
    testid="new-dialog"
    onclose={close}
  >
    {#if !form}
      <div class="filter">
        <TextField
          size="lg"
          icon="search"
          value={query}
          oninput={onFilterInput}
          onkeydown={onFilterKeydown}
          placeholder="Filter: request, GraphQL, folder, environment…"
          aria-label="Filter what to create"
          aria-controls="rt-new-grid"
          aria-describedby="rt-new-current"
          autocomplete="off"
          spellcheck="false"
          data-testid="new-filter"
          data-autofocus
        >
          {#snippet trailing()}<Kbd keys="Enter" size="sm" />{/snippet}
        </TextField>
      </div>
      <!-- Which tile Enter takes, for screen readers (focus stays in the filter). -->
      <p class="sr-only" id="rt-new-current" aria-live="polite">
        {currentTile ? `${currentTile.title}: ${currentTile.disabled ?? currentTile.description}` : "Nothing matches."}
      </p>
      <div class="groups" id="rt-new-grid" bind:this={gridEl}>
        {#each shown.groups as g (g.group)}
          <section class="group" aria-label={g.group}>
            <h3>{g.group}</h3>
            <div class="grid">
              {#each g.tiles as tile (tile.key)}
                {@const i = shown.flat.indexOf(tile)}
                <!-- Moving the pointer moves the keyboard's choice along (not a tile that only
                     appears under a resting pointer); the card itself is the control. -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="tile"
                  class:disabled={Boolean(tile.disabled)}
                  data-tile
                  data-testid="new-tile"
                  data-key={tile.key}
                  data-current={i === current}
                  aria-disabled={tile.disabled ? "true" : undefined}
                  onmousemove={() => current !== i && (current = i)}
                >
                  <Card
                    icon={isNewIcon(tile.icon) ? tile.icon : "spark"}
                    title={tile.title}
                    description={tile.disabled ?? tile.description}
                    padding="sm"
                    selected={i === current}
                    onclick={() => pick(tile)}
                  >
                    {#snippet actions()}
                      {#if tile.badge}<span data-testid="new-tile-badge"><Badge value={tile.badge} tone="accent" solid /></span>{/if}
                    {/snippet}
                  </Card>
                </div>
              {/each}
            </div>
          </section>
        {:else}
          <div data-testid="new-empty">
            <EmptyState icon="search" size="sm" title="Nothing matches “{query}”" text="Try “request”, “folder”, “environment” or “MCP”." />
          </div>
        {/each}
      </div>
      {#if error}<p class="error" role="alert" data-testid="new-error"><Icon name="error" size="sm" />{error}</p>{/if}
      <DialogFooter>
        {#snippet start()}
          <span class="where">
            <Icon name="collection" size="sm" />
            {#if targetName}{targetName}{#if target.folder}<Icon name="chevron-right" size="sm" />{target.folder}{/if}{:else}No collection open{/if}
          </span>
        {/snippet}
        <span class="keys">
          <span><Kbd keys="ArrowUp" size="sm" /><Kbd keys="ArrowDown" size="sm" /><Kbd keys="ArrowLeft" size="sm" /><Kbd keys="ArrowRight" size="sm" /> move</span>
          <span><Kbd keys="Enter" size="sm" /> choose</span>
          <span><Kbd keys="Escape" size="sm" /> close</span>
        </span>
      </DialogFooter>
    {:else}
      <form
        id={FORM_ID}
        data-testid="new-form"
        data-kind={form.kind}
        novalidate
        onsubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <NewFields bind:form {errors} {desktop} {roots} />
      </form>
      {#if error}<p class="error form-error" role="alert" data-testid="new-error"><Icon name="error" size="sm" />{error}</p>{/if}
      <DialogFooter>
        {#snippet start()}
          <Button variant="ghost" icon="chevron-left" onclick={back} data-testid="new-back">All kinds</Button>
        {/snippet}
        <span class="keys"><Kbd keys="Mod+Enter" size="sm" /> create</span>
        <Button onclick={close} data-testid="new-cancel">Cancel</Button>
        <Button variant="primary" type="submit" form={FORM_ID} loading={busy} data-testid="new-create">
          {form.kind === "collection" && form.place === "elsewhere" ? "Choose folder and create" : "Create"}
        </Button>
      </DialogFooter>
    {/if}
  </Dialog>
{/if}

<style>
  /* The filter stays in view while the tiles scroll under it (the frame's body scrolls). */
  .filter {
    position: sticky;
    top: calc(-1 * var(--rt-space-5));
    z-index: 1;
    margin: calc(-1 * var(--rt-space-5)) 0 0;
    padding: var(--rt-space-5) 0 var(--rt-space-5);
    background: var(--rt-surface-3);
  }
  /* A steady height while filtering, so the dialog does not jump as tiles come and go. */
  .groups {
    display: flex;
    flex-direction: column;
    justify-content: flex-start;
    gap: var(--rt-space-7);
    min-height: min(420px, calc(100vh - 340px));
    padding-top: var(--rt-space-2);
  }
  .groups > [data-testid="new-empty"] {
    margin: auto 0;
  }
  h3 {
    margin: 0 0 var(--rt-space-4);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    letter-spacing: var(--rt-tracking-caps);
    text-transform: uppercase;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(208px, 1fr));
    gap: var(--rt-space-5);
  }
  .tile {
    display: flex;
    min-width: 0;
  }
  .tile > :global(.rt-card) {
    flex: 1;
  }
  .tile.disabled {
    opacity: 0.55;
  }
  .error {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    margin: var(--rt-space-5) 0 0;
    padding: var(--rt-space-4) var(--rt-space-5);
    border: 1px solid var(--rt-danger-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-danger-bg);
    color: var(--rt-danger-fg);
    font: var(--rt-font-small);
    overflow-wrap: anywhere;
  }
  .form-error {
    margin-top: var(--rt-space-6);
  }
  .where {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
    min-width: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .keys {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-5);
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
    white-space: nowrap;
  }
  .keys > span {
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
