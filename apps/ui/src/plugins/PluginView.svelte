<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Renders a plugin's declarative `View` with the UI's own elements; plugin code never runs
     here and text is never markup. Buttons, tree items and form submits call `onaction`
     (a form submits as the action named by its `id`, with its values). Owner: wave 5,
     strand A. -->
<script lang="ts">
  import type { Block, FormField, TreeItem, View } from "../commands/types";
  import { Button } from "../ui";

  let {
    view,
    busy = false,
    onaction,
  }: { view: View; busy?: boolean; onaction: (action: string, input: [string, string][]) => void } = $props();

  /** Values of form fields the user changed, by `form/field`. */
  let values = $state<Record<string, string>>({});
  const valueOf = (form: string, f: FormField) => values[`${form}/${f.name}`] ?? f.value;

  /** The code block that was copied last (its index), shown as "Copied" for a moment. */
  let copied = $state<number | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copy(text: string, index: number) {
    try {
      await navigator.clipboard.writeText(text);
      copied = index;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = null), 1500);
    } catch {
      copied = null; // No clipboard (insecure context, denied): the text stays selectable.
    }
  }

  function submit(e: SubmitEvent, block: Extract<Block, { type: "form" }>) {
    e.preventDefault();
    onaction(
      block.id,
      block.fields.map((f) => [f.name, valueOf(block.id, f)]),
    );
  }
</script>

{#snippet tree(items: TreeItem[])}
  <ul>
    {#each items as item, i (i)}
      <li>
        {#if item.action}
          <button type="button" class="link" disabled={busy} onclick={() => onaction(item.action!, [])}>{item.label}</button>
        {:else}
          <span>{item.label}</span>
        {/if}
        {#if item.detail}<span class="muted">{item.detail}</span>{/if}
        {#if item.children.length > 0}{@render tree(item.children)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<div class="view" data-testid="plugin-view">
  {#each view.blocks as block, i (i)}
    {#if block.type === "heading"}
      <h3>{block.text}</h3>
    {:else if block.type === "text"}
      <p class="text">{block.text}</p>
    {:else if block.type === "notice"}
      <p class="notice {block.level}" role={block.level === "error" ? "alert" : "status"} data-testid="plugin-notice">{block.text}</p>
    {:else if block.type === "key-value"}
      <dl>
        {#each block.rows as [k, v], j (j)}<dt>{k}</dt><dd>{v}</dd>{/each}
      </dl>
    {:else if block.type === "table"}
      <div class="scroll">
        <table>
          <thead><tr>{#each block.columns as c, j (j)}<th>{c}</th>{/each}</tr></thead>
          <tbody>
            {#each block.rows as row, j (j)}<tr>{#each row as cell, k (k)}<td>{cell}</td>{/each}</tr>{/each}
          </tbody>
        </table>
      </div>
    {:else if block.type === "code"}
      <div class="code-block">
        <pre class="code" data-language={block.language ?? ""} data-testid="plugin-code">{block.text}</pre>
        <span class="copy">
          <Button variant="ghost" size="sm" icon="copy" data-testid="plugin-code-copy" onclick={() => copy(block.text, i)}>
            {copied === i ? "Copied" : "Copy"}
          </Button>
        </span>
      </div>
    {:else if block.type === "tree"}
      <div class="tree">{@render tree(block.nodes)}</div>
    {:else if block.type === "form"}
      <form onsubmit={(e) => submit(e, block)} data-testid="plugin-form">
        {#each block.fields as f (f.name)}
          {#if f.kind === "checkbox"}
            <label class="check">
              <input
                type="checkbox"
                checked={valueOf(block.id, f) === "true"}
                onchange={(e) => (values[`${block.id}/${f.name}`] = String(e.currentTarget.checked))}
              />
              {f.label}
            </label>
          {:else if f.kind === "select"}
            <label>
              <span>{f.label}</span>
              <select value={valueOf(block.id, f)} onchange={(e) => (values[`${block.id}/${f.name}`] = e.currentTarget.value)}>
                {#each f.options as o (o)}<option value={o}>{o}</option>{/each}
              </select>
            </label>
          {:else}
            <label>
              <span>{f.label}</span>
              <input
                type="text"
                spellcheck="false"
                value={valueOf(block.id, f)}
                oninput={(e) => (values[`${block.id}/${f.name}`] = e.currentTarget.value)}
              />
            </label>
          {/if}
        {/each}
        <button type="submit" disabled={busy}>{block.submit}</button>
      </form>
    {:else if block.type === "actions"}
      <div class="actions">
        {#each block.actions as a (a.id)}
          <button type="button" disabled={busy} onclick={() => onaction(a.id, [])}>{a.label}</button>
        {/each}
      </div>
    {/if}
  {/each}
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
    padding: var(--rt-gap);
    color: var(--rt-fg);
  }
  h3 {
    margin: 0;
    font-size: 1em;
  }
  .text {
    margin: 0;
    white-space: pre-wrap;
  }
  .notice {
    margin: 0;
    padding: 4px var(--rt-gap);
    border-left: 3px solid var(--rt-accent);
    background: var(--rt-bg-panel);
  }
  .notice.warning {
    border-color: var(--rt-warn);
  }
  .notice.error {
    border-color: var(--rt-danger);
    color: var(--rt-danger);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px var(--rt-gap);
    margin: 0;
  }
  dt {
    color: var(--rt-fg-muted);
  }
  dd {
    margin: 0;
    font-family: var(--rt-font-mono);
    word-break: break-all;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    font-size: var(--rt-size-small);
  }
  th,
  td {
    text-align: left;
    padding: 2px var(--rt-gap);
    border-bottom: 1px solid var(--rt-border);
  }
  .code-block {
    position: relative;
  }
  .copy {
    position: absolute;
    top: var(--rt-space-1);
    right: var(--rt-space-1);
  }
  .code {
    margin: 0;
    padding: var(--rt-gap);
    overflow: auto;
    font: var(--rt-size-small) var(--rt-font-mono);
    background: var(--rt-bg-panel);
    border-radius: var(--rt-radius);
  }
  .tree ul {
    margin: 0;
    padding-left: 1.2em;
  }
  .muted {
    margin-left: var(--rt-gap);
    color: var(--rt-fg-muted);
  }
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--rt-gap);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--rt-size-small);
  }
  label.check {
    flex-direction: row;
    align-items: center;
  }
  input[type="text"],
  select {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 6px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--rt-gap);
  }
  button {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg-panel);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px var(--rt-gap);
    cursor: pointer;
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    color: var(--rt-accent);
    text-decoration: underline;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
