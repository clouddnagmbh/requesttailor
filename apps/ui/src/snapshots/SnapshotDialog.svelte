<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The comparison of a response with the request's snapshot: the body as a JSON tree diff
     (`compare/JsonTreeDiff`), every difference with a click to ignore its path (or the path in
     every array element), the ignored paths, and "Accept new response", which replaces the
     snapshot with this response (the diff above is the preview). Nothing is written until the
     user saves the collection. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { Snapshot, SnapshotCheck, StoredResponse } from "../commands/types";
  import JsonTreeDiff from "../compare/JsonTreeDiff.svelte";
  import { Button, Dialog, DialogFooter } from "../ui";
  import { hasIndex, ignorePath, show, stateLabel, withIgnore, without } from "./model";

  let {
    backend,
    path,
    result,
    check,
    accept,
    setIgnore,
    onclose,
  }: {
    backend: Backend;
    path: string;
    result: StoredResponse;
    check: SnapshotCheck;
    accept: () => Promise<void>;
    setIgnore: (ignore: string[]) => Promise<Snapshot>;
    onclose: () => void;
  } = $props();

  let snapshot = $state<Snapshot | null>(null);
  let body = $state<{ value: unknown } | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  // svelte-ignore state_referenced_locally
  const id = result.response_id!;

  $effect(() => {
    let live = true;
    (async () => {
      const s = await backend.snapshots.read(path);
      let parsed: { value: unknown } | null = null;
      try {
        const bytes = await backend.responseBytes(id, 0, result.size);
        parsed = { value: JSON.parse(new TextDecoder().decode(bytes)) as unknown };
      } catch {
        parsed = null;
      }
      if (live) {
        snapshot = s;
        body = parsed;
      }
    })().catch((e: unknown) => {
      if (live) error = e instanceof Error ? e.message : String(e);
    });
    return () => {
      live = false;
    };
  });

  const ignore = $derived(snapshot?.ignore ?? []);
  const oldBody = $derived(snapshot && !(typeof snapshot.body === "object" && snapshot.body !== null && "$digest" in snapshot.body) ? snapshot.body : undefined);
  const showTree = $derived(snapshot !== null && body !== null && oldBody !== undefined);

  async function run(f: () => Promise<unknown>) {
    busy = true;
    error = null;
    try {
      await f();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const add = (diffPath: string, everyItem: boolean) =>
    run(async () => {
      snapshot = await setIgnore(withIgnore(ignore, ignorePath(diffPath, everyItem)));
    });
  const remove = (p: string) =>
    run(async () => {
      snapshot = await setIgnore(without(ignore, p));
    });
  const acceptNew = () =>
    run(async () => {
      await accept();
      onclose();
    });
</script>

<Dialog title="Response against the snapshot" description={`${path} · ${check.snapshot}`} icon="spec-ok" size="xl" testid="snapshot-dialog" {onclose}>
  <p class="state" data-testid="snapshot-state">{stateLabel(check)}</p>
  {#if error}<p class="error" role="alert" data-testid="snapshot-dialog-error">{error}</p>{/if}

  {#if check.differences.length > 0}
    <table aria-label="Differences">
      <thead><tr><th>Where</th><th>Snapshot</th><th>Response</th><th></th></tr></thead>
      <tbody>
        {#each check.differences as d (d.path)}
          <tr data-testid="snapshot-difference" data-path={d.path} data-kind={d.kind}>
            <td><code>{d.path}</code></td>
            <td><code>{show(d.expected)}</code></td>
            <td><code>{show(d.actual)}</code></td>
            <td class="actions">
              <Button size="sm" variant="ghost" disabled={busy} onclick={() => add(d.path, false)} data-testid="snapshot-ignore">Ignore</Button>
              {#if hasIndex(d.path)}
                <Button size="sm" variant="ghost" disabled={busy} onclick={() => add(d.path, true)} data-testid="snapshot-ignore-all">Ignore in every item</Button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if showTree && snapshot && body}
    <h3>Body</h3>
    <JsonTreeDiff old={oldBody} now={body.value} />
  {/if}

  <h3>Ignored paths</h3>
  {#if ignore.length === 0}
    <p class="muted" data-testid="snapshot-no-ignore">Nothing is ignored. Volatile values (ids, timestamps) can be ignored from the differences above.</p>
  {:else}
    <ul class="ignored" aria-label="Ignored paths">
      {#each ignore as p (p)}
        <li data-testid="snapshot-ignored"><code>{p}</code> <Button size="sm" variant="ghost" icon="close" disabled={busy} onclick={() => remove(p)} aria-label={`Stop ignoring ${p}`} /></li>
      {/each}
    </ul>
  {/if}

  <DialogFooter>
    <Button variant="ghost" onclick={onclose}>Close</Button>
    <Button variant="primary" loading={busy} onclick={acceptNew} data-testid="snapshot-accept">Accept new response</Button>
  </DialogFooter>
</Dialog>

<style>
  .state {
    margin: 0 0 var(--rt-space-4);
    font-weight: 600;
  }
  .error {
    color: var(--rt-danger);
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  h3 {
    margin: var(--rt-space-6) 0 var(--rt-space-3);
    font: var(--rt-font-caption);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--rt-fg-muted);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font: var(--rt-font-small);
  }
  th {
    padding: var(--rt-space-3) var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
    color: var(--rt-fg-muted);
    font: var(--rt-font-caption);
    text-align: left;
    text-transform: uppercase;
  }
  td {
    padding: var(--rt-space-3) var(--rt-space-4);
    border-bottom: 1px solid var(--rt-border);
    vertical-align: top;
    overflow-wrap: anywhere;
  }
  td.actions {
    white-space: nowrap;
    text-align: right;
  }
  code {
    font-family: var(--rt-font-mono);
    font-size: 12px;
  }
  .ignored {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .ignored li {
    display: flex;
    align-items: center;
    gap: var(--rt-space-3);
    padding: var(--rt-space-1) 0;
  }
</style>
