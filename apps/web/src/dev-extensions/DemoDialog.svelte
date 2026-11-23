<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of `AppState.showDialog` (never part of a build). -->
<script lang="ts">
  import type { ExtensionDialogProps } from "@requesttailor/ui";

  let { close, props }: ExtensionDialogProps = $props();
  let name = $state("");
</script>

<form
  style="display: grid; gap: 8px"
  onsubmit={(e) => {
    e.preventDefault();
    close(name);
  }}
>
  <p style="margin: 0">{String(props.message ?? "Say something.")}</p>
  <input data-testid="demo-input" aria-label="Name" bind:value={name} style="font: inherit; padding: 4px 6px" />
  <div style="display: flex; justify-content: flex-end; gap: 8px">
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" data-testid="demo-done">Done</button>
  </div>
</form>
