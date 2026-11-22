<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- App layout: header, resizable sidebar (views: requests, search, history, Git), tabs,
     request editor above the response (resizable split), command palette and global shortcuts.
     Owner: wave 3, strand A. Composes the other strands' components through their props only.
     Wave 4 mount points: GitPanel (A2), ImportDialog (B), RecentCollections and
     reopenOnStart (C), SearchPanel and HistoryPanel (E).
     Wave 5 mount points: PluginsDialog (A), AuthPromptDialog (C), CookiesDialog (D). -->
<script lang="ts">
  import { onMount, tick } from "svelte";

  import AuthPromptDialog from "../auth/AuthPromptDialog.svelte";
  import type { Backend, CollectionInfo } from "../commands/backend";
  import CookiesDialog from "../cookies/CookiesDialog.svelte";
  import RequestEditor from "../editor/RequestEditor.svelte";
  import EnvironmentManager from "../environments/EnvironmentManager.svelte";
  import EnvironmentSwitcher from "../environments/EnvironmentSwitcher.svelte";
  import GitPanel from "../git/GitPanel.svelte";
  import HistoryPanel from "../history/HistoryPanel.svelte";
  import ImportDialog from "../import/ImportDialog.svelte";
  import PluginsDialog from "../plugins/PluginsDialog.svelte";
  import RecentCollections from "../recent/RecentCollections.svelte";
  import { reopenOnStart } from "../recent/startup";
  import SearchPanel from "../search/SearchPanel.svelte";
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
  let importing = $state(false);
  let managingPlugins = $state(false);
  let showingCookies = $state(false);
  let innerWidth = $state(1024);
  let paletteOpen = $state(false);
  /** Folder the inline "new request" prompt is open for (`""` = root). */
  let newIn = $state<string | null>(null);
  let sidebar = $state<ReturnType<typeof Sidebar>>();
  let searchPanel = $state<ReturnType<typeof SearchPanel>>();

  // ---- sidebar views (per device) ----
  type SideView = "requests" | "search" | "history" | "git";
  const SIDE_VIEWS: { id: SideView; label: string }[] = [
    { id: "requests", label: "Requests" },
    { id: "search", label: "Search" },
    { id: "history", label: "History" },
    { id: "git", label: "Git" },
  ];
  const isSideView = (v: unknown): v is SideView => SIDE_VIEWS.some((s) => s.id === v);
  let sideView = $state<SideView>(load("sideView", "requests", isSideView));
  async function showView(view: SideView) {
    sideView = view;
    store("sideView", view);
    await tick();
    if (view === "search") searchPanel?.focus();
    if (view === "requests") await sidebar?.focus();
  }

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

  /** Runs `opening` (which may open a collection) and restores that collection's tabs. */
  async function openWith(opening: () => Promise<void>) {
    const before = app.collection;
    restoring = true;
    try {
      await opening();
      if (app.collection && app.collection !== before) await restoreTabs(app.collection.id);
    } finally {
      restoring = false;
    }
    if (app.collection && app.collection !== before) {
      if (sideView !== "requests") return;
      await tick();
      await sidebar?.focus();
    }
  }
  const openFolder = () => openWith(() => app.open());
  /** For collections the backend opened itself (recent list, import, clone). */
  const adoptCollection = (info: CollectionInfo) => openWith(() => app.adopt(info));

  onMount(() => {
    reopenOnStart(app).then(
      (info) => {
        if (info && !app.collection) void adoptCollection(info);
      },
      (e: unknown) => console.warn("RequestTailor: reopening the last collection failed:", e),
    );
  });

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
    "focus-sidebar": () => showView("requests"),
    import: () => (importing = true),
    search: () => showView("search"),
    history: () => showView("history"),
    git: () => showView("git"),
    plugins: () => (managingPlugins = true),
    cookies: () => {
      if (app.collection) showingCookies = true;
    },
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
    <button class="button" data-testid="import" onclick={() => (importing = true)}>Import</button>
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
      {#if app.collection}
        <div class="views" role="tablist" aria-label="Sidebar">
          {#each SIDE_VIEWS as v (v.id)}
            <button
              role="tab"
              class="view"
              data-testid="view-{v.id}"
              aria-selected={sideView === v.id}
              onclick={() => showView(v.id)}>{v.label}</button
            >
          {/each}
        </div>
      {/if}
      <div class="view-body">
        {#if sideView === "requests" || !app.collection}
          <Sidebar bind:this={sidebar} bind:newIn hint={keys("new-request")} />
        {:else if sideView === "search"}
          <SearchPanel bind:this={searchPanel} />
        {:else if sideView === "history"}
          <HistoryPanel />
        {:else}
          <GitPanel onopen={adoptCollection} />
        {/if}
      </div>
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
          {#if !app.collection}<RecentCollections onopen={adoptCollection} />{/if}
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
<ImportDialog open={importing} onclose={() => (importing = false)} onopen={adoptCollection} />
<PluginsDialog open={managingPlugins} onclose={() => (managingPlugins = false)} />
<CookiesDialog open={showingCookies} onclose={() => (showingCookies = false)} />
<AuthPromptDialog />

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
    display: flex;
    flex-direction: column;
  }
  .views {
    display: flex;
    gap: 2px;
    padding: 4px 6px 0;
    border-bottom: 1px solid var(--rt-border);
  }
  .view {
    padding: 3px 8px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--rt-fg-muted);
    font: inherit;
    font-size: var(--rt-size-small);
    cursor: pointer;
  }
  .view[aria-selected="true"] {
    border-bottom-color: var(--rt-accent);
    color: var(--rt-fg);
  }
  .view-body {
    flex: 1;
    min-height: 0;
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
