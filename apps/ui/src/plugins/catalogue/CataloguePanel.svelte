<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The plugin catalogue without a server (wave 18, strand I): the sources the user added, a
     search over their indexes, and an install that shows what the plugin itself asks for and the
     SHA-256 first. Nothing is requested until Refresh, Install or "Check for updates" is
     pressed; there is no built-in index. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { CatalogueSource, CatalogueUpdate, InstalledSource, InstallPlan, SourceIndex } from "../../commands/types";
  import { getApp } from "../../state/app.svelte";
  import { Badge, Banner, Button, EmptyState, TextField } from "../../ui";
  import { pluginsState } from "../store.svelte";
  import { claimedRights, commitNote, explain, noRights, planRights, rows, shortSum, sourceLabel } from "./model";

  let { onchange = () => {} }: { onchange?: () => void } = $props();
  const app = getApp();
  const plugins = pluginsState(app.backend, false);

  let sources = $state.raw<CatalogueSource[]>([]);
  let installed = $state.raw<InstalledSource[]>([]);
  let updates = $state.raw<CatalogueUpdate[]>([]);
  let indexes = $state.raw<SourceIndex[]>([]);
  /** Failures per source address. */
  let failures = $state.raw<Record<string, string>>({});
  let address = $state("");
  let query = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let plan = $state.raw<InstallPlan | null>(null);
  let done = $state<string | null>(null);

  // Opening lists what is stored on this device; nothing is requested.
  $effect(() => {
    untrack(() => void load());
  });

  async function load() {
    try {
      [sources, installed] = await Promise.all([app.backend.catalogue.sources(), app.backend.catalogue.installed()]);
    } catch (e) {
      error = explain(e instanceof Error ? e.message : String(e));
    }
  }

  async function act(f: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = null;
    done = null;
    try {
      await f();
    } catch (e) {
      error = explain(e instanceof Error ? e.message : String(e));
    } finally {
      busy = false;
    }
  }

  const add = () =>
    act(async () => {
      const source = await app.backend.catalogue.addSource(address);
      address = "";
      sources = await app.backend.catalogue.sources();
      done = `Added ${source.url}. Press Refresh to read its index.`;
    });

  const remove = (url: string) =>
    act(async () => {
      await app.backend.catalogue.removeSource(url);
      sources = await app.backend.catalogue.sources();
      indexes = indexes.filter((i) => i.source.url !== url);
    });

  async function read(url?: string) {
    const next: Record<string, string> = { ...failures };
    const targets = url ? [url] : sources.map((s) => s.url);
    let fresh = indexes.filter((i) => !targets.includes(i.source.url));
    for (const target of targets) {
      try {
        const got = await app.backend.catalogue.refresh(target);
        fresh = [...fresh, ...got];
        delete next[target];
      } catch (e) {
        next[target] = explain(e instanceof Error ? e.message : String(e));
      }
    }
    failures = next;
    indexes = fresh;
    sources = await app.backend.catalogue.sources();
  }

  const refresh = (url?: string) => act(() => read(url));

  const checkUpdates = () =>
    act(async () => {
      updates = await app.backend.catalogue.checkUpdates();
      done = updates.length === 0 ? "Everything installed from a source is up to date." : null;
    });

  const review = (source: string, id: string) =>
    act(async () => {
      plan = await app.backend.catalogue.plan(source, id);
    });

  const install = () =>
    act(async () => {
      if (!plan) return;
      const got = await app.backend.catalogue.install(plan);
      done = `Installed ${got.plugin} ${got.version}.`;
      plan = null;
      installed = await app.backend.catalogue.installed();
      updates = updates.filter((u) => u.plugin !== got.plugin);
      await plugins.refresh();
      onchange();
    });

  const listed = $derived(rows(indexes, installed, updates, query));
</script>

