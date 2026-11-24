<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of a diff editor (never part of a build). -->
<script lang="ts">
  import type { ExtensionDiffEditorProps } from "@requesttailor/ui";

  let { path, from, to, close }: ExtensionDiffEditorProps = $props();
</script>

<div data-testid="demo15-diff" style="padding: 12px; display: grid; gap: 8px">
  <p style="margin: 0" data-testid="demo15-diff-what">{path}: {from} → {to ?? "working tree"}</p>
  <div><button data-testid="demo15-diff-back" onclick={close}>Back to the standard view</button></div>
</div>
