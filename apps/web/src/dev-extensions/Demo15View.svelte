<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of `gitViews` (never part of a build). -->
<script lang="ts">
  import type { ExtensionGitViewProps } from "@requesttailor/ui";

  let { repository, props, close }: ExtensionGitViewProps = $props();
</script>

<div data-testid="demo15-view" style="padding: 12px; display: grid; gap: 8px">
  <p style="margin: 0" data-testid="demo15-view-repository">Repository: {repository}</p>
  <p style="margin: 0" data-testid="demo15-view-props">Props: {JSON.stringify(props)}</p>
  <div><button data-testid="demo15-view-close" onclick={close}>Close</button></div>
</div>
