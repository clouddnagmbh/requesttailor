<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The conflicts of a merge in progress, decided per field (wave 12, strand B): one row per
     conflicted field with ours and theirs to pick from, a file choice for files that do not
     merge on the model (text conflicts), and the files that merge cleanly. "Resolve" sends
     the choices (`MergeApi.resolve`; files with an undecided field stay conflicted) and
     reopens the collection. Used by `MergeView` (all files) and `git/ConflictView` (`only`:
     one file). Resolving changes files, so it is disabled while `blocked` says why. -->
<script lang="ts">
  import type { ChangeTarget, ConflictChoice, DiffValue, FieldConflict, MergeState, Side } from "../commands/types";
  import { getApp } from "../state/app.svelte";

  let {
    merge,
    only,
    blocked,
    onresolved,
  }: {
    merge: MergeState;
    /** Only this file. */
    only?: string;
    blocked: string | null;
    /** After a resolve, with the state that is left. */
    onresolved: (left: MergeState) => void;
  } = $props();
  const app = getApp();

  const fieldKey = (c: FieldConflict) => `${c.file}\u0000${JSON.stringify(c.field)}`;

  let fieldChoices = $state<Record<string, Side>>({});
  let fileChoices = $state<Record<string, Side>>({});
  let busy = $state(false);
  let error = $state<string | null>(null);

  const conflicts = $derived(merge.conflicts.filter((c) => only === undefined || c.file === only));
  const textFiles = $derived(merge.text_conflicts.filter((p) => only === undefined || p === only));
  const cleanFiles = $derived((merge.clean ?? []).filter((p) => only === undefined || p === only));
  const files = $derived.by(() => {
    const out = new Map<string, FieldConflict[]>();
    for (const c of conflicts) out.set(c.file, [...(out.get(c.file) ?? []), c]);
    return [...out];
  });
  const decided = $derived(conflicts.filter((c) => fieldChoices[fieldKey(c)]).length);
  const ready = $derived(
    cleanFiles.length > 0 ||
      textFiles.some((p) => fileChoices[p]) ||
      files.some(([, cs]) => cs.every((c) => fieldChoices[fieldKey(c)])),
  );

  function targetLabel(t: ChangeTarget): string {
    switch (t.kind) {
      case "collection":
        return "Collection settings";
      case "folder":
        return `Folder ${t.path}`;
      case "request":
        return t.path;
      case "environment":
        return `Environment ${t.name}`;
    }
  }

  const fieldLabel = (c: FieldConflict) => c.field.join(" › ");

  function shown(v: DiffValue | null): string {
    if (v === null) return "(not set)";
    if (v.kind === "secret") return "(secret value, hidden)";
    if (v.kind === "text") return v.text === "" ? "(empty)" : v.text;
    return JSON.stringify(v.value, null, 2);
  }

  function chooseAll(side: Side) {
    const next = { ...fieldChoices };
    for (const c of conflicts) next[fieldKey(c)] = side;
    fieldChoices = next;
    const files = { ...fileChoices };
    for (const p of textFiles) files[p] = side;
    fileChoices = files;
  }

  async function resolve() {
    const choices: ConflictChoice[] = [
      ...conflicts.flatMap((c): ConflictChoice[] => {
        const side = fieldChoices[fieldKey(c)];
        return side ? [{ kind: "field", target: c.target, field: c.field, side }] : [];
      }),
      ...textFiles.flatMap((p): ConflictChoice[] => (fileChoices[p] ? [{ kind: "file", path: p, side: fileChoices[p] }] : [])),
    ];
    busy = true;
    error = null;
    try {
      await app.backend.merges.resolve(choices);
      await app.reload();
      const left = await app.backend.merges.conflicts();
      fieldChoices = {};
      fileChoices = {};
      onresolved(left);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="fields" data-testid="merge-fields">
  {#if blocked}<p class="warn" role="status">{blocked}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if conflicts.length + textFiles.length > 1}
    <div class="all">
      <span class="muted">{decided} of {conflicts.length} field{conflicts.length === 1 ? "" : "s"} decided</span>
      <button class="button" data-testid="merge-all-ours" onclick={() => chooseAll("ours")}>All ours</button>
      <button class="button" data-testid="merge-all-theirs" onclick={() => chooseAll("theirs")}>All theirs</button>
    </div>
  {/if}

  {#each files as [file, cs] (file)}
    <section class="file" data-testid="merge-file" data-path={file} aria-labelledby="rt-merge-{file}">
      {#if only === undefined}<h3 id="rt-merge-{file}">{targetLabel(cs[0].target)}</h3>{/if}
      <div class="grid" role="group" aria-label="Conflicted fields of {file}">
        <span class="head">Field</span>
        <span class="head">Ours (this branch)</span>
        <span class="head">Theirs (incoming)</span>
        {#each cs as c (fieldKey(c))}
          {@const key = fieldKey(c)}
          <span class="field" data-testid="merge-field" data-field={c.field.join("/")}>{fieldLabel(c)}</span>
          {#each ["ours", "theirs"] as const as side (side)}
            <label class="value" class:chosen={fieldChoices[key] === side}>
              <input
                type="radio"
                name={key}
                value={side}
                data-testid="merge-choose-{side}"
                checked={fieldChoices[key] === side}
                onchange={() => (fieldChoices = { ...fieldChoices, [key]: side })}
              />
              <span class="text" class:secret={(side === "ours" ? c.ours : c.theirs)?.kind === "secret"}
                >{shown(side === "ours" ? c.ours : c.theirs)}</span
              >
            </label>
          {/each}
        {/each}
      </div>
    </section>
  {/each}

  {#each textFiles as p (p)}
    <section class="file" data-testid="merge-text-file" data-path={p}>
      {#if only === undefined}<h3>{p}</h3>{/if}
      <p class="muted">Does not merge field by field (e.g. deleted on one side). Keep one version of the file.</p>
      <div class="sides" role="radiogroup" aria-label="Version of {p}">
        {#each ["ours", "theirs"] as const as side (side)}
          <label class="value" class:chosen={fileChoices[p] === side}>
            <input
              type="radio"
              name="file:{p}"
              value={side}
              data-testid="merge-file-{side}"
              checked={fileChoices[p] === side}
              onchange={() => (fileChoices = { ...fileChoices, [p]: side })}
            />
            {side === "ours" ? "Keep ours" : "Keep theirs"}
          </label>
        {/each}
      </div>
    </section>
  {/each}

  {#if cleanFiles.length > 0}
    <p class="muted" data-testid="merge-clean">
      Merge field by field without a conflict: {cleanFiles.join(", ")}. Resolve writes them.
    </p>
  {/if}

  <div class="actions">
    <button class="button primary" data-testid="merge-resolve" disabled={!!blocked || busy || !ready} onclick={resolve}>
      {busy ? "Resolving…" : "Resolve"}
    </button>
    {#if conflicts.length > 0 && !ready}<span class="muted">Pick a side for each field of a file.</span>{/if}
  </div>
</div>

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: calc(var(--rt-gap) * 1.5);
  }
  .file {
    display: flex;
    flex-direction: column;
    gap: var(--rt-gap);
  }
  h3 {
    margin: 0;
    font-size: var(--rt-size);
    font-weight: 600;
    font-family: var(--rt-font-mono);
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(8em, max-content) minmax(0, 1fr) minmax(0, 1fr);
    gap: 4px var(--rt-gap);
    align-items: stretch;
  }
  .head {
    font-size: var(--rt-size-small);
    color: var(--rt-fg-muted);
  }
  .field {
    padding: var(--rt-gap) 0;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    overflow-wrap: anywhere;
  }
  .value {
    display: flex;
    align-items: flex-start;
    gap: var(--rt-gap);
    min-width: 0;
    padding: var(--rt-gap);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg-panel);
    cursor: pointer;
  }
  .value:hover {
    background: var(--rt-bg-hover);
  }
  .value.chosen {
    border-color: var(--rt-accent);
    background: var(--rt-bg-selected);
  }
  .value input {
    margin: 2px 0 0;
    flex: none;
  }
  .text {
    min-width: 0;
    font-family: var(--rt-font-mono);
    font-size: var(--rt-size-small);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .text.secret {
    color: var(--rt-var-secret);
    font-family: var(--rt-font-ui);
  }
  .sides {
    display: flex;
    gap: var(--rt-gap);
    flex-wrap: wrap;
  }
  .all,
  .actions {
    display: flex;
    align-items: center;
    gap: var(--rt-gap);
    flex-wrap: wrap;
  }
  .button {
    display: inline-flex;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
    white-space: nowrap;
  }
  .button:hover:not(:disabled) {
    background: var(--rt-bg-hover);
  }
  .button:disabled {
    color: var(--rt-fg-muted);
    cursor: default;
    opacity: 0.6;
  }
  .button.primary:not(:disabled) {
    background: var(--rt-accent);
    border-color: var(--rt-accent);
    color: var(--rt-accent-fg);
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
    font-size: var(--rt-size-small);
  }
  .warn {
    color: var(--rt-warn);
  }
  .error {
    color: var(--rt-danger);
  }
  @media (max-width: 640px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .head {
      display: none;
    }
  }
</style>
