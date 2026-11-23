<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The request editor's "Scripts" tab (wave 7, strand C): pre-request, post-response and
     tests scripts of `RequestSpec.scripts`, one JavaScript editor each. The script API is
     `crates/rt-script/api.d.ts`; `bru.*` and `pm.*` work through shims. -->
<script lang="ts">
  import SuggestTestsButton from "../ai/SuggestTestsButton.svelte";
  import type { Script, ScriptPhase } from "../commands/types";
  import { PHASES, scriptCode, withScriptCode } from "./edit";
  import ScriptEditor from "./ScriptEditor.svelte";

  let {
    scripts,
    onchange,
  }: {
    scripts: Script[];
    /** Typing: the editor debounces. */
    onchange: (scripts: Script[]) => void;
  } = $props();

  function set(phase: ScriptPhase, code: string) {
    onchange(withScriptCode(scripts, phase, code));
  }
</script>

<div class="scripts" data-testid="scripts-tab">
  <SuggestTestsButton />
  {#each PHASES as p (p.phase)}
    <section aria-labelledby="rt-script-{p.phase}">
      <h3 id="rt-script-{p.phase}">{p.label}</h3>
      <ScriptEditor
        editor={{ kind: "script", phase: p.phase }}
        testid="script-{p.phase}"
        label="{p.label} script"
        placeholder={p.hint}
        value={scriptCode(scripts, p.phase)}
        onchange={(code) => set(p.phase, code)}
      />
    </section>
  {/each}
  <p class="hint">
    Scripts run in a sandbox without file or network access (except <code>rt.sendRequest</code>), and only in a collection you
    trust. Variables they set live for the session. <code>bru.*</code> and <code>pm.*</code> work too.
  </p>
</div>

<style>
  .scripts {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h3 {
    margin: 0;
    font-size: var(--rt-size-small);
    font-weight: 600;
    color: var(--rt-fg-muted);
    text-transform: uppercase;
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
</style>
