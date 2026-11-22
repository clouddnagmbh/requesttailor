<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- One plugin tab: renders it through `backend.plugins`, runs its actions, keeps its opaque
     state per tab and request while the page lives, and applies `update_request` as an
     unsaved edit. Used by the request editor, the response panel and (wave 6) the collection
     panel. Wave 6: the selected environment goes along (the plugin's `context`; a change
     renders again), and `create_requests` / `add_environments` of any reply are shown for
     confirmation and then added as unsaved changes. Owner: wave 5, strand A; wave 6,
     strand F. -->
<script module lang="ts">
  /** Tab state by `plugin/tab/request path`. */
  const states = new Map<string, unknown>();
</script>

<script lang="ts">
  import { untrack } from "svelte";

  import type { Backend } from "../commands/backend";
  import type { RequestSpec, TabTarget, View, ViewReply } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import AdditionsDialog from "./AdditionsDialog.svelte";
  import { applyAdditions, hasAdditions, isEmptyPlan, planAdditions, type AdditionPlan } from "./additions";
  import PluginView from "./PluginView.svelte";
  import { pluginsState } from "./store.svelte";

  let {
    backend,
    plugin,
    tab,
    target,
    before,
    onupdate,
  }: {
    backend: Backend;
    plugin: string;
    tab: string;
    target: TabTarget;
    /** Runs before each call (the editor flushes pending typing). */
    before?: () => Promise<void> | void;
    /** `update_request` of a reply. */
    onupdate?: (spec: RequestSpec) => Promise<void> | void;
  } = $props();

  const app = getApp();
  let view = $state.raw<View | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let seq = 0;
  /** Additions of the last reply, waiting for the user's confirmation. */
  let proposal = $state.raw<AdditionPlan | null>(null);
  let adding = $state(false);
  let addError = $state<string | null>(null);

  const key = $derived(`${plugin}/${tab}/${target.request_path}`);
  const plugins = pluginsState(app.backend, false);
  const pluginLabel = $derived(plugins.list.find((p) => p.manifest.id === plugin)?.manifest.name ?? plugin);

  function propose(reply: ViewReply) {
    if (!hasAdditions(reply)) return;
    const plan = planAdditions(reply, app.tree, app.environments);
    if (isEmptyPlan(plan)) return;
    addError = null;
    proposal = plan;
  }

  async function confirmAdditions() {
    if (!proposal) return;
    adding = true;
    addError = null;
    try {
      await applyAdditions(backend, proposal);
      app.tree = await backend.tree();
      await app.reloadEnvironments();
      proposal = null;
    } catch (e) {
      addError = e instanceof Error ? e.message : String(e);
    } finally {
      adding = false;
    }
  }

  async function run(action: string | null, input: [string, string][]) {
    const k = key;
    const t = $state.snapshot(target) as TabTarget;
    const mine = ++seq;
    busy = true;
    try {
      await before?.();
      const state = states.get(k) ?? null;
      const environment = app.environment;
      const reply =
        action === null
          ? await backend.plugins.renderTab(plugin, tab, t, state, environment)
          : await backend.plugins.action(plugin, tab, action, t, state, input, environment);
      states.set(k, reply.state);
      if (mine !== seq) return;
      view = reply.view;
      error = null;
      if (reply.update_request) await onupdate?.(reply.update_request);
      propose(reply);
    } catch (e) {
      if (mine === seq) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (mine === seq) busy = false;
    }
  }

  $effect(() => {
    void key;
    void target.response_id;
    void app.environment;
    untrack(() => void run(null, []));
  });
</script>

<div class="plugin-tab" data-testid="plugin-tab" data-plugin={plugin} data-tab={tab} aria-busy={busy}>
  {#if error}
    <p class="error" role="alert" data-testid="plugin-tab-error">{error}</p>
  {/if}
  {#if view}
    <PluginView {view} {busy} onaction={(a, input) => void run(a, input)} />
  {:else if !error}
    <p class="note">Loading…</p>
  {/if}
</div>

{#if proposal}
  <AdditionsDialog
    plugin={pluginLabel}
    plan={proposal}
    busy={adding}
    error={addError}
    onconfirm={() => void confirmAdditions()}
    oncancel={() => (proposal = null)}
  />
{/if}

<style>
  .plugin-tab {
    overflow: auto;
    height: 100%;
  }
  .error {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-danger);
  }
  .note {
    margin: 0;
    padding: var(--rt-gap);
    color: var(--rt-fg-muted);
  }
</style>
