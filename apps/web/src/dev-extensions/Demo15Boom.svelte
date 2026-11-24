<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo: a Git view that throws while it renders (never part of a build). -->
<script lang="ts">
  import type { ExtensionGitViewProps } from "@requesttailor/ui";

  let { props }: ExtensionGitViewProps = $props();

  function boom(): string {
    throw new Error("Demo view broke");
  }
</script>

<p>{props.boom === false ? "Fine." : boom()}</p>
