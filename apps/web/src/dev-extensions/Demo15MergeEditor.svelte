<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of a merge editor (never part of a build): resolves with "their" text, and
     calls `close()` after `resolve()` as an editor that does not know the host closes. -->
<script lang="ts">
  import type { ExtensionMergeEditorProps } from "@requesttailor/ui";

  let { conflict, resolve, close }: ExtensionMergeEditorProps = $props();
  let error = $state<string | null>(null);

  async function take() {
    error = null;
    try {
      await resolve(conflict.theirs ?? "");
      close();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div data-testid="demo15-merge" style="padding: 12px; display: grid; gap: 8px">
  <p style="margin: 0">Demo merge editor for {conflict.path}</p>
  <div><button data-testid="demo15-merge-theirs" onclick={take}>Take theirs</button></div>
  {#if error}<p role="alert" data-testid="demo15-merge-error" style="margin: 0">{error}</p>{/if}
</div>
