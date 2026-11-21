<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- App layout: header, resizable sidebar with the folder tree, tabs, request editor above the
     response (resizable split), command palette and global shortcuts. Owner: wave 3, strand A.
     Composes the other strands' components through their props only. -->
<script lang="ts">
  import { tick } from "svelte";

  import type { Backend } from "../commands/backend";
  import RequestEditor from "../editor/RequestEditor.svelte";
  import EnvironmentManager from "../environments/EnvironmentManager.svelte";
  import EnvironmentSwitcher from "../environments/EnvironmentSwitcher.svelte";
  import ResponseView from "../response/ResponseView.svelte";
  import { AppState, provideApp } from "../state/app.svelte";
  import "../styles/base.css";
  import CompanionOffer from "../transport/CompanionOffer.svelte";
  import CommandPalette from "./CommandPalette.svelte";
  import { buildCommands, LABELS, type Actions } from "./commands";
  import Resizer from "./Resizer.svelte";
  import { formatShortcut, isMacPlatform, matchShortcut, shortcutFor, SHORTCUTS, type CommandId } from "./shortcuts";
  import Sidebar from "./Sidebar.svelte";
  import { isNumber, load, store } from "./storage";
  import TabBar from "./TabBar.svelte";
  import { Theme } from "./theme.svelte";
  import { requestPaths, tabElementId } from "./tree";

  let { backend }: { backend: Backend } = $props();
  // The backend is fixed for the lifetime of the shell.
  // svelte-ignore state_referenced_locally
  const app = provideApp(new AppState(backend));
  const theme = new Theme();
  const mac = isMacPlatform();
  const keys = (id: CommandId) => {
    const s = shortcutFor(id);
    return s ? formatShortcut(s, mac) : "";
  };

  let managing = $state(false);
  let innerWidth = $state(1024);
  let paletteOpen = $state(false);
  /** Folder the inline "new request" prompt is open for (`""` = root). */
  let newIn = $state<string | null>(null);
  let sidebar = $state<ReturnType<typeof Sidebar>>();

  // ---- panel sizes (per device) ----
  const SIDEBAR_MIN = 160;
  let sidebarWidth = $state(load("sidebarWidth", 260, isNumber));
  let split = $state(load("split", 0.5, isNumber));
  let panelHeight = $state(0);
  const editorHeight = $derived(Math.round(panelHeight * split));
  function resizeSidebar(px: number) {
    sidebarWidth = px;
    store("sidebarWidth", px);
  }
  function resizeSplit(px: number) {
    if (panelHeight <= 0) return;
    split = px / panelHeight;
    store("split", split);
  }

  // ---- last open tabs per collection (per device) ----
  interface SavedTabs {
    tabs: string[];
    active: string | null;
  }
  const isSavedTabs = (v: unknown): v is SavedTabs =>
    typeof v === "object" &&
    v !== null &&
    Array.isArray((v as SavedTabs).tabs) &&
    (v as SavedTabs).tabs.every((t) => typeof t === "string") &&
    (typeof (v as SavedTabs).active === "string" || (v as SavedTabs).active === null);
  let restoring = false;

  $effect(() => {
    const id = app.collection?.id;
    const saved: SavedTabs = { tabs: [...app.tabs], active: app.active };
    if (id && !restoring) store(`tabs:${id}`, saved);
  });

  async function restoreTabs(id: string) {
    const saved = load<SavedTabs | null>(`tabs:${id}`, null, isSavedTabs);
    if (!saved) return;
    const exists = new Set(requestPaths(app.tree));
    for (const path of saved.tabs) if (exists.has(path)) await app.select(path);
    if (saved.active && app.tabs.includes(saved.active)) await app.select(saved.active);
  }

  async function openFolder() {
    const before = app.collection;
    restoring = true;
    try {
      await app.open();
      if (app.collection && app.collection !== before) await restoreTabs(app.collection.id);
    } finally {
      restoring = false;
    }
    if (app.collection && app.collection !== before) {
      await tick();
      await sidebar?.focus();
    }
  }

  // ---- commands and shortcuts ----
  const actions: Actions = {
    "open-folder": openFolder,
    "new-request": () => {
      if (app.collection) newIn = "";
    },
    send: () => app.send(),
    save: () => app.save(),
    "close-tab": () => {
      if (app.active) app.close(app.active);
    },
    palette: () => (paletteOpen = !paletteOpen),
    "toggle-theme": () => theme.toggle(),
    "focus-sidebar": () => sidebar?.focus(),
  };
  const commands = $derived(buildCommands(app, actions, mac));

  // Capture phase: runs before inputs and code editors handle the key, so Mod+Enter and Mod+S
  // work wherever focus is.
  $effect(() => {
    const onkeydown = (e: KeyboardEvent) => {
      const s = matchShortcut(e, mac);
      if (!s) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat) return;
      if (s.command !== "palette") paletteOpen = false;
      actions[s.command]();
    };
    window.addEventListener("keydown", onkeydown, true);
    return () => window.removeEventListener("keydown", onkeydown, true);
  });

  const themeLabel = { system: "System", light: "Light", dark: "Dark" } as const;
  const themeIcon = { system: "◐", light: "☀", dark: "☾" } as const;
  const sending = $derived(app.active ? (app.sending[app.active] ?? false) : false);
  const activeTabId = $derived(app.active ? tabElementId(app.tabs.indexOf(app.active)) : undefined);
