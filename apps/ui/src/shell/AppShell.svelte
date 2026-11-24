<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- App layout: header, resizable sidebar (views: requests, search, history, Git), tabs,
     request editor above the response (resizable split), command palette and global shortcuts.
     Owner: wave 3, strand A. Composes the other strands' components through their props only.
     Wave 4 mount points: GitPanel (A2), ImportDialog (B), RecentCollections and
     reopenOnStart (C), SearchPanel and HistoryPanel (E).
     Wave 5 mount points: PluginsDialog (A), AuthPromptDialog (C), CookiesDialog (D).
     Wave 6 mount points (strand F): WelcomeDialog; the collection tabs of plugins (header
     button "Collection", palette entries, CollectionPanel in the main area), shown only
     while an enabled plugin has one.
     Wave 7 mount point (strand D): RunnerPanel in the main area, opened from a folder's run
     button in the sidebar, the header button "Run" and the palette ("Run collection").
     Wave 8 mount point (strand E): AiDialogs and the AI palette entries.
     Wave 8 mount point (strand A): WebSocket, SSE and gRPC requests show Connect instead of
     Send and the SessionPane instead of the response.
     Wave 12 mount points (interface): GitViewHost in the main area (one Git-for-APIs view at
     a time, `git/views.svelte.ts`) and the strands' palette entries (`git/api-commands.ts`).
     Wave 13: tabs of a distribution's extensions (`AppExtension.tabs`, `extension-tabs.ts`); the
     collection panel shows plugin and extension collection tabs together.
     Wave 14 mount points (interface): the "New…" dialog (`new/NewDialog.svelte`, strand B;
     `AppState.openNew`), extension dialogs (`DialogHost`, strand C; `AppState.showDialog`) and
     main-area views asked for with `AppState.show` / `openCollectionTab`. -->
