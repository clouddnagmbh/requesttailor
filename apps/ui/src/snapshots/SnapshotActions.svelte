<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Snapshot tests on a response (wave 17, strand B): a request with no snapshot offers
     "Freeze as snapshot"; one with a snapshot shows whether this response matches it, and the
     chip opens the differences (`SnapshotDialog`). Freezing, accepting a new response and
     ignoring a path only stage files and the assertion (unsaved, like any edit); nothing runs in
     the background. -->
<script lang="ts">
  import type { Backend } from "../commands/backend";
  import type { Snapshot, SnapshotCheck, StoredResponse } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import { Button, Chip } from "../ui";
  import { stateLabel } from "./model";
  import SnapshotDialog from "./SnapshotDialog.svelte";

  let { backend, path, result }: { backend: Backend; path: string; result: StoredResponse } = $props();

  const app = getApp();
  let check = $state<SnapshotCheck | null>(null);
  let open = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let version = $state(0);

  $effect(() => {
    const id = result.response_id;
    const p = path;
    void version;
    if (id == null) {
      check = null;
      return;
    }
    let live = true;
    backend.snapshots
      .check(p, id)
      .then((c) => {
        if (live) check = c;
      })
      .catch(() => {
        if (live) check = null;
      });
    return () => {
      live = false;
    };
  });

  /** After a change of the snapshot: the editor's copy of the request, the unsaved marks, and
   *  the check again. */
  async function changed(assertionAdded: boolean) {
    if (assertionAdded) await app.applyExternal(path, await backend.getRequest(path));
    await app.refreshDirty();
    version += 1;
  }

  async function freeze() {
    const id = result.response_id;
    if (id == null || busy) return;
    busy = true;
    error = null;
    try {
      const done = await backend.snapshots.freeze(path, id);
      await changed(done.assertion_added);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function accept(): Promise<void> {
    const id = result.response_id;
    if (id == null) return;
    const done = await backend.snapshots.freeze(path, id);
    await changed(done.assertion_added);
  }

  async function setIgnore(ignore: string[]): Promise<Snapshot> {
    const snapshot = await backend.snapshots.setIgnore(path, ignore);
    await changed(false);
    return snapshot;
  }
</script>

{#if check}
  {#if check.state === "none" || check.state === "missing"}
    {#if check.state === "missing"}
      <Chip size="sm" icon="warning" tone="warn" label={stateLabel(check)} title={`${check.snapshot} does not exist yet`} data-testid="snapshot-badge" data-state="missing" />
    {/if}
    <Button size="sm" variant="ghost" icon="add" loading={busy} onclick={freeze} title="Keep this response as the expected one for the request" data-testid="snapshot-freeze">
      Freeze as snapshot
    </Button>
  {:else}
    <Chip
      size="sm"
      icon={check.state === "matches" ? "spec-ok" : "warning"}
      tone={check.state === "matches" ? "ok" : "warn"}
      label={stateLabel(check)}
      title={check.snapshot}
      onclick={() => (open = true)}
      data-testid="snapshot-badge"
      data-state={check.state}
      data-count={check.differences.length}
    />
  {/if}
  {#if error}<span class="error" role="alert" data-testid="snapshot-error">{error}</span>{/if}
  {#if open && check.state !== "none" && check.state !== "missing"}
    <SnapshotDialog {backend} {path} {result} {check} {accept} {setIgnore} onclose={() => (open = false)} />
  {/if}
{/if}

<style>
  .error {
    color: var(--rt-danger);
    font: var(--rt-font-small);
  }
</style>
