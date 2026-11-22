<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Saved messages of a WebSocket or gRPC request (wave 8, strand A; shared with gRPC):
     titled variants, exactly one selected. The selected variant is the one shown, edited and
     sent ("Send" in the session pane). `{{variables}}` are resolved when sent. -->
<script lang="ts">
  import type { MessageKind, MessageVariant } from "../commands/types";
  import CodeField from "../editor/CodeField.svelte";
  import type { Language } from "../editor/codemirror";
  import type { VarLookup } from "../editor/vars";
  import { emptyMessage } from "./kind";

  let {
    messages,
    lookup,
    kinds = ["text", "json", "xml", "binary"],
    onchange,
  }: {
    messages: MessageVariant[];
    lookup: VarLookup;
    /** Kinds offered (gRPC: JSON only). */
    kinds?: MessageKind[];
    /** `typing`: debounce the edit. */
    onchange: (messages: MessageVariant[], typing: boolean) => void;
  } = $props();

  const KIND_LABEL: Record<MessageKind, string> = { text: "Text", json: "JSON", xml: "XML", binary: "Binary (base64)" };

  const index = $derived(Math.max(0, messages.findIndex((m) => m.selected)));
  const current = $derived<MessageVariant | undefined>(messages[index]);
  const lang = $derived<Language>(current?.kind === "json" || current?.kind === "xml" ? current.kind : "text");

  const copy = () => messages.map((m) => ({ ...m }));

  function select(i: number) {
    if (i === index && messages[i]?.selected) return;
    onchange(copy().map((m, j) => ({ ...m, selected: j === i })), false);
  }
  function patch(change: Partial<MessageVariant>, typing: boolean) {
    const next = copy();
    if (next.length === 0) next.push(emptyMessage());
    next[index] = { ...next[index], ...change, selected: true };
    onchange(next, typing);
  }
  function add() {
    const titles = new Set(messages.map((m) => m.title));
    let n = messages.length + 1;
    while (titles.has(`Message ${n}`)) n++;
    const next = copy().map((m) => ({ ...m, selected: false }));
    // A lone untitled message gets a title once there are several.
    if (next.length === 1 && next[0].title === "") next[0].title = "Message 1";
    next.push({ ...emptyMessage(`Message ${n}`), kind: current?.kind ?? kinds[0] });
    onchange(next, false);
  }
  function remove(i: number) {
    const next = copy().filter((_, j) => j !== i);
    if (next.length > 0 && !next.some((m) => m.selected)) next[Math.min(i, next.length - 1)].selected = true;
    onchange(next, false);
  }
</script>

<div class="messages" data-testid="message-editor">
  <div class="variants" role="tablist" aria-label="Message variants">
    {#each messages as m, i (i)}
      <span class="variant" class:selected={i === index}>
        <button
          role="tab"
          data-testid="message-variant"
          aria-selected={i === index}
          title="Edit and send this message"
          onclick={() => select(i)}>{m.title || "Message"}</button
        >
        {#if messages.length > 1}
          <button class="remove" aria-label="Remove {m.title || 'message'}" onclick={() => remove(i)}>×</button>
        {/if}
      </span>
    {/each}
    <button class="add" data-testid="message-add" aria-label="Add message" title="Add a message variant" onclick={add}>+</button>
  </div>
  {#if current}
    <div class="row">
      {#if messages.length > 1 || current.title}
        <input
          class="title"
          data-testid="message-title"
          aria-label="Message title"
          placeholder="Title"
          value={current.title}
          oninput={(e) => patch({ title: e.currentTarget.value }, true)}
        />
      {/if}
      <select
        data-testid="message-kind"
        aria-label="Message type"
        value={current.kind}
        onchange={(e) => patch({ kind: e.currentTarget.value as MessageKind }, false)}
      >
        {#each kinds as k (k)}<option value={k}>{KIND_LABEL[k]}</option>{/each}
      </select>
    </div>
    {#key index}
      <CodeField
        testid="message-data"
        label="Message"
        multiline
        {lang}
        {lookup}
        placeholder={current.kind === "binary" ? "base64" : "Message, {{variables}} allowed"}
        value={current.data}
        onchange={(data) => patch({ data }, true)}
      />
    {/key}
  {:else}
    <p class="hint">No message yet. Add one to send it after connecting.</p>
  {/if}
</div>

<style>
  .messages {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .variants {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
  }
  .variant {
    display: inline-flex;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .variant.selected {
    border-color: var(--rt-accent);
  }
  button,
  input,
  select {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
  }
  .variant button {
    border: none;
    background: none;
    padding: 2px 8px;
    cursor: pointer;
  }
  .variant.selected button[role="tab"] {
    font-weight: 600;
  }
  .variant .remove {
    padding: 2px 6px;
    color: var(--rt-fg-muted);
  }
  .add {
    padding: 2px 8px;
    cursor: pointer;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .title {
    flex: 1;
    max-width: 320px;
    padding: 2px 6px;
  }
  select {
    padding: 2px 4px;
  }
  .hint {
    margin: 0;
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
</style>