<script lang="ts">
  import { onMount, tick, untrack } from "svelte";

  import AiDialogs from "../ai/AiDialogs.svelte";
  import { aiFor } from "../ai/ai.svelte";
  import { aiCommands } from "../ai/commands";
  import AgentsHost from "../agents/AgentsHost.svelte";
  import { agentCommands } from "../agents/ui.svelte";
  import AuthPromptDialog from "../auth/AuthPromptDialog.svelte";
  import type { Backend, CollectionInfo } from "../commands/backend";
  import type { AppExtension, ExtensionContext } from "../extensions";
  import CookiesDialog from "../cookies/CookiesDialog.svelte";
  import { formatAvailable, requestFormat } from "../editor/format-body";
  import RequestEditor from "../editor/RequestEditor.svelte";
  import NewDialog from "../new/NewDialog.svelte";
  import DialogHost from "./DialogHost.svelte";
  import EnvironmentManager from "../environments/EnvironmentManager.svelte";
  import EnvironmentSwitcher from "../environments/EnvironmentSwitcher.svelte";
  import ProductionFrame from "../environments/ProductionFrame.svelte";
  import { sendLabel } from "../environments/production";
  import { getProduction } from "../environments/production.svelte";
  import SessionVarsButton from "../scripts/SessionVarsButton.svelte";
  import { gitApiCommands } from "../git/api-commands";
  import GitPanel from "../git/GitPanel.svelte";
  import GitViewHost from "../git/GitViewHost.svelte";
  import { GitViews, provideGitViews } from "../git/views.svelte";
  import HistoryPanel from "../history/HistoryPanel.svelte";
  import { McpState } from "../mcp/mcp.svelte";
  import McpPanel from "../mcp/McpPanel.svelte";
  import McpSidebar from "../mcp/McpSidebar.svelte";
  import CloneDialog from "../git/CloneDialog.svelte";
  import ImportDialog from "../import/ImportDialog.svelte";
  import { openApiCommands } from "../openapi/commands";
  import { shareCommands } from "../share/commands";
  import SharedBanner from "./SharedBanner.svelte";
  import OpenApiHost from "../openapi/OpenApiHost.svelte";
  import CollectionPanel, { type CollectionTab } from "../plugins/CollectionPanel.svelte";
  import { tabsFor } from "../plugins/permissions";
  import PluginsDialog from "../plugins/PluginsDialog.svelte";
  import { pluginsState } from "../plugins/store.svelte";
  import WelcomeDialog from "../plugins/WelcomeDialog.svelte";
  import RecentCollections from "../recent/RecentCollections.svelte";
  import { reopenOnStart } from "../recent/startup";
  import RunnerPanel from "../runner/RunnerPanel.svelte";
  import { RunnerState } from "../runner/runner.svelte";
  import TrustPrompt from "../scripts/TrustPrompt.svelte";
  import SearchPanel from "../search/SearchPanel.svelte";
  import ConnectButton from "../stream/ConnectButton.svelte";
  import SessionPane from "../stream/SessionPane.svelte";
  import { provideSessions, SessionsState } from "../stream/sessions.svelte";
  import ResponseView from "../response/ResponseView.svelte";
  import { AppState, provideApp } from "../state/app.svelte";
  import "../styles/base.css";
  import CompanionOffer from "../transport/CompanionOffer.svelte";
  import CommandPalette from "./CommandPalette.svelte";
  import AssistantPanel from "./AssistantPanel.svelte";
  import { createExtensionContext } from "./extension-context";
  import { extensionAssistant } from "./extension-actions";
  import SuggestHost from "../suggest/SuggestHost.svelte";
  import { inlineProposalOpen, suggestionOwnsEnter } from "../suggest/keys.svelte";
  import { suggestUndoFor } from "../suggest/undo.svelte";
  import { extensionGitViews } from "./extension-git";
  import { extensionTabsFor, provideExtensions } from "./extension-tabs";
  import { buildCommands, LABELS, type Actions } from "./commands";
  import Resizer from "./Resizer.svelte";
  import { formatShortcut, isMacPlatform, matchShortcut, shortcutFor, SHORTCUTS, TAB_NUMBERS, type CommandId, type TabNumber } from "./shortcuts";
  import { IconButton } from "../ui";
  import Sidebar from "./Sidebar.svelte";
  import { isNumber, load, store } from "./storage";
  import TabBar from "./TabBar.svelte";
  import { Theme } from "./theme.svelte";
  import { connectExchange } from "./extension-exchange";
  import { requestPaths, tabElementId } from "./tree";

  let { backend, extensions = [] }: { backend: Backend; extensions?: readonly AppExtension[] } = $props();
  // The backend is fixed for the lifetime of the shell.
  // svelte-ignore state_referenced_locally
  const app = provideApp(new AppState(backend));
  // Wave 9: extensions of a distribution (none in the builds of this repository); fixed like the backend.
  // svelte-ignore state_referenced_locally
  const extensionContext: ExtensionContext = createExtensionContext(backend, app);
  // Wave 13: their request, response and collection tabs.
  // svelte-ignore state_referenced_locally
  const provided = provideExtensions({ extensions, ctx: extensionContext });
  const sessions = provideSessions(new SessionsState(app));
  // A session lives while its request's tab is open.
  $effect(() => {
    const open = [...app.tabs];
    untrack(() => void sessions.keepOnly(open));
  });
  const theme = new Theme();
  const mac = isMacPlatform();
  const keys = (id: CommandId) => {
    const s = shortcutFor(id);
    return s ? formatShortcut(s, mac) : "";
  };

  let managing = $state(false);
  let importing = $state(false);
  let cloning = $state(false);
  let managingPlugins = $state(false);
  let showingCookies = $state(false);
  let innerWidth = $state(1024);
  let paletteOpen = $state(false);
  /** Folder the inline "new request" prompt is open for (`""` = root). */
  let newIn = $state<string | null>(null);
  let sidebar = $state<ReturnType<typeof Sidebar>>();
  let searchPanel = $state<ReturnType<typeof SearchPanel>>();

  // ---- sidebar views (per device) ----
  // Wave 14: icons with their name as tooltip, so the bar fits a narrow sidebar without
  // scrolling; each view names itself in its own header. The id "requests" stays (stored
  // per device, test ids).
  type SideView = "requests" | "search" | "history" | "git" | "mcp";
  const SIDE_VIEWS: { id: SideView; label: string; icon: string }[] = [
    { id: "requests", label: "Collections", icon: "collection" },
    { id: "search", label: "Search", icon: "search" },
    { id: "history", label: "History", icon: "history" },
    { id: "git", label: "Git", icon: "branch" },
    { id: "mcp", label: "MCP: Test MCP servers and see what AI agents see", icon: "mcp" },
  ];
  function onViewsKeydown(e: KeyboardEvent) {
    const i = sideViews.findIndex((v) => v.id === shownView);
    const n = sideViews.length;
    const next =
      e.key === "ArrowRight" ? (i + 1) % n : e.key === "ArrowLeft" ? (i - 1 + n) % n : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    const id = sideViews[next]!.id;
    sideView = id;
    store("sideView", id);
    void tick().then(() => document.querySelector<HTMLElement>(`[data-testid="view-${id}"]`)?.focus());
  }
  const isSideView = (v: unknown): v is SideView => SIDE_VIEWS.some((s) => s.id === v);
  let sideView = $state<SideView>(load("sideView", "requests", isSideView));
  // A set with only repositories has no collection to search, run or test: it offers the list
  // and Git. Collection-based views come back with the first collection.
  const sideViews = $derived(app.collection ? SIDE_VIEWS : SIDE_VIEWS.filter((v) => v.id === "requests" || v.id === "git"));
  const shownView = $derived<SideView>(sideViews.some((v) => v.id === sideView) ? sideView : "requests");
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
  // A proposal inside an editor (wave 16) needs room to be read: the editor pane takes most of
  // the panel while it is open, and goes back to the chosen split when it closes.
  const editorHeight = $derived(
    inlineProposalOpen() ? Math.max(Math.round(panelHeight * split), Math.round(panelHeight * 0.75)) : Math.round(panelHeight * split),
  );
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
  /** For collections the backend opened itself (recent list, clone). */
  const adoptCollection = (info: CollectionInfo) => openWith(() => app.adopt(info));
  /** For a collection the backend added to the set (import as a new collection). */
  const adoptAdded = (info: CollectionInfo) => openWith(() => app.adoptAdded(info));

  onMount(() => {
    // Wave 17: `AppExtension.onExchange` after every send.
    const disconnectExchange = connectExchange(app, extensions, extensionContext);
    for (const x of extensions) {
      Promise.resolve()
        .then(() => x.start?.(extensionContext))
        .catch((e: unknown) => console.warn(`RequestTailor: extension ${x.id} failed to start:`, e));
    }
    reopenOnStart(app).then(
      (info) => {
        if (info && !app.collection) void adoptCollection(info);
      },
      (e: unknown) => console.warn("RequestTailor: reopening the last collection failed:", e),
    );
    return disconnectExchange;
  });

  // ---- the assistant panel of an extension (wave 16): header button, palette, Mod+J ----
  const assistant = $derived(extensionAssistant(provided));
  let assistantOpen = $state(false);
  const assistantFocus = $derived({
    collection: app.activeCollection,
    request: app.active,
    response: app.active ? (app.results[app.active] ?? null) : null,
    folder: null,
  });
  const suggestUndo = suggestUndoFor(app);
  const assistantKeys = mac ? "⌘J" : "Ctrl+J";

  // ---- commands and shortcuts ----
  const actions: Actions = {
    "open-folder": openFolder,
    // Also without an open collection: the dialog can make the first one.
    "new-request": () => app.openNew(),
    send: () => (app.active && sessions.kindOf(app.active) ? sessions.primary(app.active) : app.send()),
    save: () => app.save(),
    "close-tab": () => {
      if (app.active) app.close(app.active);
    },
    palette: () => (paletteOpen = !paletteOpen),
    "toggle-theme": () => theme.toggle(),
    "focus-sidebar": () => showView("requests"),
    import: () => (importing = true),
    clone: () => (cloning = true),
    search: () => showView("search"),
    history: () => showView("history"),
    git: () => showView("git"),
    plugins: () => (managingPlugins = true),
    cookies: () => {
      if (app.collection) showingCookies = true;
    },
    "format-body": requestFormat,
    "go-to-tab": () => {
      if (app.openTabs.length > 0) app.tabList = true;
    },
    "next-tab": () => app.stepTab(1),
    "previous-tab": () => app.stepTab(-1),
    ...(Object.fromEntries(
      TAB_NUMBERS.map((id, i) => [id, () => app.gotoTab(i + 1, id === "tab-9")]),
    ) as Record<TabNumber, () => unknown>),
  };
  // ---- collection tabs of plugins (wave 6) ----
  // The list is read once a collection is open (listing loads the web core).
  const plugins = pluginsState(app.backend, false);
  $effect(() => {
    if (app.collection) void plugins.ensure();
  });
  // Plugin collection tabs first, then those of the distribution's extensions (wave 13).
  const collectionTabs = $derived<CollectionTab[]>(
    app.collection
      ? [
          ...tabsFor(plugins.list, "collection").map((t) => ({
            key: t.key,
            title: t.tab.title,
            source: `Plugin ${t.pluginName}`,
            label: `Collection: ${t.tab.title} (${t.pluginName})`,
            plugin: t,
          })),
          ...extensionTabsFor(provided, "collection").map((t) => ({
            key: `extension:${t.key}`,
            title: t.tab.title,
            source: t.tab.title,
            label: `Collection: ${t.tab.title}`,
            extension: t,
          })),
        ]
      : [],
  );
  /** Key of the shown collection tab, `null`: the request editor. */
  let collectionTab = $state<string | null>(null);
  const showingCollection = $derived(collectionTab !== null && collectionTabs.some((t) => t.key === collectionTab));
  // Opening a request (sidebar, tabs, palette) shows the editor again.
  $effect(() => {
    void app.active;
    untrack(() => (collectionTab = null));
  });
  async function openCollectionTab(key: string) {
    // Set after the effect above has seen the current request.
    await tick();
    runner.close();
    collectionTab = key;
  }

  // ---- views asked for by extensions and the "New…" dialog (wave 14) ----
  $effect(() => {
    const asked = app.view;
    if (!asked) return;
    untrack(() => {
      app.view = null;
      const v = asked.view;
      if (v.kind === "collection-tab") {
        const key = [v.key, `extension:${v.key}`].find((k) => collectionTabs.some((t) => t.key === k));
        if (key) void openCollectionTab(key);
      } else if (v.kind === "mcp") void openMcp(v.test);
      else if (v.kind === "environments") managing = true;
      else if (v.kind === "git") gitViews.open(v.view);
      else void openRunner(v.folder);
    });
  });

  // ---- AI assistant (wave 8) ----
  const ai = aiFor(app.backend);

  // ---- collection runner (wave 7) ----
  const runner = new RunnerState(app);
  const production = getProduction(app);
  runner.beforeStart = (target) => production.gateRun(target.name);
  /** Opens the runner for `folder` (`null`: the collection) of `collection` (default: the
   *  active one), which becomes active (wave 14). */
  async function openRunner(folder: string | null, name?: string, collection?: string) {
    if (collection && app.activeCollection !== collection) await app.activate(collection);
    if (!app.collection) return;
    // Set after the effect below has seen the current request.
    await tick();
    collectionTab = null;
    runner.open({ collection: app.collection.id, folder, name: name ?? app.collection.name });
  }
  // Opening a request or another collection shows the editor again; a run goes on. The
  // runner's own collection becoming active (a run starting in it) keeps it open.
  let runnerSeen: string | undefined = undefined;
  $effect(() => {
    void app.active;
    const id = app.collection?.id;
    untrack(() => {
      const switched = id !== runnerSeen;
      runnerSeen = id;
      if (switched && runner.target && runner.target.collection === id) return;
      runner.close();
    });
  });
  /** Wave 14 (sidebar menu): collection tab `key` of `collection`, made active first. */
  async function openCollectionTabOf(collection: string, key: string) {
    if (app.activeCollection !== collection) await app.activate(collection);
    await openCollectionTab(key);
  }
  async function manageEnvironmentsOf(collection: string) {
    if (app.activeCollection !== collection) await app.activate(collection);
    managing = true;
  }

  // ---- MCP client (wave 8) ----
  const mcp = new McpState(app);
  /** Shows the MCP client; with `test`, that saved test opened in it (the "New…" dialog). */
  async function openMcp(test?: string) {
    await tick();
    runner.close();
    collectionTab = null;
    mcp.show();
    if (sideView !== "mcp") await showView("mcp");
    if (test) {
      await mcp.loadTests();
      await mcp.openTest(test);
    }
  }
  // Opening a request, the runner or a collection tab shows that instead.
  $effect(() => {
    void app.active;
    untrack(() => mcp.close());
  });
  $effect(() => {
    if (runner.target || showingCollection) untrack(() => mcp.close());
  });
  $effect(() => {
    void app.collection?.id;
    untrack(() => mcp.reset());
  });

  // ---- Git for APIs (wave 12): one view at a time in the main area ----
  const gitViews = provideGitViews(new GitViews());
  // Opening a request or another collection shows the editor again.
  $effect(() => {
    void app.active;
    void app.collection?.id;
    untrack(() => gitViews.close());
  });
  // A Git view replaces the runner, the MCP client and a collection tab, and the other way round.
  $effect(() => {
    if (gitViews.current) {
      untrack(() => {
        runner.close();
        mcp.close();
        collectionTab = null;
      });
    }
  });
  $effect(() => {
    if (runner.target || mcp.open || showingCollection) untrack(() => gitViews.close());
  });

  const commands = $derived([
    ...buildCommands(app, actions, mac),
    ...collectionTabs.map((t) => ({
      id: `collection-tab:${t.key}`,
      label: t.label,
      run: () => openCollectionTab(t.key),
    })),
    ...(app.collection ? [{ id: "run-collection", label: "Run collection…", run: () => openRunner(null) }] : []),
    ...(assistant
      ? [{ id: "assistant", label: "Assistant", shortcut: assistantKeys, run: () => (assistantOpen = !assistantOpen) }]
      : []),
    ...(suggestUndo.last?.undo ? [{ id: "suggest-undo", label: "Undo suggestion", run: () => void suggestUndo.run() }] : []),
    ...aiCommands(app, ai),
    ...agentCommands(app),
    ...gitApiCommands(app, gitViews),
    // Views of the distribution's extensions in the Git host (since wave 15): one palette
    // entry each, while there is a repository to look at.
    ...(app.activeRepository
      ? extensionGitViews(provided).map((v) => ({
          id: `git-view:${v.key}`,
          label: `Git: ${v.action.title}`,
          run: () => gitViews.open({ kind: "extension", id: v.key }),
        }))
      : []),
    ...openApiCommands(),
    ...shareCommands(app),
    ...(app.collection ? [{ id: "mcp", label: "Test an MCP server", run: () => openMcp() }] : []),
    ...extensions.flatMap((x) =>
      (x.commands?.(extensionContext) ?? []).map((c) => ({ id: `${x.id}:${c.id}`, label: c.label, run: c.run })),
    ),
  ]);

  // Capture phase: runs before inputs and code editors handle the key, so Mod+Enter and Mod+S
  // work wherever focus is.
  $effect(() => {
    const onkeydown = (e: KeyboardEvent) => {
      if (assistant && e.key.toLowerCase() === "j" && !e.altKey && !e.shiftKey && (mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        e.stopPropagation();
        if (!e.repeat) assistantOpen = !assistantOpen;
        return;
      }
      const s = matchShortcut(e, mac);
      if (!s) return;
      // Shift+Alt+F types a character (e.g. "Ï" on macOS): only taken when a body can be formatted.
      if (s.command === "format-body" && !formatAvailable()) return;
      // A suggestion on screen takes Mod+Enter (apply), not "send".
      if (s.command === "send" && suggestionOwnsEnter()) return;
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
  const sessionKind = $derived(sessions.kindOf(app.active));
  const activeTabId = $derived.by(() => {
    const i = app.openTabs.findIndex((t) => t.collection === app.activeCollection && t.path === app.active);
    return i >= 0 ? tabElementId(i) : undefined;
  });
</script>

<div class="shell">
  <ProductionFrame />
  <header>
    <button class="button" data-testid="open-collection" title="Open folder ({keys('open-folder')})" onclick={openFolder}>
      Open folder
    </button>
    <button class="button" data-testid="import" onclick={() => (importing = true)}>Import</button>
    {#if !app.collection}
      <button class="button" data-testid="header-clone" title="Clone a repository…" onclick={() => (cloning = true)}>Clone</button>
    {/if}
    <span class="collection" title={app.collection?.location ?? ""}>
      <span data-testid="collection-name">{app.collection?.name ?? ""}</span>
    </span>
    <span class="spacer"></span>
    {#if collectionTabs.length > 0}
      <button
        class="button"
        data-testid="collection-tabs"
        aria-pressed={showingCollection}
        title="Collection tabs"
        onclick={() => (showingCollection ? (collectionTab = null) : openCollectionTab(collectionTabs[0]!.key))}>Collection</button
      >
    {/if}
    {#if app.collection}
      <button
        class="button"
        data-testid="run-collection"
        aria-pressed={runner.target?.folder === null}
        title="Run the collection's requests"
        onclick={() => (runner.target?.folder === null ? runner.close() : openRunner(null))}>Run</button
      >
      <EnvironmentSwitcher />
      <button class="button" onclick={() => (managing = true)}>Environments</button>
      <SessionVarsButton />
    {/if}
    <button
      class="button primary"
      data-testid="save"
      title="Save all collections ({keys('save')})"
      onclick={() => app.save()}
      disabled={app.dirty.length === 0 && !app.collections.some((c) => c.dirty > 0)}>Save</button
    >
    {#each extensions as x (x.id)}
      {#if x.header}
        <x.header ctx={extensionContext} />
      {/if}
    {/each}
    {#if assistant}
      <button
        class="button"
        data-testid="assistant-toggle"
        aria-pressed={assistantOpen}
        title="{assistant.assistant.title} ({assistantKeys})"
        onclick={() => (assistantOpen = !assistantOpen)}>{assistant.assistant.title}</button
      >
    {/if}
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
      {#if app.collection || app.activeRepository}
        <div class="views" role="tablist" aria-label="Sidebar" tabindex="-1" onkeydown={onViewsKeydown}>
          {#each sideViews as v (v.id)}
            <span class="view" class:selected={shownView === v.id} role="none">
              <IconButton
                role="tab"
                icon={v.icon}
                label={v.label}
                tooltipPlacement="bottom"
                data-testid="view-{v.id}"
                aria-selected={shownView === v.id}
                tabindex={shownView === v.id ? 0 : -1}
                onclick={() => void showView(v.id)}
              />
            </span>
          {/each}
        </div>
      {/if}
      <div class="view-body">
        {#if shownView === "requests" || !(app.collection || app.activeRepository)}
          <Sidebar
            bind:this={sidebar}
            bind:newIn
            hint={keys("new-request")}
            onrun={(collection, folder, name) => openRunner(folder, name, collection)}
            panelTabs={collectionTabs}
            onpanel={openCollectionTabOf}
            onclone={() => (cloning = true)}
            onenvironments={manageEnvironmentsOf}
          />
        {:else if shownView === "search"}
          <SearchPanel bind:this={searchPanel} />
        {:else if shownView === "history"}
          <HistoryPanel />
        {:else if shownView === "mcp"}
          <McpSidebar {mcp} />
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
      <TabBar
        panelId="rt-request-panel"
        onreveal={async (ref) => {
          await app.selectRef(ref);
          await showView("requests");
        }}
      />
      <TrustPrompt />
      <AgentsHost />
      {#if gitViews.current}
        <GitViewHost views={gitViews} />
      {:else if mcp.open}
        <McpPanel {mcp} />
      {:else if runner.target}
        <RunnerPanel {runner} />
      {:else if showingCollection && collectionTab}
        <CollectionPanel tabs={collectionTabs} active={collectionTab} onselect={(k) => (collectionTab = k)} onclose={() => (collectionTab = null)} />
      {:else if app.active}
        <div class="panel" id="rt-request-panel" role="tabpanel" aria-labelledby={activeTabId} bind:clientHeight={panelHeight}>
          <div class="pane editor" style:height="{editorHeight}px">
            <RequestEditor path={app.active}>
              {#snippet actions()}
                {#if sessionKind && app.active}
                  <ConnectButton path={app.active} {sessions} shortcut={keys("send")} />
                {:else}
                <button
                  class="button primary send"
                  data-testid="send"
                  data-production={production.active ? "true" : "false"}
                  title="Send ({keys('send')})"
                  disabled={sending}
                  onclick={() => app.send()}
                  >{sendLabel(production.active, sending)}<kbd>{keys("send")}</kbd></button
                >
                {/if}
              {/snippet}
            </RequestEditor>
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
            {#if sessionKind}
              <SessionPane path={app.active} {sessions} />
            {:else}
              <CompanionOffer path={app.active} />
              <ResponseView path={app.active} />
            {/if}
          </div>
        </div>
      {:else}
        <div class="start" data-testid="start">
          <p>{app.collection ? "Open a request from the sidebar." : "Open a folder with a collection to start."}</p>
          {#if !app.collection}
            <button class="button" data-testid="start-clone" onclick={() => (cloning = true)}>Clone a repository…</button>
            <RecentCollections onopen={adoptCollection} />
          {/if}
          <dl>
            {#each SHORTCUTS.filter((x) => !x.unlisted) as s (s.command)}
              <dt><kbd>{formatShortcut(s, mac)}</kbd></dt>
              <dd>{LABELS[s.command]}</dd>
            {/each}
          </dl>
        </div>
      {/if}
    </main>
    {#if assistant && assistantOpen}
      <AssistantPanel assistant={assistant.assistant} ctx={extensionContext} focus={assistantFocus} onclose={() => (assistantOpen = false)} />
    {/if}
  </div>

  <footer>
    <SharedBanner />
    {#if app.error}<p class="error" role="alert">{app.error}</p>{/if}
  </footer>
</div>

<CommandPalette bind:open={paletteOpen} {commands} />
{#each extensions as x (x.id)}
  {#if x.overlay}
    <x.overlay ctx={extensionContext} />
  {/if}
{/each}
<NewDialog onfallback={(folder) => (newIn = folder)} />
<DialogHost />
<SuggestHost />
<EnvironmentManager open={managing} onclose={() => (managing = false)} />
<CloneDialog open={cloning} onclose={() => (cloning = false)} onopen={adoptCollection} />
<ImportDialog open={importing} onclose={() => (importing = false)} onopen={adoptAdded} />
<OpenApiHost onopen={adoptAdded} />
<PluginsDialog open={managingPlugins} onclose={() => (managingPlugins = false)} />
<CookiesDialog open={showingCookies} onclose={() => (showingCookies = false)} />
<AuthPromptDialog />
<WelcomeDialog />
<AiDialogs />

<svelte:window bind:innerWidth />

<style>
  .shell {
    position: fixed;
    inset: 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    /* Without this the column grows with the widest line (a long URL) and pushes Send out. */
    grid-template-columns: minmax(0, 1fr);
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
    position: relative;
    display: flex;
    min-height: 0;
    min-width: 0;
  }
  .side {
    flex: none;
    min-width: 0;
    max-width: 60vw;
    display: flex;
    flex-direction: column;
  }
  .views {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--rt-space-2);
    height: var(--rt-bar-height);
    padding: 0 var(--rt-side-inset);
    border-bottom: 1px solid var(--rt-border);
  }
  .view {
    position: relative;
    display: inline-flex;
    color: var(--rt-fg-muted);
  }
  /* The chosen view: an accent bar on the bar's bottom edge, as tabs elsewhere. */
  .view.selected::after {
    content: "";
    position: absolute;
    left: var(--rt-space-2);
    right: var(--rt-space-2);
    bottom: calc((var(--rt-bar-height) - var(--rt-control-md)) / -2 - 1px);
    height: 2px;
    border-radius: var(--rt-radius-pill);
    background: var(--rt-accent-solid);
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
  /* The work area keeps distance from the sidebar: editor and response share one inset. */
  .editor {
    flex: none;
    padding: var(--rt-gap) var(--rt-inset);
  }
  .response {
    flex: 1;
    padding: 0 var(--rt-inset);
  }
  .panel {
    --rt-inset: 16px;
  }
  /* Send closes the URL row, like the Enter key in the URL field. */
  .send {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding-inline: 16px;
  }
  .button.send[data-production="true"]:not(:disabled) {
    background: var(--rt-danger-solid);
    border-color: var(--rt-danger-solid);
    color: #fff;
  }
  .send kbd {
    font-size: var(--rt-size-small);
    opacity: 0.75;
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
