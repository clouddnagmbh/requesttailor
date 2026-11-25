<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The checks of one response in a dialog (the chip's click). -->
<script lang="ts">
  import type { StoredResponse } from "../commands/types";
  import type { ExtensionDialogProps } from "../extensions";
  import { getApp } from "../state/app.svelte";
  import ChecksTab from "./ChecksTab.svelte";

  let { props }: ExtensionDialogProps = $props();
  const app = getApp();
  // svelte-ignore state_referenced_locally
  const { requestPath, response } = props as { requestPath: string; response: StoredResponse };
</script>

<ChecksTab {app} {requestPath} {response} />
