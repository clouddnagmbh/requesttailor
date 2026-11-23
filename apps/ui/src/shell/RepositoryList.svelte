<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The "Repositories" group of the sidebar (wave 15, strand A): every Git repository of the set,
     also those without a collection, with its status (branch, ahead/behind, changed files, a
     half-done merge). A click makes the repository the active one (every Git view acts on it);
     the context menu opens the Git views, shows the folder, or takes the folder out of the
     sidebar. Summaries are asked for after the first frame, one repository at a time, and again
     when the set changes, a collection is reloaded, or the window gets focus.
     `gitActions` of extensions for `{ kind: "repository" }` hang in at `extra` (strand E). -->
<script lang="ts">
  import type { GitRepoSummary, RepositoryEntry } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Badge, Icon, Menu, type MenuEntry } from "../ui";
  import { repoStatusParts } from "./tree";

  let { reveal = null }: { reveal?: ((id: string) => Promise<void> | void) | null } = $props();

  const app = getApp();

  let summaries = $state<Record<string, GitRepoSummary | null>>({});
  let round = 0;

  async function refresh() {
    const mine = ++round;
    for (const id of app.repositories.map((r) => r.id)) {
      let summary: GitRepoSummary | null = null;
      try {
        summary = await app.backend.repositories.summary(id);
      } catch {
        // A repository that cannot be read shows no status; its node stays usable.
      }
      if (mine !== round) return;
      summaries[id] = summary;
    }
  }

  $effect(() => {
    // What a new summary depends on: the list, a reload, the chosen repository.
    void [app.repositories.map((r) => r.id).join("\n"), app.loadCount, app.activeRepository];
    const timer = setTimeout(() => void refresh(), 30);
    return () => clearTimeout(timer);
  });

  const revealLabel = (() => {
    const p = typeof navigator === "undefined" ? "" : navigator.platform;
    return /Mac/i.test(p) ? "Show in Finder" : /Win/i.test(p) ? "Show in Explorer" : "Show in file manager";
  })();

  async function openView(repo: RepositoryEntry, view: "graph" | "changes") {
    await app.activateRepository(repo.id);
    app.show({ kind: "git", view: { kind: view } });
  }

  /** Extensions' `gitActions` for `{ kind: "repository" }` (strand E hangs them in here). */
  function extra(_repo: RepositoryEntry): MenuEntry[] {
    return [];
  }

  function menuFor(repo: RepositoryEntry): MenuEntry[] {
    const root = app.roots.find((r) => r.id === repo.root);
    const siblings = root ? Math.max(root.collections.length, 1) : 1;
    const more = extra(repo);
    return [
      { id: "graph", label: "Graph", icon: "graph", run: () => openView(repo, "graph") },
      { id: "changes", label: "Changes", icon: "file-text", run: () => openView(repo, "changes") },
      ...(more.length ? [{ separator: true } as MenuEntry, ...more] : []),
      ...(reveal ? [{ separator: true } as MenuEntry, { id: "reveal", label: revealLabel, icon: "folder-open", run: () => reveal(repo.id) }] : []),
      { separator: true },
      {
        id: "remove",
        label: siblings > 1 ? `Remove “${root?.name ?? repo.name}” from sidebar` : "Remove from sidebar",
        hint: siblings > 1 ? `${siblings} collections` : undefined,
        icon: "close",
        run: () => app.removeRoot(repo.root),
      },
    ];
  }

  let menu = $state<{ items: MenuEntry[]; anchor: HTMLElement | { x: number; y: number }; label: string } | null>(null);
  let menuOpen = $state(false);

  function openMenu(repo: RepositoryEntry, anchor: HTMLElement | { x: number; y: number }) {
    menu = { items: menuFor(repo), anchor, label: `${repo.name} actions` };
    menuOpen = true;
  }

  function onkeydown(e: KeyboardEvent, repo: RepositoryEntry) {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      openMenu(repo, e.currentTarget as HTMLElement);
    }
  }
</script>

<svelte:window onfocus={() => void refresh()} />

{#if app.repositories.length > 0}
  <section class="repos" data-testid="repositories" aria-label="Repositories">
    <h3 class="heading">Repositories</h3>
    <ul>
      {#each app.repositories as repo (repo.id)}
        {@const status = repoStatusParts(summaries[repo.id])}
        <li>
          <button
            type="button"
            class="repo"
            class:current={repo.id === app.activeRepository}
            data-testid="repository"
            data-repository={repo.id}
            aria-current={repo.id === app.activeRepository ? "true" : undefined}
            title="{repo.location}{status ? ` — ${status.text}` : ''}"
            onclick={() => void app.activateRepository(repo.id)}
            oncontextmenu={(e) => {
              e.preventDefault();
              openMenu(repo, { x: e.clientX, y: e.clientY });
            }}
            onkeydown={(e) => onkeydown(e, repo)}
          >
            <span class="glyph" aria-hidden="true"><Icon name="branch" size="sm" /></span>
            <span class="name">{repo.name}</span>
            {#if status}
              <span class="status" data-testid="repository-status">
                <span class="branch" data-testid="repository-branch">{status.branch}</span>
                {#if status.sync}<span class="sync" data-testid="repository-sync">{status.sync}</span>{/if}
                {#if status.operation}<Badge tone="warn" value={status.operation} />{/if}
                {#if status.changes > 0}
                  <span data-testid="repository-changes" title="{status.changes} changed files"><Badge kind="count" tone="accent" value={status.changes} /></span>
                {/if}
              </span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if menu}
  <Menu items={menu.items} anchor={menu.anchor} label={menu.label} bind:open={menuOpen} onclose={() => (menu = null)} />
{/if}

<style>
  .repos {
    padding: var(--rt-space-4) var(--rt-space-3) var(--rt-space-4);
    border-top: 1px solid var(--rt-border);
  }
  .heading {
    margin: 0 0 var(--rt-space-2) var(--rt-space-2);
    font: var(--rt-font-caption);
    color: var(--rt-fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .repo {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    width: 100%;
    min-height: var(--rt-control-md);
    padding: 0 var(--rt-space-2);
    border: 0;
    border-radius: var(--rt-radius-md);
    background: transparent;
    color: var(--rt-fg);
    font: var(--rt-font-body);
    text-align: left;
    cursor: default;
    transition: background-color var(--rt-dur-fast) var(--rt-ease);
  }
  .repo:hover {
    background: var(--rt-bg-hover);
  }
  .repo:focus-visible {
    outline: 2px solid var(--rt-focus);
    outline-offset: -2px;
  }
  .repo.current {
    background: var(--rt-accent-subtle);
    color: var(--rt-accent-subtle-fg);
  }
  .glyph {
    flex: none;
    display: inline-flex;
    color: var(--rt-fg-muted);
  }
  .current .glyph {
    color: var(--rt-accent);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: var(--rt-weight-medium);
  }
  .status {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--rt-space-2);
    max-width: 55%;
    font: var(--rt-font-small);
    color: var(--rt-fg-muted);
  }
  .branch {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sync {
    white-space: nowrap;
  }
</style>
