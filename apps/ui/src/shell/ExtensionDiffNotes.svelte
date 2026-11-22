<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The diff notes of a distribution's extensions (`AppExtension.diffNotes`, since wave 13) for
     a range of revisions, in extension order. Nothing outside the shell or without extensions. -->
<script lang="ts">
  import { extensionDiffNotes, getExtensions } from "./extension-tabs";

  let { from, to }: { from: string; to: string } = $props();
  const provided = getExtensions();
  const notes = extensionDiffNotes(provided);
</script>

{#if provided}
  {#each notes as n (n.extension)}
    <div data-testid="extension-diff-notes" data-extension={n.extension}>
      <n.component ctx={provided.ctx} {from} {to} />
    </div>
  {/each}
{/if}
