<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of `staging` (never part of a build): lists the changed files, each with
     the state it has in the index. -->
<script lang="ts">
  import type { ExtensionStagingProps } from "@requesttailor/ui";

  let { workingCopy, refresh }: ExtensionStagingProps = $props();
</script>

<div data-testid="demo15-staging" style="display: grid; gap: 4px">
  {#each workingCopy.files as f (f.path)}
    <div data-testid="demo15-staging-file" data-path={f.path} data-staged={f.staged ? "yes" : "no"}>
      {f.staged ? "[x]" : "[ ]"} {f.path}
    </div>
  {/each}
  <div><button data-testid="demo15-staging-refresh" onclick={() => void refresh()}>Refresh</button></div>
</div>
