<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Arguments of an MCP tool call (wave 8, strand D): a form from the tool's input JSON
     Schema (schema.ts), or raw JSON (always available, the only way for schemas without a
     form). Text may hold {{variables}}, resolved when calling. -->
<script lang="ts">
  import { fieldText, missing, parseField, parseRaw, withArg, type Field } from "./schema";

  let {
    fields,
    args,
    raw,
    rawText,
    onargs,
    onraw,
    onmode,
  }: {
    /** `null`: the schema has no form. */
    fields: Field[] | null;
    args: Record<string, unknown>;
    raw: boolean;
    rawText: string;
    onargs: (args: Record<string, unknown>) => void;
    onraw: (text: string, parsed: Record<string, unknown> | null) => void;
    onmode: (raw: boolean) => void;
  } = $props();

  let fieldErrors = $state<Record<string, string | null>>({});
  const rawError = $derived(parseRaw(rawText).error);
  const absent = $derived(fields ? missing(fields, args) : []);

  function typed(field: Field, text: string) {
    const { value, error } = parseField(field, text);
    fieldErrors[field.name] = error;
    onargs(withArg(args, field.name, value));
  }
</script>

<div class="args" data-testid="mcp-args">
  <div class="mode" role="group" aria-label="Arguments">
    <button type="button" class="seg" aria-pressed={!raw} disabled={fields === null} data-testid="mcp-args-form" onclick={() => onmode(false)}>Form</button>
    <button type="button" class="seg" aria-pressed={raw} data-testid="mcp-args-raw" onclick={() => onmode(true)}>JSON</button>
    {#if fields === null}<span class="muted">No form for this schema; edit the arguments as JSON.</span>{/if}
  </div>
  {#if raw || fields === null}
    <textarea
      class="raw"
      spellcheck="false"
      rows="8"
      aria-label="Arguments as JSON"
      data-testid="mcp-args-json"
      value={rawText}
      oninput={(e) => {
        const text = (e.currentTarget as HTMLTextAreaElement).value;
        onraw(text, parseRaw(text).args);
      }}
    ></textarea>
    {#if rawError}<p class="error" data-testid="mcp-args-json-error">{rawError}</p>{/if}
  {:else if fields.length === 0}
    <p class="muted">This tool takes no arguments.</p>
  {:else}
    {#each fields as f (f.name)}
      <label class="field" data-testid="mcp-arg" data-name={f.name}>
        <span class="name">{f.name}{#if f.required}<span class="req" title="required">*</span>{/if}</span>
        {#if f.kind === "boolean"}
          <input
            type="checkbox"
            data-testid="mcp-arg-input"
            checked={args[f.name] === true}
            onchange={(e) => onargs(withArg(args, f.name, (e.currentTarget as HTMLInputElement).checked))}
          />
        {:else if f.kind === "enum"}
          <select data-testid="mcp-arg-input" value={fieldText(args, f)} onchange={(e) => typed(f, (e.currentTarget as HTMLSelectElement).value)}>
            <option value="">—</option>
            {#each f.options as o (o)}<option value={o}>{o}</option>{/each}
          </select>
        {:else}
          <input
            type="text"
            spellcheck="false"
            data-testid="mcp-arg-input"
            placeholder={f.kind === "json" ? "JSON" : f.kind}
            value={fieldText(args, f)}
            oninput={(e) => typed(f, (e.currentTarget as HTMLInputElement).value)}
          />
        {/if}
        {#if f.description}<span class="muted desc">{f.description}</span>{/if}
        {#if fieldErrors[f.name]}<span class="error">{fieldErrors[f.name]}</span>{/if}
      </label>
    {/each}
    {#if absent.length > 0}<p class="muted" data-testid="mcp-args-missing">Required: {absent.join(", ")}</p>{/if}
  {/if}
</div>

<style>
  .args {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mode {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .seg {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
  }
  .seg[aria-pressed="true"] {
    background: var(--rt-bg-selected);
  }
  .seg:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(80px, 160px) 1fr;
    gap: 2px 8px;
    align-items: center;
  }
  .field .desc,
  .field .error {
    grid-column: 2;
    font-size: var(--rt-size-small);
  }
  .name {
    font-family: var(--rt-font-mono);
  }
  .req {
    color: var(--rt-danger);
  }
  input[type="text"],
  select,
  .raw {
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    padding: 3px 6px;
    min-width: 0;
  }
  input[type="checkbox"] {
    justify-self: start;
  }
  .raw {
    font-family: var(--rt-font-mono);
    resize: vertical;
  }
  .muted {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  .error {
    color: var(--rt-danger);
    margin: 0;
  }
</style>
