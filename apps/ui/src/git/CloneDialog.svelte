<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- "Clone a repository" as a dialog, reachable without an open collection (start screen,
     command palette). Wave 15: the clone joins the sidebar as a new root (CloneForm); a clone
     with collections shows the first one, `onopen` takes it when nothing was open (as after
     Open folder), a clone without any is listed as a repository root. -->
<script lang="ts">
  import { untrack } from "svelte";

  import type { CollectionInfo } from "../commands/backend";
  import type { RepositoryEntry } from "../commands/types";
  import { getApp } from "../state/app.svelte";
  import CloneForm from "@tailor-kit/git-ui/CloneForm.svelte";

  let {
    open,
    onclose,
    onopen,
  }: { open: boolean; onclose: () => void; onopen: (info: CollectionInfo) => Promise<void> } = $props();
  const app = getApp();

  // A fresh form each time the dialog opens.
  let session = $state(0);

  function opened(node: HTMLDialogElement) {
    node.showModal();
    untrack(() => session++);
    return () => node.close();
  }

  async function done(entry: RepositoryEntry) {
    const wasEmpty = app.collections.length === 0;
    onclose();
    const set = await app.backend.collections.current();
    const first = set.collections.find((c) => entry.collections.includes(c.id));
    if (wasEmpty && first) await onopen({ id: first.id, name: first.name, location: first.location });
    else await app.adoptRepository(entry);
  }
</script>

{#if open}
  <dialog
    data-testid="clone-dialog"
    aria-labelledby="clone-title"
    {@attach opened}
    oncancel={(e) => {
      e.preventDefault();
      onclose();
    }}
  >
    <h2 id="clone-title">Clone a repository</h2>
    {#key session}
      <CloneForm oncancel={onclose} ondone={done} />
    {/key}
  </dialog>
{/if}

<style>
  dialog {
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    box-sizing: border-box;
    padding: calc(var(--rt-gap) * 2);
    color: var(--rt-fg);
    background: var(--rt-bg);
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    font: inherit;
  }
  dialog::backdrop {
    background: var(--rt-bg-overlay);
  }
  h2 {
    margin: 0 0 var(--rt-gap);
    font-size: 1.1em;
  }
</style>
