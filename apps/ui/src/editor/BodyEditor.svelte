<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Request body: none, raw (json, xml, text, sparql), form-url-encoded, multipart, file. -->
<script lang="ts">
  import type { Body, MultipartPart, PartKind, RawKind } from "../commands/types";
  import CodeField from "./CodeField.svelte";
  import type { Language } from "./codemirror";
  import KeyValueTable, { type Row } from "./KeyValueTable.svelte";
  import type { VarLookup } from "./vars";

  type BodyChoice = "none" | RawKind | "form-url-encoded" | "multipart" | "file";

  let {
    body,
    lookup,
    stash,
    onchange,
  }: {
    body: Body;
    lookup: VarLookup;
    /** Bodies of other types, kept while switching until the request is saved. */
    stash: Map<string, unknown>;
    /** `typing`: debounce the edit. */
    onchange: (body: Body, typing: boolean) => void;
  } = $props();

  const CHOICES: [BodyChoice, string][] = [
    ["none", "No body"],
    ["json", "JSON"],
    ["xml", "XML"],
    ["text", "Text"],
    ["sparql", "SPARQL"],
    ["form-url-encoded", "Form URL encoded"],
    ["multipart", "Multipart form"],
    ["file", "File"],
  ];
  const RAW: RawKind[] = ["json", "xml", "text", "sparql"];

  const choice = $derived<BodyChoice>(body.type === "raw" ? body.kind : body.type);
  const lang = $derived<Language>(body.type === "raw" && (body.kind === "json" || body.kind === "xml") ? body.kind : "text");

  function choose(next: BodyChoice) {
    if (next === choice) return;
    stash.set(`body:${body.type}`, body);
    if ((RAW as string[]).includes(next)) {
      const raw = body.type === "raw" ? body : (stash.get("body:raw") as Body | undefined);
      onchange({ type: "raw", kind: next as RawKind, data: raw?.type === "raw" ? raw.data : "" }, false);
      return;
    }
    const kept = stash.get(`body:${next}`) as Body | undefined;
    const fresh: Record<string, Body> = {
      none: { type: "none" },
      "form-url-encoded": { type: "form-url-encoded", fields: [] },
      multipart: { type: "multipart", parts: [] },
      file: { type: "file", path: "", content_type: null },
    };
    onchange(kept ?? fresh[next], false);
  }

  // ---- form fields ----
  const fields = $derived(body.type === "form-url-encoded" ? body.fields : []);
  function setFields(next: Body & { type: "form-url-encoded" }, typing: boolean) {
    onchange(next, typing);
  }

  // ---- multipart ----
  const parts = $derived(body.type === "multipart" ? body.parts : []);
  /** File parts may carry several paths; they are edited as one `;`-separated text. */
  const partValue = (p: MultipartPart) => (p.kind === "file" ? p.values.join(";") : (p.values[0] ?? ""));
  const partRows = $derived<Row[]>(parts.map((p) => ({ name: p.name, value: partValue(p), enabled: p.enabled })));

  function setPart(i: number, patch: Partial<MultipartPart>, typing: boolean) {
    const next = parts.map((p, j) => (j === i ? { ...p, ...patch } : p));
    onchange({ type: "multipart", parts: next }, typing);
  }
  function partRow(i: number, patch: Partial<Row>) {
    const p = parts[i];
    const out: Partial<MultipartPart> = {};
    if (patch.name !== undefined) out.name = patch.name;
    if (patch.enabled !== undefined) out.enabled = patch.enabled;
    if (patch.value !== undefined) {
      out.values = p.kind === "file" ? (patch.value === "" ? [] : patch.value.split(";")) : [patch.value];
    }
    setPart(i, out, patch.enabled === undefined);
  }
  function partKind(i: number, kind: PartKind) {
    const p = parts[i];
    setPart(i, { kind, values: kind === "text" && p.values.length === 0 ? [""] : p.values }, false);
  }

  // ---- file ----
  const file = $derived(body.type === "file" ? body : { type: "file" as const, path: "", content_type: null });
</script>

<div class="body">
  <label class="type">
    <span>Body</span>
    <select data-testid="body-type" value={choice} onchange={(e) => choose(e.currentTarget.value as BodyChoice)}>
      {#each CHOICES as [value, text] (value)}
        <option {value}>{text}</option>
      {/each}
    </select>
  </label>

  {#if body.type === "none"}
    <p class="hint">This request has no body.</p>
  {:else if body.type === "raw"}
    <CodeField
      testid="body-editor"
      label="Body"
      multiline
      {lang}
      {lookup}
      value={body.data}
      onchange={(data) => body.type === "raw" && onchange({ ...body, data }, true)}
    />
  {:else if body.type === "form-url-encoded"}
    <KeyValueTable
      testid="form-table"
      label="Form field"
      rows={fields}
      {lookup}
      onchange={(i, patch) =>
        setFields({ type: "form-url-encoded", fields: fields.map((f, j) => (j === i ? { ...f, ...patch } : f)) }, patch.enabled === undefined)}
      onadd={() => setFields({ type: "form-url-encoded", fields: [...fields, { name: "", value: "", enabled: true }] }, false)}
      onremove={(i) => setFields({ type: "form-url-encoded", fields: fields.filter((_, j) => j !== i) }, false)}
    />
  {:else if body.type === "multipart"}
    <KeyValueTable
      testid="multipart-table"
      label="Part"
      rows={partRows}
      {lookup}
      valuePlaceholder="value, or file path(s) separated by ;"
      onchange={partRow}
      onadd={() =>
        onchange(
          { type: "multipart", parts: [...parts, { name: "", kind: "text", values: [""], content_type: null, enabled: true }] },
          false,
        )}
      onremove={(i) => onchange({ type: "multipart", parts: parts.filter((_, j) => j !== i) }, false)}
    >
      {#snippet extraHeader()}<span>Kind · Content type</span>{/snippet}
      {#snippet extra(i: number)}
        <select
          data-testid="part-kind"
          aria-label={`Kind of part ${i + 1}`}
          value={parts[i].kind}
          onchange={(e) => partKind(i, e.currentTarget.value as PartKind)}
        >
          <option value="text">Text</option>
          <option value="file">File</option>
        </select>
        <CodeField
          testid="part-content-type"
          label={`Content type of part ${i + 1}`}
          placeholder="auto"
          value={parts[i].content_type ?? ""}
          {lookup}
          onchange={(v) => setPart(i, { content_type: v === "" ? null : v }, true)}
        />
      {/snippet}
    </KeyValueTable>
  {:else if body.type === "file"}
    <div class="file">
      <label>
        <span>Path</span>
        <CodeField
          testid="body-file-path"
          label="Body file path"
          placeholder="path relative to the collection"
          value={file.path}
          {lookup}
          onchange={(path) => onchange({ ...file, path }, true)}
        />
      </label>
      <label>
        <span>Content type</span>
        <CodeField
          testid="body-file-content-type"
          label="Body file content type"
          placeholder="auto"
          value={file.content_type ?? ""}
          {lookup}
          onchange={(v) => onchange({ ...file, content_type: v === "" ? null : v }, true)}
        />
      </label>
    </div>
  {/if}
</div>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  .type,
  .file label {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
  }
  .type span,
  .file span {
    color: var(--rt-fg-muted);
    min-width: 6em;
  }
  .file {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hint {
    color: var(--rt-fg-muted);
    margin: 0;
  }
  select {
    font: inherit;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    padding: 2px 4px;
  }
</style>
