<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The "New…" dialog (wave 14, strand B): tiles for every kind (./kinds.ts) and the tiles of
     extensions (./items.ts), then a short form per kind. Opened with `AppState.openNew`
     (Mod+N, palette, sidebar). Interface stub: hands the request to the inline "new request"
     prompt (`onfallback`) as before wave 14. -->
<script lang="ts">
  import { untrack } from "svelte";

  import { getApp } from "../state/app.svelte";

  let { onfallback }: { onfallback?: (folder: string) => void } = $props();

  const app = getApp();

  $effect(() => {
    const request = app.newDialog;
    if (!request) return;
    untrack(() => {
      app.closeNew();
      if (app.collection) onfallback?.(request.folder);
    });
  });
</script>
