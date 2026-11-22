<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- Start screen: collections opened before on this device, one click to reopen
     (`backend.openRecent(id, true)`). Owner: wave 4, strand C. -->
<script lang="ts">
  import type { CollectionInfo } from "../commands/backend";

  let _props: { onopen: (info: CollectionInfo) => Promise<void> } = $props();

</script>
