<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Import dialog: drop or pick files (or paste cURL / .http text), see the loss report, then
     write the new collection into a folder (`backend.imports`) and open it. Nothing is
     written before "Choose folder and import". Owner: wave 4, strand B. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { CollectionInfo, ImportSource } from "../commands/backend";
  import type { ImportFormat, ImportPlan } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { FORMATS, formatName, groupLosses, pasted, readFiles, reportText, summary, type LossGroup } from "./report";

  let {
    open,
    onclose,
    onopen,
  }: { open: boolean; onclose: () => void; onopen: (info: CollectionInfo) => Promise<void> } = $props();
  const app = getApp();

  let format = $state<ImportFormat | "auto">("auto");
  let text = $state("");
  let dragging = $state(false);
  let busy = $state<null | "analyzing" | "writing">(null);
  let error = $state<string | null>(null);
  let plan = $state<ImportPlan | null>(null);
  let sources = $state<string[]>([]);
  let copied = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);

  const groupCounts = (g: LossGroup) =>
    [String(g.losses.length), g.dropped && `${g.dropped} dropped`, g.approximated && `${g.approximated} approximated`]
      .filter(Boolean)
      .join(", ");
  const groups = $derived(plan ? groupLosses(plan.report.losses) : []);
  const dropped = $derived(plan ? plan.report.losses.filter((l) => l.effect === "dropped").length : 0);

  function reset() {
    format = "auto";
    text = "";
    dragging = false;
    busy = null;
    error = null;
    plan = null;
    sources = [];
    copied = false;
  }

  function opened(node: HTMLDialogElement) {
    node.showModal();
    untrack(reset);
    return () => node.close();
  }

  async function discardPlan() {
    const id = plan?.id;
    plan = null;
    if (id !== undefined) await app.backend.imports.discard(id).catch(() => {});
  }

  async function close() {
    await discardPlan();
    onclose();
  }

  async function analyze(files: ImportSource[]) {
    if (files.length === 0 || busy) return;
    await discardPlan();
    busy = "analyzing";
    error = null;
    copied = false;
    try {
      plan = await app.backend.imports.analyze(files, format === "auto" ? null : format);
      sources = files.map((f) => f.name);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }

  async function picked(list: FileList | null) {
    if (list && list.length > 0) await analyze(await readFiles(list));
    if (fileInput) fileInput.value = "";
  }

  async function dropped_(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    await picked(e.dataTransfer?.files ?? null);
  }

  async function copy() {
    if (!plan) return;
    try {
      await navigator.clipboard.writeText(reportText(plan, sources));
      copied = true;
    } catch {
      error = "The clipboard is not available; select the report and copy it by hand.";
    }
  }

  async function write() {
    if (!plan || busy) return;
    busy = "writing";
    error = null;
    try {
      const info = await app.backend.imports.write(plan.id);
      if (info) {
        plan = null;
        onclose();
        await onopen(info);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }
</script>

{#if open}
  <dialog
    data-testid="import-dialog"
    aria-labelledby="import-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      void close();
    }}
  >
    <header>
      <h2 id="import-title">Import</h2>
      <button data-testid="import-close" onclick={close}>Close</button>
    </header>

    {#if error}<p class="error" role="alert" data-testid="import-error">{error}</p>{/if}

    {#if !plan}
      <div class="body">
        <p class="hint">
          Postman, Insomnia, OpenAPI, HAR, .http / .rest or cURL. Add environment files (Postman environments and
          globals, <code>http-client.env.json</code>) together with the collection.
        </p>
        <label
          class="drop"
          class:dragging
          data-testid="import-drop"
          ondragover={(e) => {
            e.preventDefault();
            dragging = true;
          }}
          ondragleave={() => (dragging = false)}
          ondrop={dropped_}
        >
          <span>Drop files here, or <u>choose files</u></span>
          <input
            bind:this={fileInput}
            data-testid="import-file"
            type="file"
            multiple
            onchange={(e) => picked(e.currentTarget.files)}
          />
        </label>

        <label class="field">
          <span>Or paste cURL commands or .http requests</span>
          <textarea
            data-testid="import-text"
            rows="5"
            spellcheck="false"
            placeholder={"curl https://api.example.com/users -H 'Accept: application/json'"}
            bind:value={text}
          ></textarea>
        </label>

        <div class="actions">
          <label class="format">
            Format
            <select data-testid="import-format" bind:value={format}>
              <option value="auto">Detect automatically</option>
              {#each FORMATS as f (f.value)}<option value={f.value}>{f.label}</option>{/each}
            </select>
          </label>
          <button
            data-testid="import-analyze-text"
            disabled={!text.trim() || busy !== null}
            onclick={() => analyze([pasted(text)])}
          >
            Convert pasted text
          </button>
        </div>
        {#if busy === "analyzing"}<p class="hint" role="status">Converting…</p>{/if}
      </div>
    {:else}
      <div class="body" data-testid="import-plan">
        <dl class="facts">
          <dt>Format</dt>
          <dd data-testid="import-detected">{formatName(plan.format)}</dd>
          <dt>Collection</dt>
          <dd data-testid="import-name">{plan.collection_name}</dd>
          <dt>Contents</dt>
          <dd data-testid="import-counts">{summary(plan)}</dd>
          <dt>From</dt>
          <dd>{sources.join(", ")}</dd>
        </dl>

        <section aria-labelledby="import-report-title" class="report">
          <div class="report-head">
            <h3 id="import-report-title">
              {#if plan.report.losses.length === 0}
                Nothing was lost
              {:else}
                Loss report: {plan.report.losses.length}
                <span class="muted">({dropped} dropped, {plan.report.losses.length - dropped} approximated)</span>
              {/if}
            </h3>
            <button data-testid="import-copy" onclick={copy}>{copied ? "Copied" : "Copy report"}</button>
          </div>
          {#if plan.report.losses.length === 0}
            <p data-testid="import-complete">Everything was imported exactly.</p>
          {:else}
            <p class="hint">
              <strong>Dropped</strong>: not in the new collection. <strong>Approximated</strong>: in it, but not exactly
              as in the source.
            </p>
            {#each groups as g (g.kind)}
              <details open={groups.length <= 3} data-testid="import-group" data-kind={g.kind}>
                <summary>
                  {g.label}
                  <span class="muted">{groupCounts(g)}</span>
                </summary>
                <ul>
                  {#each g.losses as l, i (i)}
                    <li data-testid="import-loss">
                      <span class="effect {l.effect}">{l.effect}</span>
                      <span class="where">{l.location}</span>
                      <span class="detail">{l.detail}</span>
                    </li>
                  {/each}
                </ul>
              </details>
            {/each}
          {/if}
        </section>
      </div>
      <footer>
        <button data-testid="import-back" disabled={busy !== null} onclick={discardPlan}>Back</button>
        <span class="muted">Creates the folder “{plan.folder_name}” in the folder you choose.</span>
        <button class="primary" data-testid="import-write" disabled={busy !== null} onclick={write}>
          {busy === "writing" ? "Importing…" : "Choose folder and import"}
        </button>
      </footer>
    {/if}
  </dialog>
{/if}

<style>
  dialog {
    width: min(760px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    padding: 0;
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }

  dialog[open] {
    display: flex;
    flex-direction: column;
  }

  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }

  header,
  footer {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    background: var(--rt-bg-panel);
  }

  header {
    border-bottom: 1px solid var(--rt-border);
  }

  footer {
    border-top: 1px solid var(--rt-border);
  }

  footer .muted {
    flex: 1;
  }

  h2,
  h3 {
    flex: 1;
    margin: 0;
    font-size: inherit;
  }

  .body {
    padding: var(--rt-gap) calc(var(--rt-gap) * 2);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }

  .hint,
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
    margin: 0;
  }

  .drop {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 96px;
    border: 2px dashed var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    cursor: pointer;
    position: relative;
  }

  .drop.dragging {
    border-color: var(--rt-accent);
    background: var(--rt-bg-selected);
  }

  .drop:focus-within {
    outline: 2px solid var(--rt-focus);
    outline-offset: 2px;
  }

  /* Visually hidden, still focusable and announced. */
  .drop input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  textarea {
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    resize: vertical;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    justify-content: space-between;
  }

  .format {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px calc(var(--rt-gap) * 2);
    margin: 0;
  }

  .facts dt {
    color: var(--rt-fg-muted);
  }

  .facts dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .report-head {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    margin-bottom: 4px;
  }

  details {
    border-top: 1px solid var(--rt-border);
    padding: 4px 0;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
  }

  summary .muted {
    font-weight: normal;
  }

  ul {
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 96px minmax(120px, 30%) 1fr;
    gap: var(--rt-gap);
    padding: 2px 0;
    font-size: var(--rt-size-small);
  }

  .effect {
    font-weight: 600;
    text-transform: uppercase;
    font-size: 11px;
  }

  .effect.dropped {
    color: var(--rt-danger);
  }

  .effect.approximated {
    color: var(--rt-warn);
  }

  .where {
    color: var(--rt-fg-muted);
    overflow-wrap: anywhere;
  }

  .error {
    margin: var(--rt-gap) calc(var(--rt-gap) * 2) 0;
    color: var(--rt-danger);
  }

  .primary {
    background: var(--rt-accent);
    color: var(--rt-accent-fg);
    border-color: var(--rt-accent);
  }
</style>