<div class="catalogue" data-testid="catalogue">
  {#if error}<Banner tone="danger" data-testid="catalogue-error">{error}</Banner>{/if}
  {#if done}<Banner tone="ok" data-testid="catalogue-done">{done}</Banner>{/if}

  {#if plan}
    {@const p = plan}
    <section class="confirm" data-testid="catalogue-confirm" aria-label="Confirm installation">
      <h3>{p.update ? "Update" : "Install"} {p.entry.name} {p.entry.version}?</h3>
      <p class="muted"><code>{p.entry.id}</code>{#if p.entry.license} · {p.entry.license}{/if} · from {p.source}</p>
      <p class="muted" data-testid="catalogue-sha">SHA-256 <code title={p.sha256}>{shortSum(p.sha256)}</code>, checked against the index.</p>
      {#each p.warnings as w (w)}<Banner tone="warn" data-testid="catalogue-warning">{w}</Banner>{/each}
      {#if p.update && p.more_permissions}
        <Banner tone="warn" data-testid="catalogue-more">This update asks for more than you granted before.</Banner>
      {/if}
      {#if noRights(p.permissions)}
        <p data-testid="catalogue-permissions-none">The plugin asks for no permissions.</p>
      {:else}
        <p>The plugin itself asks for:</p>
        <ul data-testid="catalogue-permissions">
          {#each planRights(p) as r (r)}<li>{r}</li>{/each}
        </ul>
      {/if}
      <p class="muted">Plugins never see secret values. Their network access is limited to the hosts listed.</p>
      <div class="row">
        <Button variant="primary" loading={busy} onclick={() => void install()} data-testid="catalogue-confirm-install">
          {p.update ? (p.more_permissions ? "Grant and update" : "Update") : "Install"}
        </Button>
        <Button disabled={busy} onclick={() => (plan = null)} data-testid="catalogue-confirm-cancel">Cancel</Button>
      </div>
    </section>
  {/if}

  <section aria-label="Sources">
    <h3>Sources</h3>
    <p class="muted">
      There is no built-in index. Add the address of a <code>plugins.json</code> or of a Git repository that has one (<code>https://host/org/repo</code>).
      Nothing is requested until you press Refresh.
    </p>
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <TextField label="Source address" placeholder="https://example.com/plugins.json" bind:value={address} data-testid="catalogue-source-input" />
      <Button type="submit" disabled={busy || address.trim() === ""} data-testid="catalogue-add-source">Add</Button>
    </form>
    {#if sources.length === 0}
      <EmptyState icon="info" title="No sources yet" text="Add one above to search for plugins." data-testid="catalogue-no-sources" />
    {:else}
      <ul class="sources" data-testid="catalogue-sources">
        {#each sources as s (s.url)}
          <li data-testid="catalogue-source" data-source={s.url}>
            <span class="name">
              <strong>{sourceLabel(s)}</strong>
              {#if commitNote(s)}<Badge kind="label" value={commitNote(s) ?? ""} />{/if}
              <span class="muted url">{s.url}</span>
            </span>
            <Button size="sm" disabled={busy} onclick={() => void refresh(s.url)} data-testid="catalogue-refresh-source">Refresh</Button>
            <Button size="sm" variant="ghost" disabled={busy} onclick={() => void remove(s.url)} data-testid="catalogue-remove-source">Remove</Button>
            {#if failures[s.url]}<Banner tone="danger" data-testid="catalogue-source-error">{failures[s.url]}</Banner>{/if}
          </li>
        {/each}
      </ul>
      <div class="row">
        <Button variant="primary" loading={busy} onclick={() => void refresh()} data-testid="catalogue-refresh">Refresh all</Button>
        <Button disabled={busy || installed.length === 0} onclick={() => void checkUpdates()} data-testid="catalogue-check-updates">
          Check for updates
        </Button>
      </div>
    {/if}
  </section>

  {#if indexes.length > 0}
    <section aria-label="Plugins">
      <h3>Plugins</h3>
      <TextField label="Search" placeholder="Name or description" bind:value={query} data-testid="catalogue-search" />
      {#if listed.length === 0}
        <p class="muted" data-testid="catalogue-none">No plugin matches.</p>
      {:else}
        <ul class="entries">
          {#each listed as r (r.source.url + r.entry.id)}
            <li data-testid="catalogue-entry" data-plugin={r.entry.id}>
              <div class="title">
                <strong>{r.entry.name}</strong>
                <span class="muted">{r.entry.version}</span>
                {#if r.installed}<Badge kind="label" tone="ok" value={r.update ? `installed ${r.installed}` : "installed"} />{/if}
                {#if r.update}<Badge kind="label" tone="accent" value="update" data-testid="catalogue-update-badge" />{/if}
                <span class="spacer"></span>
                <Button
                  size="sm"
                  variant={r.update || !r.installed ? "secondary" : "ghost"}
                  disabled={busy}
                  onclick={() => void review(r.source.url, r.entry.id)}
                  data-testid="catalogue-install"
                >
                  {r.installed ? (r.installed === r.entry.version ? "Reinstall" : "Update") : "Install"}
                </Button>
              </div>
              {#if r.entry.description}<p class="muted">{r.entry.description}</p>{/if}
              <p class="muted small">
                from {sourceLabel(r.source)}{#if commitNote(r.source)} ({commitNote(r.source)}){/if} · the index says:
                {claimedRights(r.entry).join(" · ") || "no permissions"}. The plugin's own rights are shown before you install.
              </p>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  .catalogue {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
  }
  h3 {
    margin: 0 0 var(--rt-space-1);
    font-size: var(--rt-text-md, 1em);
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .small {
    font-size: var(--rt-size-small);
  }
  .row,
  .add {
    display: flex;
    gap: var(--rt-space-2);
    align-items: flex-end;
  }
  .add :global(.rt-field) {
    flex: 1;
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-2);
    padding: var(--rt-space-3);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius-md);
    background: var(--rt-surface-2);
  }
  .confirm ul {
    margin: 0;
    padding-left: 1.4em;
  }
  .sources,
  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .sources li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--rt-space-2);
    padding: var(--rt-space-2) 0;
    border-top: 1px solid var(--rt-border);
  }
  .sources .name {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-space-2);
    align-items: baseline;
  }
  .url {
    overflow-wrap: anywhere;
  }
  .sources li :global([role="alert"]) {
    flex-basis: 100%;
  }
  .entries li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--rt-space-2) 0;
    border-top: 1px solid var(--rt-border);
  }
  .title {
    display: flex;
    gap: var(--rt-space-2);
    align-items: center;
  }
  .spacer {
    flex: 1;
  }
</style>
