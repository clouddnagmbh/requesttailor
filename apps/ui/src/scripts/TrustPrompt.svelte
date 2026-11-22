<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Asks once per collection whether its scripts may run (wave 7, strand A).
     A banner, not a dialog: it takes no focus and blocks nothing, so sending stays at the
     same keystrokes. Until the user trusts the collection, sends run without scripts and
     their report carries a warning note. "Not now" hides the banner for this collection
     until the app is restarted. -->
<script lang="ts">
  import type { ScriptTrust } from "../commands/types";
  import { getApp } from "../state/app.svelte";

  const app = getApp();

  let trust = $state<ScriptTrust | null>(null);
  let busy = $state(false);
  let problem = $state<string | null>(null);
  /** Collections the user answered "Not now" for, in this session. */
  const later = $state<Record<string, true>>({});

  const id = $derived(app.collection?.id ?? null);

  // Asked again when the collection or its tree changes (open, reload, a new request).
  $effect(() => {
    const current = id;
    void app.tree;
    trust = null;
    problem = null;
    if (current === null) return;
    let stale = false;
    app.backend.scripts
      .trust()
      .then((t) => {
        if (!stale) trust = t;
      })
      .catch(() => {
        if (!stale) trust = null;
      });
    return () => {
      stale = true;
    };
  });

  const shown = $derived(id !== null && trust?.has_scripts === true && !trust.trusted && !later[id]);

  async function allow(): Promise<void> {
    busy = true;
    problem = null;
    try {
      await app.backend.scripts.setTrusted(true);
      trust = { has_scripts: true, trusted: true };
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function notNow(): void {
    if (id !== null) later[id] = true;
  }
</script>

{#if shown}
  <aside data-testid="script-trust" aria-live="polite" aria-label="Scripts in this collection">
    <p>This collection contains scripts. Run them?</p>
    <div class="actions">
      <button class="primary" data-testid="script-trust-allow" disabled={busy} onclick={allow}>
        Trust and run scripts
      </button>
      <button data-testid="script-trust-later" disabled={busy} onclick={notNow}>Not now</button>
    </div>
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  </aside>
{/if}

<style>
  aside {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
    align-items: center;
    justify-content: space-between;
    margin: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 1.5);
    border: 1px solid var(--rt-warn);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    color: var(--rt-fg);
  }
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    padding: 4px 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    color: var(--rt-fg);
  }
  button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  button.primary {
    border-color: var(--rt-accent);
    background: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  button:disabled {
    opacity: 0.6;
  }
  .problem {
    width: 100%;
    color: var(--rt-danger);
  }
</style>
