<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Dev-server demo of the extension points (never part of a build). -->
<script lang="ts">
  import type { ExtensionTabProps } from "@requesttailor/ui";

  let { requestPath }: ExtensionTabProps = $props();
</script>

<div data-testid="demo-tab" style="padding: 12px">
  <p style="margin: 0">{requestPath === "" ? "Demo collection tab" : `Demo request tab for ${requestPath}`}</p>
</div>
