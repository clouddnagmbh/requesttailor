<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Field conflicts of the merge in progress, resolved per field (wave 12, strand B). The state
     comes from `MergeApi.conflicts` (recomputed from the conflicted files, so a merge started
     on the command line shows too); `MergeFields` decides and resolves. The merge commit is
     made in the Git panel. -->
<script lang="ts">
  import type { MergeState } from "../commands/types";
  import ExtensionGitHint from "@tailor-kit/git-ui/ExtensionGitHint.svelte";
  import { getApp } from "../state/app.svelte";
  import MergeFields from "./MergeFields.svelte";

  let { onclose }: { onclose?: () => void } = $props();
  const app = getApp();

  let merge = $state<MergeState | null>(null);
  let error = $state<string | null>(null);
  let resolvedOnce = $state(false);

  const blocked = $derived(app.dirty.length > 0 ? "Save your changes first: resolving changes files." : null);
  const open = $derived(
    merge ? merge.conflicts.length + merge.text_conflicts.length + (merge.clean?.length ?? 0) : 0,
  );

  async function load() {
    error = null;
    try {
      merge = await app.backend.merges.conflicts();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void app.loadCount;
    void load();
  });
</script>

<div class="merge" data-testid="merge-view">
  <ExtensionGitHint target={{ place: "git", repository: app.activeRepository ?? "", view: "conflict" }} />
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if !merge}
    <p class="muted">Loading…</p>
  {:else if !merge.merging}
    <p class="muted" data-testid="merge-none">No merge is in progress. A pull that stops on conflicts shows them here.</p>
  {:else if open === 0}
    <p data-testid="merge-done">
      {resolvedOnce ? "All conflicts are resolved." : "No conflicts are left."} Commit the merge in the Git panel.
    </p>
    {#if onclose}<p><button class="button" onclick={onclose}>Close</button></p>{/if}
  {:else}
    <p class="muted">
      Both sides changed these fields. Pick a side for each; everything else is already merged.
    </p>
    <MergeFields
      {merge}
      {blocked}
      onresolved={(left) => {
        merge = left;
        resolvedOnce = true;
      }}
    />
  {/if}
</div>

<style>
  .merge {
    display: flex;
    flex-direction: column;
    gap: calc(var(--rt-gap) * 1.5);
    padding: calc(var(--rt-gap) * 2);
    max-width: 1100px;
  }
  p {
    margin: 0;
  }
  .muted {
    color: var(--rt-fg-muted);
  }
  .error {
    color: var(--rt-danger);
  }
  .button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--rt-border);
    border-radius: var(--rt-radius);
    background: var(--rt-bg);
    cursor: pointer;
  }
  .button:hover {
    background: var(--rt-bg-hover);
  }
</style>