</script>

<div class="shell">
  <header>
    <button class="button" data-testid="open-collection" title="Open folder ({keys('open-folder')})" onclick={openFolder}>
      Open folder
    </button>
    <span class="collection" title={app.collection?.location ?? ""}>
      <span data-testid="collection-name">{app.collection?.name ?? ""}</span>
    </span>
    <span class="spacer"></span>
    {#if app.collection}
      <EnvironmentSwitcher />
      <button class="button" onclick={() => (managing = true)}>Environments</button>
    {/if}
    <button
      class="button primary"
      data-testid="save"
      title="Save all ({keys('save')})"
      onclick={() => app.save()}
      disabled={app.dirty.length === 0}>Save</button
    >
    <button
      class="button"
      data-testid="palette-button"
      aria-label="Command palette"
      title="Command palette ({keys('palette')})"
      onclick={() => (paletteOpen = true)}>{keys("palette")}</button
    >
    <button
      class="button"
      data-testid="theme-toggle"
      aria-label="Theme: {theme.preference}"
      title="Theme: {themeLabel[theme.preference]} (click to change)"
      onclick={() => theme.cycle()}
    >
      <span aria-hidden="true">{themeIcon[theme.preference]}</span>
      {themeLabel[theme.preference]}
    </button>
  </header>

  <div class="body">
    <div class="side" style:width="{sidebarWidth}px">
      <Sidebar bind:this={sidebar} bind:newIn hint={keys("new-request")} />
    </div>
    <Resizer
      label="Resize sidebar"
      orientation="vertical"
      value={sidebarWidth}
      min={SIDEBAR_MIN}
      max={Math.max(SIDEBAR_MIN, Math.round(innerWidth * 0.6))}
      onresize={resizeSidebar}
      testid="sidebar-resizer"
    />

    <main>
      <TabBar panelId="rt-request-panel" />
      {#if app.active}
        <div class="panel" id="rt-request-panel" role="tabpanel" aria-labelledby={activeTabId} bind:clientHeight={panelHeight}>
          <div class="pane editor" style:height="{editorHeight}px">
            <div class="toolbar">
              <button
                class="button primary"
                data-testid="send"
                title="Send ({keys('send')})"
                disabled={sending}
                onclick={() => app.send()}>{sending ? "Sending…" : "Send"}</button
              >
              <kbd>{keys("send")}</kbd>
            </div>
            <RequestEditor path={app.active} />
          </div>
          <Resizer
            label="Resize request and response"
            orientation="horizontal"
            value={editorHeight}
            min={Math.round(panelHeight * 0.15)}
            max={Math.round(panelHeight * 0.85)}
            onresize={resizeSplit}
            testid="split-resizer"
          />
          <div class="pane response">
            <CompanionOffer path={app.active} />
            <ResponseView path={app.active} />
          </div>
        </div>
      {:else}
        <div class="start" data-testid="start">
          <p>{app.collection ? "Open a request from the sidebar." : "Open a folder with a collection to start."}</p>
          <dl>
            {#each SHORTCUTS as s (s.command)}
              <dt><kbd>{formatShortcut(s, mac)}</kbd></dt>
              <dd>{LABELS[s.command]}</dd>
            {/each}
          </dl>
        </div>
      {/if}
    </main>
  </div>

  <footer>
    {#if app.error}<p class="error" role="alert">{app.error}</p>{/if}
  </footer>
</div>

<CommandPalette bind:open={paletteOpen} {commands} />
<EnvironmentManager open={managing} onclose={() => (managing = false)} />

<svelte:window bind:innerWidth />

<style>
  .shell {
    position: fixed;
    inset: 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: 6px 10px;
    border-bottom: 1px solid var(--rt-border);
    background: var(--rt-bg-panel);
  }
  .collection {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
    cursor: pointer;
    white-space: nowrap;
  }
  .button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  .button:disabled {
    color: var(--rt-fg-muted);
    cursor: default;
    opacity: 0.6;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  .body {
    display: flex;
    min-height: 0;
  }
  .side {
    flex: none;
    min-width: 0;
    max-width: 60vw;
  }
  main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pane {
    min-height: 0;
    overflow: auto;
  }
  .editor {
    flex: none;
  }
  .response {
    flex: 1;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) 12px 0;
  }
  .toolbar kbd {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .start {
    margin: auto;
    color: var(--rt-fg-muted);
    text-align: center;
  }
  .start dl {
    display: grid;
    grid-template-columns: auto auto;
    gap: 6px 16px;
    justify-content: center;
    text-align: left;
  }
  .start dt,
  .start dd {
    margin: 0;
  }
  .start kbd {
    padding: 1px 6px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    color: var(--rt-fg);
  }
  footer .error {
    margin: 0;
    padding: 4px 12px;
    border-top: 1px solid var(--rt-border);
    color: var(--rt-danger);
    background: var(--rt-bg-panel);
  }
</style>
