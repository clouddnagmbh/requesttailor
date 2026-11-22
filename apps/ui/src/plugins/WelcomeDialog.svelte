<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The first-start question (wave 6): which of the bundled plugins to enable. Shown once,
     only while `welcome()` says it is not answered and there is a choice. The default is
     "no extras"; Escape skips (no extras), Enter confirms the current choice. Bundled plugins
     nobody chose stay disabled: no tabs, no hooks, nothing about them in the UI.
     The shell checks after the first frame, so the cold start does not wait for it (the
     desktop installs the bundled plugins during that call). Owner: wave 6, strand F. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { WelcomeState } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { pluginsState } from "./store.svelte";

  const app = getApp();
  let welcome = $state.raw<WelcomeState | null>(null);
  let chosen = $state<string[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    let cancelled = false;
    // After the first frame: never on the cold start path.
    const frame = requestAnimationFrame(() =>
      setTimeout(() => {
        app.backend.plugins.welcome().then(
          (w) => {
            if (!cancelled && !w.done && w.choices.length > 0) welcome = w;
          },
          (e: unknown) => console.warn("RequestTailor: the first-start question is unavailable:", e),
        );
      }, 0),
    );
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  });

  function toggle(id: string, on: boolean) {
    chosen = on ? [...chosen.filter((c) => c !== id), id] : chosen.filter((c) => c !== id);
  }

  async function answer(ids: string[]) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await app.backend.plugins.chooseWelcome(ids);
      welcome = null;
      // The list changed only if something was enabled; listing loads the web core, so
      // only once a collection is open (otherwise the first use reads it fresh).
      if (ids.length > 0 && app.collection) await pluginsState(app.backend, false).refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    return () => node.close();
  }

  function onkeydown(e: KeyboardEvent) {
    // Enter confirms from anywhere in the dialog; a focused button handles it itself.
    if (e.key !== "Enter" || e.target instanceof HTMLButtonElement) return;
    e.preventDefault();
    void answer(chosen);
  }
</script>

{#if welcome}
  <dialog
    data-testid="welcome-dialog"
    aria-labelledby="welcome-title"
    {@attach opened}
    {onkeydown}
    oncancel={(e) => {
      e.preventDefault();
      void answer([]);
    }}
  >
    <header>
      <h2 id="welcome-title">What do you work with?</h2>
    </header>
    <div class="body">
      <p class="muted">Extras can be switched on or off later under “Manage plugins”.</p>
      <label class="option">
        <input type="checkbox" data-testid="welcome-none" checked={chosen.length === 0} disabled={busy} onchange={() => (chosen = [])} />
        <span>
          <strong>REST, GraphQL &amp; co. (no extras)</strong>
        </span>
      </label>
      {#each welcome.choices as c (c.plugin)}
        <label class="option">
          <input
            type="checkbox"
            data-testid="welcome-choice"
            data-plugin={c.plugin}
            checked={chosen.includes(c.plugin)}
            disabled={busy}
            onchange={(e) => toggle(c.plugin, e.currentTarget.checked)}
          />
          <span>
            <strong>{c.choice.label}</strong>
            {#if c.choice.description}<span class="muted">{c.choice.description}</span>{/if}
          </span>
        </label>
      {/each}
      {#if error}<p class="error" role="alert" data-testid="welcome-error">{error}</p>{/if}
      <div class="row">
        <button class="primary" data-testid="welcome-continue" disabled={busy} onclick={() => answer(chosen)}>Continue</button>
        <button data-testid="welcome-skip" disabled={busy} onclick={() => answer([])}>Skip</button>
      </div>
    </div>
  </dialog>
{/if}

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
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
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }
  h2 {
    margin: 0;
    font-size: 1em;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: calc(var(--rt-gap) * 2);
  }
  p {
    margin: 0;
  }
  .option {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-gap);
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    cursor: pointer;
  }
  .option span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .error {
    color: var(--rt-danger);
  }
  .row {
    display: flex;
    gap: var(--rt-gap);
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
</style>
