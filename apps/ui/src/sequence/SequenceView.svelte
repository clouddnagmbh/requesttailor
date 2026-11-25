<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Sequence diagrams (wave 19, strand G): what a run, a session, a paged send or an agent
     really sent, as participants and arrows. Values stay out unless "Include values" is on, and
     then only short masked excerpts (the diagram says they may contain personal data). Export is
     text: Mermaid or a Markdown block to paste into a pull request; nothing is posted. -->
<script lang="ts">
  import { onMount } from "svelte";

  import type { SequenceModel, SequenceOptions, SequenceSource, SequenceText } from "../commands/types";
  import type { ToolProps } from "../shell/tool-host.svelte";
  import { Banner, Button, Checkbox, EmptyState, Select, Spinner } from "../ui";
  import { omittedText, savedName, sourceChoices } from "./model";
  import { presetHolder } from "./open.svelte";
  import SequenceDiagram from "./SequenceDiagram.svelte";

  let { app }: ToolProps = $props();

  const choices = $derived(sourceChoices(app.collection ? app.active : null));
  let choice = $state<string>("run");
  let includeQuery = $state(false);
  let includeValues = $state(false);

  let model = $state<SequenceModel | null>(null);
  let text = $state<SequenceText | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  let selected = $state<number | null>(null);
  let seq = 0;

  const options = $derived<SequenceOptions>({ include_query: includeQuery, include_values: includeValues });
  const source = $derived<SequenceSource>(choices.find((c) => c.id === choice)?.source ?? { kind: "session" });
  const empty = $derived(model !== null && model.messages.length === 0);
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  onMount(() => {
    const preset = presetHolder.take();
    if (preset) {
      const found = sourceChoices(app.collection ? app.active : null).find((c) => c.source.kind === preset.kind);
      if (found) {
        // Keep the preset's own numbers (a run id, a page request).
        choice = found.id;
        presetSource = preset;
      }
    }
    lastKey = key;
    void load();
  });

  const key = $derived(`${choice}|${includeQuery}|${includeValues}`);
  let lastKey: string | null = null;
  // Choosing another source or switching a box reads the model again.
  $effect(() => {
    const now = key;
    if (lastKey !== null && now !== lastKey) {
      lastKey = now;
      void load();
    }
  });

  /** A preset with its own numbers; used once for the first load. */
  let presetSource: SequenceSource | null = null;

  async function load() {
    const mine = ++seq;
    loading = true;
    error = null;
    status = null;
    try {
      const use = presetSource ?? source;
      presetSource = null;
      const m = await app.backend.sequence.model(use, options);
      const t = await app.backend.sequence.render(m, options);
      if (mine !== seq) return;
      model = m;
      text = t;
      selected = null;
    } catch (e) {
      if (mine === seq) {
        error = message(e);
        model = null;
        text = null;
      }
    } finally {
      if (mine === seq) loading = false;
    }
  }

  async function copy(what: "markdown" | "mermaid") {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(what === "markdown" ? text.markdown : text.mermaid);
      status = what === "markdown" ? "Copied as Markdown. Paste it into a pull request description." : "Copied the Mermaid text.";
    } catch {
      status = "The browser did not allow copying. Use “Save as .md” instead.";
    }
  }

  async function save() {
    if (!text) return;
    const name = savedName(choice, new Date());
    try {
      await app.backend.companions.stage(name, text.markdown);
      void app.refreshDirty();
      status = `Added ${name} to the collection. Save the collection (Cmd/Ctrl+S) to write it.`;
    } catch (e) {
      status = message(e);
    }
  }

  async function clear() {
    await app.backend.sequence.clear().catch(() => undefined);
    await load();
  }

  function open(path: string) {
    const collection = app.collection?.id;
    if (collection) void app.selectRef({ collection, path });
  }

  const omitted = $derived(model ? omittedText(model) : null);
</script>

<section class="sequence" data-testid="sequence-panel" aria-label="Sequence diagram">
  <header class="top">
    <h2>Sequence diagram</h2>
    <p class="note">What went over the wire, as participants and arrows. Paste the Markdown into a pull request description.</p>
    <div class="bar">
      <Select
        label="Source"
        bind:value={choice}
        options={choices.map((c) => ({ value: c.id, label: c.label }))}
        data-testid="sequence-source"
      />
      <Checkbox label="Include query" bind:checked={includeQuery} data-testid="sequence-query" />
      <Checkbox label="Include values" bind:checked={includeValues} data-testid="sequence-values" />
      <span class="push"></span>
      <Button size="sm" variant="secondary" icon="copy" disabled={!text || empty} onclick={() => copy("markdown")} data-testid="sequence-copy">Copy as Markdown</Button>
      <Button size="sm" variant="ghost" icon="code" disabled={!text || empty} onclick={() => copy("mermaid")} data-testid="sequence-copy-mermaid">Copy Mermaid</Button>
      <Button size="sm" variant="ghost" icon="file" disabled={!text || empty || !app.collection} onclick={save} data-testid="sequence-save">Save as .md</Button>
      {#if choice === "session"}
        <Button size="sm" variant="ghost" icon="delete" onclick={clear} data-testid="sequence-clear">Clear</Button>
      {/if}
    </div>
    {#if includeValues}
      <Banner tone="warn" data-testid="sequence-values-warning">
        Values may contain personal data. Short excerpts of ids, fields and failed checks are shown, already masked and checked for secrets; read the diagram before you share it.
      </Banner>
    {/if}
  </header>

  <div class="body">
    {#if loading && !model}
      <div class="center"><Spinner label="Reading…" /></div>
    {:else if error}
      <Banner tone="danger" data-testid="sequence-error">{error}</Banner>
    {:else if empty}
      <EmptyState
        icon="diagram"
        title="Nothing recorded yet"
        text="Run a folder or send a request, then open this view. The diagram is built from what was really sent."
        data-testid="sequence-empty"
      />
    {:else if model}
      <SequenceDiagram {model} bind:selected onopen={open} oncopy={() => void copy("markdown")} />
      <p class="foot" data-testid="sequence-foot">
        {model.messages.length} messages, {model.participants.length} participants.
        {#if omitted}<span data-testid="sequence-omitted">{omitted}</span>{/if}
        Arrow keys move between messages, Enter opens the request, c copies.
      </p>
    {/if}
    {#if status}<p class="status" role="status" data-testid="sequence-status">{status}</p>{/if}
  </div>
</section>

<style>
  .sequence {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-4);
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
    padding: var(--rt-space-6);
  }
  h2 {
    margin: 0;
    font: var(--rt-font-title);
  }
  .note {
    margin: var(--rt-space-2) 0 var(--rt-space-3);
    color: var(--rt-fg-muted);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--rt-space-4);
  }
  .push {
    flex: 1;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-space-3);
    flex: 1;
    min-height: 0;
  }
  .center {
    display: grid;
    place-items: center;
    flex: 1;
  }
  .foot,
  .status {
    margin: 0;
    color: var(--rt-fg-muted);
    font: var(--rt-font-small);
  }
</style>
