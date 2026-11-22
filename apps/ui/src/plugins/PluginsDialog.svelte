<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Installed plugins: install a .wasm file (its permissions are shown and confirmed first;
     an update that asks for more is confirmed again), enable, disable, remove.
     Owner: wave 5, strand A. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { ImportSource } from "../commands/backend";
  import type { Manifest } from "../commands/types";
  import { readFiles } from "../import/report";
  import { getApp } from "../state/app.svelte";
  import { describePermissions, installPlan, type InstallPlan } from "./permissions";
  import { pluginsState } from "./store.svelte";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();
  const app = getApp();
  const plugins = pluginsState(app.backend);

  let error = $state<string | null>(null);
  let busy = $state(false);
  let pending = $state.raw<{ file: ImportSource; manifest: Manifest; plan: InstallPlan } | null>(null);
  let removing = $state<string | null>(null);
  let fileInput = $state<HTMLInputElement | null>(null);

  function opened(node: HTMLDialogElement) {
    node.showModal();
    untrack(() => {
      error = null;
      pending = null;
      removing = null;
      void plugins.refresh();
    });
    return () => node.close();
  }

  async function act(f: () => Promise<void>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await f();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function picked(list: FileList | null) {
    const files = list ? await readFiles(list) : [];
    if (fileInput) fileInput.value = "";
    const file = files[0];
    if (!file) return;
    await act(async () => {
      pending = null;
      const manifest = await app.backend.plugins.inspect(file);
      pending = { file, manifest, plan: installPlan(manifest, plugins.list) };
    });
  }

  const install = () =>
    act(async () => {
      if (!pending) return;
      await app.backend.plugins.install(pending.file);
      pending = null;
      await plugins.refresh();
    });

  const setEnabled = (id: string, enabled: boolean) =>
    act(async () => {
      await app.backend.plugins.setEnabled(id, enabled);
      await plugins.refresh();
    });

  const remove = (id: string) =>
    act(async () => {
      await app.backend.plugins.remove(id);
      removing = null;
      await plugins.refresh();
    });
</script>

{#if open}
  <dialog
    data-testid="plugins-dialog"
    aria-labelledby="plugins-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <header>
      <h2 id="plugins-title">Plugins</h2>
      <button data-testid="plugins-close" onclick={onclose}>Close</button>
    </header>

    <div class="body">
      {#if error}<p class="error" role="alert" data-testid="plugins-error">{error}</p>{/if}

      {#if pending}
        {@const m = pending.manifest}
        {@const plan = pending.plan}
        <section class="confirm" data-testid="plugins-confirm" aria-label="Confirm installation">
          <h3>
            {#if plan.kind === "update"}
              Update {m.name} from {plan.from} to {plan.to}?
            {:else}
              Install {m.name} {m.version}?
            {/if}
          </h3>
          {#if m.description}<p class="muted">{m.description}</p>{/if}
          <p class="muted">
            <code>{m.id}</code>{#if m.license} · {m.license}{/if}{#if m.homepage} · {m.homepage}{/if}
          </p>
          {#if plan.kind === "new"}
            {#if plan.permissions.length === 0}
              <p data-testid="plugins-permissions-none">This plugin asks for no permissions.</p>
            {:else}
              <p>This plugin asks for:</p>
              <ul data-testid="plugins-permissions">
                {#each plan.permissions as p (p)}<li>{p}</li>{/each}
              </ul>
            {/if}
          {:else if plan.added.length > 0}
            <p class="warn">This update asks for more than you granted before:</p>
            <ul data-testid="plugins-permissions-added">
              {#each plan.added as p (p)}<li>{p}</li>{/each}
            </ul>
          {:else}
            <p data-testid="plugins-permissions-same">No new permissions.</p>
          {/if}
          <p class="muted">Plugins never see secret values. Their network access is limited to the hosts listed.</p>
          <div class="row">
            <button class="primary" data-testid="plugins-confirm-install" disabled={busy} onclick={install}>
              {plan.kind === "update" ? (plan.added.length > 0 ? "Grant and update" : "Update") : "Install"}
            </button>
            <button data-testid="plugins-confirm-cancel" disabled={busy} onclick={() => (pending = null)}>Cancel</button>
          </div>
        </section>
      {:else}
        <div class="row">
          <label class="pick">
            <span>Install from file…</span>
            <input
              bind:this={fileInput}
              data-testid="plugins-file"
              type="file"
              accept=".wasm,application/wasm"
              disabled={busy}
              onchange={(e) => picked(e.currentTarget.files)}
            />
          </label>
        </div>
      {/if}

      {#if plugins.list.length === 0}
        <p class="muted" data-testid="plugins-empty">
          {plugins.error ?? "No plugins installed. A plugin is a single .wasm file."}
        </p>
      {:else}
        <ul class="list" data-testid="plugins-list">
          {#each plugins.list as p (p.manifest.id)}
            {@const perms = describePermissions(p.manifest.permissions)}
            <li data-testid="plugin-item" data-plugin={p.manifest.id}>
              <div class="title">
                <strong>{p.manifest.name}</strong>
                <span class="muted" data-testid="plugin-version">{p.manifest.version}</span>
                <label class="toggle">
                  <input
                    type="checkbox"
                    data-testid="plugin-enabled"
                    checked={p.enabled}
                    disabled={busy}
                    onchange={(e) => setEnabled(p.manifest.id, e.currentTarget.checked)}
                  />
                  Enabled
                </label>
                {#if removing === p.manifest.id}
                  <button class="danger" data-testid="plugin-remove-confirm" disabled={busy} onclick={() => remove(p.manifest.id)}>
                    Remove {p.manifest.name}
                  </button>
                  <button disabled={busy} onclick={() => (removing = null)}>Keep</button>
                {:else}
                  <button data-testid="plugin-remove" disabled={busy} onclick={() => (removing = p.manifest.id)}>Remove</button>
                {/if}
              </div>
              {#if p.manifest.description}<p class="muted">{p.manifest.description}</p>{/if}
              <p class="muted perms">
                {perms.length === 0 ? "No permissions" : perms.join(" · ")}
                {#if !p.granted}<span class="warn"> (not granted; reinstall to grant)</span>{/if}
              </p>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(640px, calc(100vw - 32px));
    padding: 0;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }
  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 1em;
  }
  h3 {
    margin: 0;
    font-size: 1em;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) * 2);
    max-height: 70vh;
    overflow: auto;
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .warn {
    color: var(--rt-warn);
  }
  .error {
    color: var(--rt-danger);
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
  }
  .confirm ul {
    margin: 0;
    padding-left: 1.4em;
  }
  .row {
    display: flex;
    gap: var(--rt-gap);
  }
  .pick {
    position: relative;
    display: inline-block;
    padding: 4px var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .pick input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .list li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--rt-gap) 0;
    border-top: 1px solid var(--rt-border);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  .title strong {
    flex: 0 1 auto;
  }
  .toggle {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--rt-size-small);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
    cursor: pointer;
  }
  button.primary {
    color: var(--rt-accent-fg);
    background: var(--rt-accent);
    border-color: var(--rt-accent);
  }
  button.danger {
    color: var(--rt-danger);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
